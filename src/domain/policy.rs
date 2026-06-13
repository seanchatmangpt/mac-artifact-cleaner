use serde::{Serialize, Deserialize};
use std::path::Path;
use anyhow::{Result, Context};
use std::fs;
use toml;

#[derive(Serialize, Deserialize, Debug)]
pub struct OclnrPolicy {
    pub safe_to_clean: Vec<String>,
    pub ignore_paths: Vec<String>,
    pub retention_hours: u64,
}

impl OclnrPolicy {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context("Failed to read OCLNR.toml")?;
        let policy: OclnrPolicy = toml::from_str(&content)
            .context("Failed to parse OCLNR.toml")?;
        Ok(policy)
    }
}
impl Default for OclnrPolicy {
    fn default() -> Self {
        Self {
            safe_to_clean: vec![
                "target".to_string(),
                "node_modules".to_string(),
                ".cache".to_string(),
            ],
            ignore_paths: vec![
                "/System".to_string(),
                "/Library".to_string(),
            ],
            retention_hours: 168,
        }
    }
}
