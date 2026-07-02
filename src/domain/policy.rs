//! User-configurable cleanup policy (`osxclnr.toml`): which leaf names are
//! considered safe to clean, which paths are always ignored, and how long
//! deletion evidence should be retained.
//!
//! **Domain purity**: this module holds only the policy data type and its
//! pure validation/normalization logic. Loading the policy from disk is the
//! integration layer's job (`crate::integration::config`), which admits it
//! through `star-toml`'s `TrustedLoader` (Raw → Validated → Admitted,
//! `q_config = 1`).

use serde::{Deserialize, Serialize};
use star_toml::{loader::ConfigLifecycle, Validate, Validator};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct OclnrPolicy {
    pub safe_to_clean: Vec<String>,
    pub ignore_paths: Vec<String>,
    pub retention_hours: u64,
}

impl OclnrPolicy {
    /// Pure invariant check used by the star-toml admission pipeline.
    ///
    /// A policy is coherent when it names at least one cleanable leaf, keeps
    /// evidence for at least one hour, and every ignore path is absolute
    /// (relative ignore paths would silently fail to match during scans).
    ///
    /// ```
    /// use osx_clnr::domain::policy::OclnrPolicy;
    ///
    /// // Positive: the default policy is coherent.
    /// assert!(OclnrPolicy::default().invariant_violations().is_empty());
    ///
    /// // Negative: zero retention is rejected.
    /// let mut p = OclnrPolicy::default();
    /// p.retention_hours = 0;
    /// assert!(!p.invariant_violations().is_empty());
    ///
    /// // Refusal: a relative ignore path is named in the violation.
    /// let mut p = OclnrPolicy::default();
    /// p.ignore_paths.push("relative/path".to_string());
    /// let v = p.invariant_violations();
    /// assert!(v.iter().any(|m| m.contains("relative/path")));
    /// ```
    pub fn invariant_violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.safe_to_clean.is_empty() {
            out.push("safe_to_clean must name at least one leaf".to_string());
        }
        if self.retention_hours == 0 {
            out.push("retention_hours must be at least 1".to_string());
        }
        for p in &self.ignore_paths {
            if !p.starts_with('/') {
                out.push(format!("ignore path must be absolute: {}", p));
            }
        }
        out
    }
}

impl Validate for OclnrPolicy {
    fn validate(&self, v: &mut Validator) {
        for msg in self.invariant_violations() {
            v.check_predicate("policy", false, "invariant", &msg);
        }
    }
}

impl ConfigLifecycle for OclnrPolicy {
    fn normalize(&mut self) {
        // Drop duplicate leaf names while preserving first-seen order.
        let mut seen = std::collections::BTreeSet::new();
        self.safe_to_clean.retain(|s| seen.insert(s.clone()));
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
