use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct OclnrPolicy {
    pub safe_to_clean: Vec<String>,
    pub ignore_paths: Vec<String>,
    pub retention_hours: u64,
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
