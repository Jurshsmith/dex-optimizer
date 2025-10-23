use anyhow::Result;
use optimizer::edge_layouts::EdgeAoS;
use std::{hint::black_box, time::Instant};

#[path = "common/mod.rs"]
mod common;

use common::{load_edges, FEE_BPS, TARGET_EDGE_COUNT};

pub fn run() -> Result<()> {
    let edges = load_edges(TARGET_EDGE_COUNT)?;
    run_benchmark(edges);
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
fn main() -> Result<()> {
    run()
}

pub fn run_benchmark(mut edges: Vec<EdgeAoS>) {
    let fee_multiplier = 1.0 - FEE_BPS / 10_000.0;
    let start = Instant::now();

    let mut checksum = 0.0;
    for edge in edges.iter_mut() {
        edge.rate *= fee_multiplier;
        checksum = black_box(checksum + edge.rate);
    }

    let elapsed = start.elapsed();
    println!(
        "AoS elapsed={:.4}ms checksum={:.6}",
        elapsed.as_secs_f64() * 1_000.0,
        checksum
    );
}
