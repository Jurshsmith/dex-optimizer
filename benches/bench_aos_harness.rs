#[path = "bench_aos.rs"]
mod bench_aos;

fn main() {
    if let Err(err) = bench_aos::run() {
        eprintln!("bench_aos failed: {err:?}");
        std::process::exit(1);
    }
}
