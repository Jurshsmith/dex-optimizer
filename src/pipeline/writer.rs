use super::{
    config::{PipelineConfig, RateBounds},
    types::{GraphUpdate, SharedGraph, UpdateValidationError, WriterOutcome},
};
use std::time::Duration;
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{timeout_at, Instant},
};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tracing::{debug, error, info, instrument, warn};

pub(super) fn start(
    shared_edges: SharedGraph,
    receiver: mpsc::Receiver<GraphUpdate>,
    config: PipelineConfig,
) -> JoinHandle<WriterOutcome> {
    tokio::spawn(writer_task(
        shared_edges,
        ReceiverStream::new(receiver),
        config,
    ))
}

#[instrument(
    name = "pipeline_writer",
    level = "debug",
    skip_all,
    fields(
        max_coalesce = config.max_coalesce,
        coalesce_window_ms = config.coalesce_window.as_millis()
    )
)]
async fn writer_task(
    shared_edges: SharedGraph,
    mut update_stream: ReceiverStream<GraphUpdate>,
    config: PipelineConfig,
) -> WriterOutcome {
    let edge_count = shared_edges.read().edge_count();
    let mut outcome = WriterOutcome::default();

    let max_coalesce = config.max_coalesce.max(1);
    let coalesce_window = config.coalesce_window;
    let bounds = RateBounds::from_config(&config);

    while let Some(batch) = next_batch(&mut update_stream, max_coalesce, coalesce_window).await {
        let mut validated = Vec::with_capacity(batch.len());
        debug!(batch_size = batch.len(), "coalesced batch ready");
        for update in batch {
            match validate_update(update, edge_count) {
                Ok(valid) => validated.push(valid),
                Err(UpdateValidationError::IndexOutOfBounds(index)) => {
                    outcome.invalid_index_updates += 1;
                    warn!(index, "dropped update with out-of-bounds index");
                }
                Err(UpdateValidationError::InvalidRate(rate)) => {
                    outcome.invalid_rate_updates += 1;
                    warn!(rate, "dropped update with invalid rate");
                }
            }
        }

        if validated.is_empty() {
            error!("discarded batch: no valid updates after validation");
            continue;
        }

        outcome.processed_updates += validated.len();

        let bounded_updates: Vec<GraphUpdate> = validated
            .into_iter()
            .map(|update| match update {
                GraphUpdate::Rate {
                    edge_index,
                    new_rate,
                } => GraphUpdate::Rate {
                    edge_index,
                    new_rate: bounds.clamp(new_rate),
                },
            })
            .collect();

        let applied = apply_valid_updates(&shared_edges, &bounded_updates);
        if applied == 0 {
            error!(
                batch_received = bounded_updates.len(),
                "failed to apply validated updates"
            );
            continue;
        }

        outcome.unique_updates_applied += applied;
        info!(
            batch_received = bounded_updates.len(),
            unique_applied = applied,
            total_processed = outcome.processed_updates,
            total_unique_applied = outcome.unique_updates_applied,
            "processed update batch"
        );
    }

    outcome
}

#[instrument(level = "trace", skip_all, fields(batch = updates.len()))]
fn apply_valid_updates(shared_graph: &SharedGraph, updates: &[GraphUpdate]) -> usize {
    if updates.is_empty() {
        return 0;
    }

    let mut graph = shared_graph.write();
    for update in updates {
        match *update {
            GraphUpdate::Rate {
                edge_index,
                new_rate,
            } => {
                graph
                    .update_rate(edge_index, new_rate)
                    .expect("validated update should succeed");
            }
        }
    }
    updates.len()
}

/// Coalescing helper (aka chunk timeout):
/// - Always awaits the first item to respect backpressure.
/// - Then drains up to `max_coalesce - 1` additional items until `coalesce_window` elapses.
/// - Batches reduce lock traffic in the writer at the cost of bounded latency.
async fn next_batch<S>(
    stream: &mut S,
    max_coalesce: usize,
    coalesce_window: Duration,
) -> Option<Vec<GraphUpdate>>
where
    S: Stream<Item = GraphUpdate> + Unpin,
{
    match stream.next().await {
        Some(first) => {
            let mut batch = Vec::with_capacity(max_coalesce);
            batch.push(first);

            if coalesce_window > Duration::ZERO && max_coalesce > 1 {
                let deadline = Instant::now() + coalesce_window;
                while batch.len() < max_coalesce {
                    match timeout_at(deadline, stream.next()).await {
                        Ok(Some(next)) => batch.push(next),
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }

            Some(batch)
        }
        None => None,
    }
}

fn validate_update(
    update: GraphUpdate,
    edge_count: usize,
) -> Result<GraphUpdate, UpdateValidationError> {
    match update {
        GraphUpdate::Rate {
            edge_index,
            new_rate,
        } => {
            if edge_index >= edge_count {
                return Err(UpdateValidationError::IndexOutOfBounds(edge_index));
            }
            if new_rate <= 0.0 || !new_rate.is_finite() {
                return Err(UpdateValidationError::InvalidRate(new_rate));
            }
            Ok(GraphUpdate::Rate {
                edge_index,
                new_rate,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csr_graph::CSRGraph;
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tokio_stream::wrappers::ReceiverStream;

    #[tokio::test]
    async fn writer_tracks_invalid_updates() {
        let shared = Arc::new(RwLock::new(CSRGraph::from_edges(
            2,
            vec![(0usize, 1usize, 1.0)],
        )));
        let (tx, rx) = mpsc::channel(4);

        // invalid index
        tx.send(GraphUpdate::Rate {
            edge_index: 5,
            new_rate: 1.0,
        })
        .await
        .unwrap();
        // invalid rate
        tx.send(GraphUpdate::Rate {
            edge_index: 0,
            new_rate: 0.0,
        })
        .await
        .unwrap();
        drop(tx);

        let outcome = writer_task(
            Arc::clone(&shared),
            ReceiverStream::new(rx),
            PipelineConfig {
                max_coalesce: 4,
                coalesce_window: Duration::from_millis(1),
                ..PipelineConfig::default()
            },
        )
        .await;

        assert_eq!(outcome.processed_updates, 0);
        assert_eq!(outcome.invalid_index_updates, 1);
        assert_eq!(outcome.invalid_rate_updates, 1);
        assert_eq!(outcome.unique_updates_applied, 0);
    }
}
