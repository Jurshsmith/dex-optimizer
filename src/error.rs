use std::{num::TryFromIntError, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatasetError {
    #[error("dataset file {path} could not be opened")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("dataset file {path} could not be parsed")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("dataset contains no edges")]
    EmptyDataset,
    #[error("edge {edge_id} has from index outside usize")]
    FromIndex {
        edge_id: u64,
        #[source]
        source: TryFromIntError,
    },
    #[error("edge {edge_id} has to index outside usize")]
    ToIndex {
        edge_id: u64,
        #[source]
        source: TryFromIntError,
    },
    #[error("edge {edge_id} has invalid rate {rate}")]
    InvalidRate { edge_id: u64, rate: f64 },
    #[error("producer task failed")]
    ProducerJoin(#[source] tokio::task::JoinError),
    #[error("writer task failed")]
    WriterJoin(#[source] tokio::task::JoinError),
    #[error("searcher task failed")]
    SearcherJoin(#[source] tokio::task::JoinError),
}
