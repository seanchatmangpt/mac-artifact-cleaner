//! User-configurable cleanup policy (`osxclnr.toml`): which leaf names are
//! considered safe to clean, which paths are always ignored, and how long
//! deletion evidence should be retained.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use toml;

#[derive(Serialize, Deserialize, Debug)]
pub struct OclnrPolicy {
    pub safe_to_clean: Vec<String>,
    pub ignore_paths: Vec<String>,
    pub retention_hours: u64,
}

impl OclnrPolicy {
    /// Loads a policy from a TOML file on disk.
    ///
    /// ```
    /// use std::io::Write;
    /// use osx_clnr::domain::policy::OclnrPolicy;
    ///
    /// let mut file = tempfile::NamedTempFile::new().unwrap();
    /// writeln!(
    ///     file,
    ///     r#"
    ///     safe_to_clean = ["target"]
    ///     ignore_paths = ["/System"]
    ///     retention_hours = 24
    ///     "#
    /// )
    /// .unwrap();
    ///
    /// let policy = OclnrPolicy::load_from_file(file.path()).unwrap();
    /// assert_eq!(policy.safe_to_clean, vec!["target".to_string()]);
    /// assert_eq!(policy.retention_hours, 24);
    ///
    /// // Refusal: a missing file yields an error, not a panic.
    /// assert!(OclnrPolicy::load_from_file(std::path::Path::new("/nonexistent/osxclnr.toml")).is_err());
    /// ```
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).context("Failed to read osxclnr.toml")?;
        let policy: OclnrPolicy =
            toml::from_str(&content).context("Failed to parse osxclnr.toml")?;
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
            ignore_paths: vec!["/System".to_string(), "/Library".to_string()],
            retention_hours: 168,
        }
    }
}
