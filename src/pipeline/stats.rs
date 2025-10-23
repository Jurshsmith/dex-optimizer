use crate::cycle_finder::Cycle;

#[derive(Debug, Default, Clone)]
pub struct PipelineStats {
    pub updates_processed: usize,
    pub unique_updates_applied: usize,
    pub searches_run: usize,
    pub last_cycle: Option<Cycle>,
    pub invalid_index_updates: usize,
    pub invalid_rate_updates: usize,
}
