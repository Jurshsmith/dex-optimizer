use crate::error::DatasetError;
use serde::Deserialize;
use std::{fs::File, path::Path};

pub const DEFAULT_DATASET_PATH: &str = "datasets/dataset.json";

#[derive(Debug, Deserialize, Clone)]
pub struct Token {
    pub id: u64,
    pub symbol: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Edge {
    pub id: u64,
    pub from: u64,
    pub to: u64,
    pub rate: f64,
    pub pool_id: u64,
    pub kind: u8,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dataset {
    pub tokens: Vec<Token>,
    pub edges: Vec<Edge>,
}

impl Dataset {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, DatasetError> {
        let path_ref = path.as_ref();
        let path_buf = path_ref.to_path_buf();
        let file = File::open(path_ref).map_err(|source| DatasetError::Open {
            path: path_buf.clone(),
            source,
        })?;
        serde_json::from_reader(file).map_err(|source| DatasetError::Deserialize {
            path: path_buf,
            source,
        })
    }
}

pub fn load_default_dataset() -> Result<Dataset, DatasetError> {
    Dataset::load_from_path(DEFAULT_DATASET_PATH)
}
