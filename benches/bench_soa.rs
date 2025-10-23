use anyhow::Result;
use optimizer::edge_layouts::EdgeSoA;
use std::{hint::black_box, time::Instant};

#[path = "common/mod.rs"]
mod common;

use common::{load_edges, FEE_BPS, TARGET_EDGE_COUNT};

pub fn run() -> Result<()> {
    let edges = load_edges(TARGET_EDGE_COUNT)?;
    let soa = EdgeSoA::from(edges);
    run_benchmark(soa);
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
fn main() -> Result<()> {
    run()
}

pub fn run_benchmark(mut soa: EdgeSoA) {
    let fee_multiplier = 1.0 - FEE_BPS / 10_000.0;
    let start = Instant::now();

    let mut checksum = 0.0;
    for rate in soa.rate.iter_mut() {
        *rate *= fee_multiplier;
        checksum = black_box(checksum + *rate);
    }

    let elapsed = start.elapsed();
    println!(
        "SoA elapsed={:.4}ms checksum={:.6}",
        elapsed.as_secs_f64() * 1_000.0,
        checksum
    );
}
