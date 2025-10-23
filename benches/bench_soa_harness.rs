#[path = "bench_soa.rs"]
mod bench_soa;

fn main() {
    if let Err(err) = bench_soa::run() {
        eprintln!("bench_soa failed: {err:?}");
        std::process::exit(1);
    }
}
