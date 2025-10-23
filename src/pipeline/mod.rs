mod config;
mod producer;
mod searcher;
mod stats;
mod types;
mod writer;

pub use crate::error::PipelineError;
pub use config::PipelineConfig;
pub use stats::PipelineStats;

use crate::{
    csr_graph::{CSRGraph, InputEdge},
    dataset::Dataset,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, instrument};

use types::{GraphUpdate, SharedGraph};

#[instrument(name = "pipeline_run", level = "debug", skip_all)]
pub async fn run(dataset: Dataset, config: PipelineConfig) -> Result<PipelineStats, PipelineError> {
    if dataset.edges.is_empty() {
        return Err(PipelineError::EmptyDataset);
    }

    let mut graph_edges: Vec<InputEdge> = Vec::with_capacity(dataset.edges.len());
    let mut baseline_rates = Vec::with_capacity(dataset.edges.len());
    let mut highest_node_index = 0usize;

    for edge in &dataset.edges {
        let from = usize::try_from(edge.from).map_err(|source| PipelineError::FromIndex {
            edge_id: edge.id,
            source,
        })?;
        let to = usize::try_from(edge.to).map_err(|source| PipelineError::ToIndex {
            edge_id: edge.id,
            source,
        })?;
        if !edge.rate.is_finite() || edge.rate <= 0.0 {
            return Err(PipelineError::InvalidRate {
                edge_id: edge.id,
                rate: edge.rate,
            });
        }
        graph_edges.push((from, to, edge.rate));
        baseline_rates.push(edge.rate);
        highest_node_index = highest_node_index.max(from.max(to));
    }

    info!(
        edge_count = graph_edges.len(),
        node_count = highest_node_index + 1,
        "initialised pipeline state"
    );

    let node_count = highest_node_index + 1;
    let shared_graph: SharedGraph =
        Arc::new(RwLock::new(CSRGraph::from_edges(node_count, graph_edges)));

    let (update_sender, update_receiver) = mpsc::channel::<GraphUpdate>(config.channel_capacity);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    info!("spawning writer task");
    let writer_handle = writer::start(Arc::clone(&shared_graph), update_receiver, config.clone());

    info!("spawning searcher task");
    let search_handle = searcher::start(Arc::clone(&shared_graph), shutdown_rx, config.clone());

    info!("spawning producer task");
    let producer_handle = producer::start(update_sender, baseline_rates, config.clone());

    info!("awaiting producer task completion");
    producer_handle.await.map_err(PipelineError::ProducerJoin)?;
    info!("producer task completed");

    let writer_outcome = writer_handle.await.map_err(PipelineError::WriterJoin)?;
    info!(
        processed_updates = writer_outcome.processed_updates,
        unique_updates_applied = writer_outcome.unique_updates_applied,
        invalid_index_updates = writer_outcome.invalid_index_updates,
        invalid_rate_updates = writer_outcome.invalid_rate_updates,
        "writer task completed"
    );

    let _ = shutdown_tx.send(());
    let search_outcome = search_handle.await.map_err(PipelineError::SearcherJoin)?;
    if let Some(ref cycle) = search_outcome.last_cycle {
        info!(
            searches_run = search_outcome.searches_run,
            cycle_profit = cycle.profit,
            cycle_neg_log = cycle.neg_log_sum,
            vertices = ?cycle.vertices,
            edge_indexes = ?cycle.edge_indexes,
            "searcher task completed with profitable cycle"
        );
    } else {
        info!(
            searches_run = search_outcome.searches_run,
            found_cycle = false,
            "searcher task completed"
        );
    }

    Ok(PipelineStats {
        updates_processed: writer_outcome.processed_updates,
        unique_updates_applied: writer_outcome.unique_updates_applied,
        searches_run: search_outcome.searches_run,
        last_cycle: search_outcome.last_cycle,
        invalid_index_updates: writer_outcome.invalid_index_updates,
        invalid_rate_updates: writer_outcome.invalid_rate_updates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, Edge, Token};
    use std::time::Duration;

    fn triangular_arbitrage_dataset() -> Dataset {
        Dataset {
            tokens: vec![
                Token {
                    id: 0,
                    symbol: "A".into(),
                },
                Token {
                    id: 1,
                    symbol: "B".into(),
                },
                Token {
                    id: 2,
                    symbol: "C".into(),
                },
            ],
            edges: vec![
                Edge {
                    id: 0,
                    from: 0,
                    to: 1,
                    rate: 1.10,
                    pool_id: 0,
                    kind: 0,
                },
                Edge {
                    id: 1,
                    from: 1,
                    to: 2,
                    rate: 1.05,
                    pool_id: 0,
                    kind: 0,
                },
                Edge {
                    id: 2,
                    from: 2,
                    to: 0,
                    rate: 0.98,
                    pool_id: 0,
                    kind: 0,
                },
            ],
        }
    }

    fn acyclic_dataset() -> Dataset {
        Dataset {
            tokens: vec![
                Token {
                    id: 0,
                    symbol: "A".into(),
                },
                Token {
                    id: 1,
                    symbol: "B".into(),
                },
            ],
            edges: vec![
                Edge {
                    id: 0,
                    from: 0,
                    to: 1,
                    rate: 0.99,
                    pool_id: 0,
                    kind: 0,
                },
                Edge {
                    id: 1,
                    from: 1,
                    to: 0,
                    rate: 0.99,
                    pool_id: 0,
                    kind: 0,
                },
            ],
        }
    }

    fn invalid_rate_dataset() -> Dataset {
        Dataset {
            tokens: vec![Token {
                id: 0,
                symbol: "A".into(),
            }],
            edges: vec![Edge {
                id: 0,
                from: 0,
                to: 0,
                rate: 0.0,
                pool_id: 0,
                kind: 0,
            }],
        }
    }

    fn quick_config(max_updates: usize) -> PipelineConfig {
        PipelineConfig {
            max_updates,
            channel_capacity: 8,
            hop_cap: 4,
            search_interval: Duration::from_millis(5),
            coalesce_window: Duration::from_millis(1),
            max_coalesce: 4,
            rate_jitter: 0.0,
            ..PipelineConfig::default()
        }
    }

    #[tokio::test]
    async fn pipeline_consumes_expected_number_of_updates() {
        let dataset = triangular_arbitrage_dataset();
        let stats = run(dataset, quick_config(32))
            .await
            .expect("pipeline runs without error");

        assert_eq!(stats.updates_processed, 32);
        assert!(
            stats.unique_updates_applied <= stats.updates_processed,
            "unique applied updates should never exceed processed"
        );
        assert!(
            stats.searches_run >= 1,
            "expected at least one search pass; got {}",
            stats.searches_run
        );
        assert_eq!(
            stats.invalid_index_updates, 0,
            "unexpected out-of-bounds updates recorded"
        );
        assert_eq!(
            stats.invalid_rate_updates, 0,
            "unexpected invalid rate updates recorded"
        );
    }

    #[tokio::test]
    async fn pipeline_reports_last_cycle_when_one_exists() {
        let dataset = triangular_arbitrage_dataset();
        let stats = run(
            dataset,
            PipelineConfig {
                max_updates: 16,
                channel_capacity: 4,
                hop_cap: 4,
                search_interval: Duration::from_millis(2),
                coalesce_window: Duration::from_millis(1),
                max_coalesce: 4,
                rate_jitter: 0.0,
                ..PipelineConfig::default()
            },
        )
        .await
        .expect("pipeline has a profitable cycle to report");

        assert!(stats.last_cycle.is_some(), "expected a profitable cycle");
        assert!(
            stats.unique_updates_applied <= stats.updates_processed,
            "unique applied updates should never exceed processed"
        );
        assert_eq!(
            stats.invalid_index_updates, 0,
            "unexpected out-of-bounds updates recorded"
        );
        assert_eq!(
            stats.invalid_rate_updates, 0,
            "unexpected invalid rate updates recorded"
        );
    }

    #[tokio::test]
    async fn pipeline_runs_search_even_without_cycle() {
        let dataset = acyclic_dataset();
        let stats = run(dataset, quick_config(24))
            .await
            .expect("pipeline should still run on acyclic graph");

        assert!(
            stats.searches_run > 0,
            "pipeline should keep checking even when no profitable cycle exists"
        );
        assert!(
            stats.last_cycle.is_none(),
            "acyclic dataset should not yield a profitable cycle"
        );
        assert!(
            stats.unique_updates_applied <= stats.updates_processed,
            "unique applied updates should never exceed processed"
        );
        assert_eq!(
            stats.invalid_index_updates, 0,
            "acyclic run should not drop updates for index reasons"
        );
        assert_eq!(
            stats.invalid_rate_updates, 0,
            "acyclic run should not drop updates for rate reasons"
        );
    }

    #[tokio::test]
    async fn pipeline_handles_zero_updates_gracefully() {
        let dataset = triangular_arbitrage_dataset();
        let stats = run(dataset, quick_config(0))
            .await
            .expect("pipeline runs even when producer has nothing to send");

        assert_eq!(stats.updates_processed, 0);
        assert_eq!(stats.unique_updates_applied, 0);
        assert!(
            stats.searches_run >= 1,
            "searcher should take at least one snapshot on shutdown"
        );
        assert_eq!(
            stats.invalid_index_updates, 0,
            "no updates implies no index errors"
        );
        assert_eq!(
            stats.invalid_rate_updates, 0,
            "no updates implies no rate errors"
        );
    }

    #[tokio::test]
    async fn pipeline_rejects_empty_dataset() {
        let dataset = Dataset {
            tokens: vec![],
            edges: vec![],
        };
        let err = run(dataset, quick_config(4))
            .await
            .expect_err("empty dataset should not start the pipeline");

        assert!(matches!(err, PipelineError::EmptyDataset));
    }

    #[tokio::test]
    async fn pipeline_rejects_invalid_edge_rate() {
        let dataset = invalid_rate_dataset();
        let err = run(dataset, quick_config(4))
            .await
            .expect_err("invalid rate should cause the pipeline to abort");

        assert!(matches!(err, PipelineError::InvalidRate { .. }));
    }

    #[tokio::test]
    async fn pipeline_handles_bursty_producer() {
        let dataset = triangular_arbitrage_dataset();
        let stats = run(
            dataset,
            PipelineConfig {
                max_updates: 64,
                channel_capacity: 4,
                hop_cap: 6,
                search_interval: Duration::from_millis(5),
                coalesce_window: Duration::from_millis(8),
                max_coalesce: 16,
                rate_jitter: 0.05,
                ..PipelineConfig::default()
            },
        )
        .await
        .expect("bursty producer should still succeed");

        assert!(
            stats.updates_processed >= 16,
            "expected a reasonable portion of bursts to land"
        );
        assert!(
            stats.unique_updates_applied <= stats.updates_processed,
            "unique applied updates should never exceed processed"
        );
        assert!(
            stats.searches_run >= 1,
            "searcher should still run during bursty traffic"
        );
    }
}
