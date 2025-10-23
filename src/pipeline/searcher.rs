use super::{
    config::PipelineConfig,
    types::{SearchOutcome, SharedGraph},
};
use crate::cycle_finder::{find_profitable_cycle_with_graph, Cycle};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing::{info, instrument};

pub(super) fn start(
    shared_graph: SharedGraph,
    shutdown: oneshot::Receiver<()>,
    config: PipelineConfig,
) -> JoinHandle<SearchOutcome> {
    tokio::spawn(searcher_task(shared_graph, shutdown, config))
}

#[instrument(
    name = "pipeline_searcher",
    level = "debug",
    skip_all,
    fields(hop_cap = config.hop_cap, search_interval_ms = config.search_interval.as_millis())
)]
async fn searcher_task(
    shared_graph: SharedGraph,
    mut shutdown: oneshot::Receiver<()>,
    config: PipelineConfig,
) -> SearchOutcome {
    let mut interval = time::interval(config.search_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut outcome = SearchOutcome::default();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let shared_graph = {
                    let shared_graph = shared_graph.read();
                    if shared_graph.edge_count() == 0 {
                        None
                    } else {
                        // clone to release read lock
                        Some(shared_graph.clone())
                    }
                };


                if let Some(shared_graph) = shared_graph {
                    if let Some(cycle) = find_profitable_cycle_with_graph(&shared_graph, config.hop_cap) {
                        let Cycle {
                            ref vertices,
                            ref edge_indexes,
                            profit,
                            neg_log_sum,
                        } = cycle;
                        info!(
                            vertices = ?vertices,
                            edge_indexes = ?edge_indexes,
                            profit,
                            neg_log_sum,
                            "profitable cycle detected"
                        );
                        outcome.last_cycle = Some(cycle);
                    }
                    outcome.searches_run += 1;
                }
            }
            _shutdown_request = &mut shutdown => {
                let shared_graph = {
                    let shared_graph = shared_graph.read();
                    if shared_graph.edge_count() == 0 {
                        None
                    } else {
                        // clone to release read lock
                        Some(shared_graph.clone())
                    }
                };

                if let Some(shared_graph) = shared_graph {
                    if let Some(cycle) = find_profitable_cycle_with_graph(&shared_graph, config.hop_cap) {
                        let Cycle {
                            ref vertices,
                            ref edge_indexes,
                            profit,
                            neg_log_sum,
                        } = cycle;
                        info!(
                            vertices = ?vertices,
                            edge_indexes = ?edge_indexes,
                            profit,
                            neg_log_sum,
                            "profitable cycle detected during shutdown check"
                        );
                        outcome.last_cycle = Some(cycle);
                    }
                    outcome.searches_run += 1;
                }
                break;
            }
        }
    }

    outcome
}
