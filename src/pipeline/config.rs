use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub hop_cap: usize,
    pub max_updates: usize,
    pub channel_capacity: usize,
    pub search_interval: Duration,
    pub coalesce_window: Duration,
    pub max_coalesce: usize,
    pub rate_jitter: f64,
    pub min_rate_bound: f64,
    pub max_rate_bound: f64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            hop_cap: 6,
            max_updates: 256,
            channel_capacity: 64,
            search_interval: Duration::from_millis(250),
            coalesce_window: Duration::from_millis(5),
            max_coalesce: 16,
            rate_jitter: 0.02,
            min_rate_bound: 1e-9,
            max_rate_bound: 1e9,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RateBounds {
    min: f64,
    max: f64,
}

impl RateBounds {
    pub(super) fn from_config(config: &PipelineConfig) -> Self {
        let min = config.min_rate_bound.max(f64::MIN_POSITIVE);
        let max = config.max_rate_bound.max(min);
        Self { min, max }
    }

    #[inline]
    pub(super) fn clamp(self, rate: f64) -> f64 {
        rate.clamp(self.min, self.max)
    }
}
