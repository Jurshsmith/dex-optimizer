# Dex Route Optimizer

This repository contains a DEX arbitrage finder, data layout tricks, a numerical kernel, and async plumbing.

## Working on the project

### Quick start

- `cargo build` compiles the workspace.
- `cargo test` runs the unit and integration suites, including the numerical kernel checks.
- `cargo run` executes the async pipeline against `datasets/dataset.json`.

### Hygiene checks

- `cargo fmt --check` keeps the diff clean before pushing anything.
- `cargo clippy -- -D warnings` has surfaced a couple of edge cases already, so I treat warnings as blockers.
- `cargo test` covers unit + integration tests, including the asynchronous pipeline scenarios.

### Runnable Binaries

- `cargo run` loads `datasets/dataset.json`, runs the pipeline for a minute, and prints a short stats summary.
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

#### Task topology

- `producer` sends jittered rate updates into an MPSC channel, `writer` drains batches of those updates and mutates the shared graph, `searcher` reads snapshots on a cadence, and `run` ties their lifecycles together with one-shot shutdown.
- The shared graph lives behind an `Arc<RwLock<_>>`, so the writer takes the write lock for short, bounded stretches while the searcher clones a read-locked snapshot before running the heavier cycle detection.

#### Update flow and safeguards

- Batching (`max_coalesce`, `coalesce_window`) lets the writer amortize lock traffic: we validate and clamp the batch once, apply everything while holding the lock, then release.
- Validation keeps upstream bugs visible—out-of-range indexes and non-finite rates bump dedicated counters instead of silently mutating state, and rates are clamped to configured bounds before the graph sees them.
- The producer respects `max_updates`, sleeps relative to `search_interval`, and jittered rates stay within those same bounds via the shared `RateBounds` helper.

#### Search cadence and shutdown

- The searcher ticks on `search_interval`, incrementing a `PipelineStats` counter every pass and stashing the most recent profitable cycle (if any) for the caller.
- On shutdown we send a one-shot, let the writer finish naturally, then force one last search so the stats reflect any late-breaking win before returning.

### Data Layout

On my Mac M4 summary: 50k–100k edges AoS ≈ SoA (~0.17–0.35 ms, equal checksums); at 1M edges SoA is ~18% faster (AoS 4.1258 ms vs SoA 3.3779 ms).

#### Explanation

- Small sizes fit in cache and both loops linearly touch only rate; M‑series prefetchers make AoS’s fixed‑stride rate loads as cache‑friendly as SoA’s.
- The kernel is compute‑light (one multiply + add), so at 50k–100k it isn’t bandwidth‑bound and the compiler can generate similarly efficient loops for both layouts.
- At 1M, the loop becomes bandwidth‑bound: SoA packs rates densely (more useful values per cache line), while AoS drags unused from/to fields, wasting memory bandwidth.
- AoS may win when you need from/to/rate together per edge, where whole‑object locality avoids hopping across multiple arrays.

### Numerical Kernel

Goal: Fuse Clamp → Multiply → Quantize (linear) → Log → Gate into one stable step. Inputs/outputs are log-space so tiny multiplicative updates stay accurate.

#### Short Explanation

The bits that make the kernel fast and steady:

- We keep state in log space but quantize in linear space (where real ticks live), then hop back using a near-1.0-aware `log1p` path to preserve precision.
- Bounds are forced ≥ `f64::MIN_POSITIVE`, so nothing ever hits denormals or ln(0) slow paths even when inputs crowd the floor.
- The quantum is floored to `max(1e-12, ~1 ULP at lo)`, turning every update into a meaningful step and eliminating sub-ULP no-ops.
- Rounding uses ties-to-even with a tiny ULP-scaled deadband, which kills long-run bias and keeps boundary oscillations from flapping.
- The epsilon gate is expressed in log units, so it scales multiplicatively and quashes micro-jitter without hiding real price movement.
- NaN/Inf cases snap to bounds for deterministic outcomes, and the whole routine stays branch-lean—sanitize once, then straight-line math—so it plays nicely with SIMD and remains idempotent when re-applied.
