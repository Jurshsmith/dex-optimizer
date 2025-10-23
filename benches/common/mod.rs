use anyhow::{Context, Result};
use optimizer::{dataset, edge_layouts::EdgeAoS};

pub const TARGET_EDGE_COUNT: usize = 50_000;
pub const FEE_BPS: f64 = 30.0;

pub fn load_edges(target_len: usize) -> Result<Vec<EdgeAoS>> {
    let dataset = dataset::load_default_dataset()?;
    anyhow::ensure!(!dataset.edges.is_empty(), "dataset contains no edges");

    dataset
        .edges
        .iter()
        .cycle()
        .take(target_len)
        .map(|edge| {
            Ok(EdgeAoS::new(
                usize::try_from(edge.from).context("from node index does not fit in usize")?,
                usize::try_from(edge.to).context("to node index does not fit in usize")?,
                edge.rate,
            ))
        })
        .collect()
}
