use optimizer::{
    dataset::{self, Dataset},
    pipeline::{self, PipelineConfig},
};
use tracing::info;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    let dataset: Dataset = dataset::load_default_dataset()?;
    let stats = pipeline::run(dataset, PipelineConfig::default()).await?;
    if let Some(ref cycle) = stats.last_cycle {
        info!(
            updates_processed = stats.updates_processed,
            searches_run = stats.searches_run,
            cycle_profit = cycle.profit,
            cycle_neg_log = cycle.neg_log_sum,
            vertices = ?cycle.vertices,
            edge_indexes = ?cycle.edge_indexes,
            "pipeline finished with profitable cycle"
        );
    } else {
        info!(
            updates_processed = stats.updates_processed,
            searches_run = stats.searches_run,
            found_cycle = false,
            "pipeline finished"
        );
    }
    Ok(())
}

fn init_tracing() -> anyhow::Result<()> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("optimizer=info"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_span_events(FmtSpan::ENTER | FmtSpan::EXIT)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
