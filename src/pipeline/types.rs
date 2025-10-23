use crate::{csr_graph::CSRGraph, cycle_finder::Cycle};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub(super) enum GraphUpdate {
    Rate { edge_index: usize, new_rate: f64 },
    // TODO: Additional graph mutations (insert/remove edges, fee updates, etc.) can slot in here later.
}

#[derive(Debug, Default)]
pub(super) struct WriterOutcome {
    pub processed_updates: usize,
    pub unique_updates_applied: usize,
    pub invalid_index_updates: usize,
    pub invalid_rate_updates: usize,
}

#[derive(Debug, Default)]
pub(super) struct SearchOutcome {
    pub searches_run: usize,
    pub last_cycle: Option<Cycle>,
}

#[derive(Debug)]
pub(super) enum UpdateValidationError {
    IndexOutOfBounds(usize),
    InvalidRate(f64),
}

pub(super) type SharedGraph = Arc<RwLock<CSRGraph>>;
