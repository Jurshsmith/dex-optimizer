# Dex Route Optimizer

Dex Route Optimizer surfaces short-hop arbitrage opportunities across token graphs while keeping the data plane hot and consistent. It combines a CSR-backed cycle finder, an async update pipeline, and a fused numerical kernel that keeps multiplicative adjustments stable in log space.

Benchmarks compare array-of-structs versus struct-of-arrays layouts, and the included dataset harness makes it easy to replay historical pools or synthetic stress tests.

## Working on the project

### Requirements

- Rust 1.74+ installed via `rustup` (the pipeline and benches target stable and exercise async + tokio features).
- `cargo fmt` and `clippy` components: `rustup component add rustfmt clippy` keeps tooling in sync with CI.
- `datasets/dataset.json` or another dataset in the `datasets/` directory—copy your pool snapshots here if you want to replay different markets.
- Optional: `perf`/`dtrace` for deeper latency profiling when running the pipeline bench in release mode.

### Quick start

- `cargo build` compiles the workspace.
- `cargo test` runs the unit and integration suites, including the numerical kernel checks.
- `cargo run` executes the async pipeline against `datasets/dataset.json`.

### Hygiene checks

- `cargo fmt --check` keeps the diff clean before pushing anything.
- `cargo clippy -- -D warnings` has surfaced a couple of edge cases already, so I treat warnings as blockers.
- `cargo test` covers unit + integration tests, including the asynchronous pipeline scenarios; a clean pass reports `running … tests` followed by `test result: ok`.
- `cargo bench --bench pipeline` exercises the end-to-end async loop—look for the `time: [...]` summary to confirm hop-cap latency and throughput regressions.

### Runnable Binaries

- `cargo run` loads `datasets/dataset.json`, runs the pipeline for a minute, and logs a short stats summary.
- `cargo run --release --bin bench_aos` and `cargo run --release --bin bench_soa` recreate the lightweight layout microbenchmarks from the assignment.
- `cargo bench --bench pipeline` runs an end-to-end benchmark to see how hop caps influence latency in the async pipeline.

## Design Notes

### Cycle Finder

Goal: Given a list of edges (a graph) representing the tokens as nodes and conversion rates as weights, find profitable arbitrage loops. We first turn multiplicative rates into additive costs with a negative log to avoid overflows or underflows when doing the product operation for tiny or large float numbers.

Graph layout: We pack the graph in a Compressed Sparse Row (CSR) format. Think of CSR as a compact index that lets us grab “all neighbors of node u” in one cache-friendly slice. It keeps memory tight and scans fast, which is exactly what we need for repeated neighbor lookups.

#### Search strategy (Bellman–Ford, with a hop cap):

- We use a Bellman–Ford-style dynamic program because it handles negative costs (unlike Dijkstra).
- We treat every node as a potential start. For a chosen start S, we run the search for exactly 1 hop, 2 hops, … up to H hops (the hop cap).
- After each hop h, we ask a simple question:
  “Can we get back to S in h steps with a negative total cost?”
  If yes, we’ve found a profitable cycle of length h and we stop early (shortest profitable loop wins).
  - To keep it speedy, we reuse a small set of buffers “in place” each hop (no constant reallocating).

Why the hop cap? Production graphs can be huge. The hop cap bounds the work while still surfacing short, high-quality opportunities (which are usually the most actionable).

Remembering what we chose. Each time a node’s best cost improves at hop h, we record the predecessor edge that achieved it. When we detect a cycle, we walk those predecessors backward for exactly h steps, then reverse the list to get the cycle in the forward order.

### DEX Optimizer Pipeline

Goal: Keep a shared CSR graph hot with streaming rate edits while the cycle searcher polls for new opportunities, so we surface arbitrage loops quickly without dropping updates.

#### Architecture

- `producer` sends jittered rate updates into an MPSC channel, `writer` drains batches of those updates and mutates the shared graph, `searcher` reads snapshots on a cadence, and `run` ties their lifecycles together with one-shot shutdown.
- The shared graph lives behind an `Arc<RwLock<_>>`, so the writer takes the write lock for short, bounded stretches while the searcher clones a read-locked snapshot before running the heavier cycle detection.

#### Update flow and safeguards

- Batching (`max_coalesce`, `coalesce_window`) lets the writer amortize lock traffic: we validate and clamp the batch once, apply everything while holding the lock, then release.
- Validation keeps upstream bugs visible—out-of-range indexes and non-finite rates bump dedicated counters instead of silently mutating state, and rates are clamped to configured bounds before the graph sees them.
- The producer respects `max_updates`, sleeps relative to `search_interval`, and jittered rates stay within those same bounds via the shared `RateBounds` helper.

#### Search cadence and shutdown

- The searcher ticks on `search_interval`, incrementing a `PipelineStats` counter every pass and stashing the most recent profitable cycle (if any) for the caller.
- On shutdown we send a one-shot, let the writer finish naturally, then force one last search so the stats reflect any late-breaking win before returning.

### Data Layout (AoS vs SoA)

On my Mac M4 summary: 50k–100k edges AoS ≈ SoA (~0.17–0.35 ms, equal checksums); at 1M edges SoA is ~18% faster (AoS 4.1258 ms vs SoA 3.3779 ms).

#### Explanation

- Small sizes fit in cache and both loops linearly touch only rate; M‑series prefetchers make AoS’s fixed‑stride rate loads as cache‑friendly as SoA’s.
- The kernel is compute‑light (one multiply + add), so at 50k–100k it isn’t bandwidth‑bound and the compiler can generate similarly efficient loops for both layouts.
- At 1M, the loop becomes bandwidth‑bound: SoA packs rates densely (more useful values per cache line), while AoS drags unused from/to fields, wasting memory bandwidth.
- AoS may win when you need from/to/rate together per edge, where whole‑object locality avoids hopping across multiple arrays.

### Numerical Kernel (`log_kernel`)

Goal: Fuse Clamp → Multiply → Quantize (linear) → Log → Gate into one steady, branch-light step. Inputs and outputs live in log space, so even near-1.0 nudges keep their precision.

#### Short Explanation

The bits that make the kernel fast and steady:

- We keep state in log space and quantize in the linear domain where real ticks live, then map back with a near‑1.0‑aware `ln_1p` path to preserve sub‑ULP precision.
- Clamp bounds stay ≥ `f64::MIN_POSITIVE`, avoiding `ln(0)` and denormal slow paths when values hug the floor.
- We floor the quantum to `max(1e-12, ~1 ULP @ lo)`, so every tick reflects a real move instead of disappearing into sub‑ULP noise.
- Rounding uses ties‑to‑even with a small ULP slack, which reduces long‑run drift and prevents bouncing between adjacent bins.
- The epsilon gate operates in log units, so it scales with price—micro‑jitter gets filtered without masking real movement.
- NaN/Inf snap to bounds; we sanitise up front and keep the rest as straight‑line math that’s idempotent when reapplied.
