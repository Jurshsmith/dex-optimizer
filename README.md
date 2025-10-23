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
