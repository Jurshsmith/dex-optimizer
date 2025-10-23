use super::{
    config::{PipelineConfig, RateBounds},
    types::GraphUpdate,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::time::Duration;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{instrument, warn};

pub(super) fn start(
    update_sender: mpsc::Sender<GraphUpdate>,
    baseline_rates: Vec<f64>,
    config: PipelineConfig,
) -> JoinHandle<()> {
    tokio::spawn(producer_task(update_sender, baseline_rates, config))
}

#[instrument(
    name = "pipeline_producer",
    level = "debug",
    skip_all,
    fields(
        max_updates = config.max_updates,
        edge_count = baseline_rates.len(),
        rate_jitter = config.rate_jitter
    )
)]
async fn producer_task(
    update_sender: mpsc::Sender<GraphUpdate>,
    baseline_rates: Vec<f64>,
    config: PipelineConfig,
) {
    let edge_count = baseline_rates.len();
    if edge_count == 0 {
        return;
    }

    let mut rng = StdRng::from_seed(rand::random::<[u8; 32]>());
    let mut remaining = config.max_updates;
    let max_burst = config.max_coalesce.max(1);
    let bounds = RateBounds::from_config(&config);

    while remaining > 0 {
        let burst = rng.random_range(1..=max_burst.min(remaining));

        for _ in 0..burst {
            let edge_index = rng.random_range(0..edge_count);
            let base_rate = baseline_rates[edge_index];
            let jitter = if config.rate_jitter > 0.0 {
                rng.random_range(-config.rate_jitter..config.rate_jitter)
            } else {
                0.0
            };
            let new_rate = bounds.clamp(base_rate * (1.0 + jitter));

            if update_sender
                .send(GraphUpdate::Rate {
                    edge_index,
                    new_rate,
                })
                .await
                .is_err()
            {
                warn!("writer dropped before producer finished sending updates");
                return;
            }
        }

        remaining -= burst;
        if remaining == 0 {
            break;
        }

        let max_delay_ms = (config.search_interval.as_millis().max(1) as u64).saturating_mul(2);
        let sleep_ms = rng.random_range(0..=max_delay_ms);
        if sleep_ms > 0 {
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }
    }
}
