//! Deletion plan representation and build logic.

use crate::domain::artifact::Candidate;
use crate::domain::tool_roots::ToolRootReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletionPlan {
    pub version: u32,
    pub created_unix: u64,
    pub roots: Vec<PathBuf>,
    pub deps: bool,
    pub aggressive: bool,
    pub items: Vec<PlanItem>,
    pub tool_roots: Vec<ToolRootReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanItem {
    pub path: PathBuf,
    pub kind: PlanItemKind,
    pub reason: String,
    /// Physical disk allocation of this item in bytes (blocks × 512), so the plan
    /// shows reclaim impact before deletion. `#[serde(default)]` keeps plans
    /// written before this field was added loadable.
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlanItemKind {
    File,
    Dir,
    GithubRepo,
    GithubBranch,
    GithubRun,
    GithubRelease,
}


impl DeletionPlan {
    /// Creates a new deletion plan.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    /// use std::path::PathBuf;
    ///
    /// let items = vec![PlanItem {
    ///     path: PathBuf::from("/Users/user/dev/project/target"),
    ///     kind: PlanItemKind::Dir,
    ///     reason: "rust target".to_string(),
    ///     bytes: 0,
    /// }];
    /// let plan = DeletionPlan::new(
    ///     vec![PathBuf::from("/Users/user")],
    ///     false,
    ///     true,
    ///     items,
    ///     vec![],
    /// );
    /// assert_eq!(plan.version, 1);
    /// assert_eq!(plan.items.len(), 1);
    /// ```
    pub fn new(
        roots: Vec<PathBuf>,
        deps: bool,
        aggressive: bool,
        items: Vec<PlanItem>,
        tool_roots: Vec<ToolRootReport>,
    ) -> Self {
        let created_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            version: 1,
            created_unix,
            roots,
            deps,
            aggressive,
            items,
            tool_roots,
        }
    }
}

/// Extracts a sorted, deduplicated list of directories from a deletion plan
/// that are candidates for Time Machine exclusion (e.g., rebuildable project directories
/// or tool root cache directories).
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind, extract_exclusion_candidates};
/// use osx_clnr::domain::tool_roots::ToolRootReport;
/// use std::path::PathBuf;
///
/// let items = vec![
///     PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/target"),
///         kind: PlanItemKind::Dir,
///         reason: "rust target".to_string(),
///         bytes: 0,
///     },
///     PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/src/main.rs"),
///         kind: PlanItemKind::File,
///         reason: "source file".to_string(),
///         bytes: 0,
///     },
/// ];
/// let tool_roots = vec![
///     ToolRootReport {
///         path: "/Users/user/.npm".to_string(),
///         category: "node_package_cache".to_string(),
///         bytes: 1000,
///         human: "1KB".to_string(),
///         files: 10,
///         dirs: 2,
///         created_unix: None,
///         last_accessed_unix: None,
///         last_modified_unix: None,
///         metadata_changed_unix: None,
///         newest_descendant_modified_unix: None,
///         newest_descendant_path: None,
///         days_since_modified: None,
///         days_since_accessed: None,
///         days_since_newest_descendant_modified: None,
///         recommendation: "cleanup_candidate".to_string(),
///         rationale: "cache".to_string(),
///     }
/// ];
/// let plan = DeletionPlan::new(
///     vec![PathBuf::from("/Users/user")],
///     false,
///     true,
///     items,
///     tool_roots,
/// );
///
/// let candidates = extract_exclusion_candidates(&plan);
/// assert_eq!(candidates.len(), 2);
/// assert_eq!(candidates[0].path, PathBuf::from("/Users/user/.npm"));
/// assert_eq!(candidates[1].path, PathBuf::from("/Users/user/dev/project/target"));
/// ```
pub fn extract_exclusion_candidates(plan: &DeletionPlan) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = plan
        .items
        .iter()
        .filter(|item| matches!(item.kind, PlanItemKind::Dir))
        .map(|item| Candidate {
            path: item.path.clone(),
            reason: item.reason.clone(),
        })
        .collect();

    for tr in &plan.tool_roots {
        if tr.category.ends_with("_cache")
            || tr.category.ends_with("_caches")
            || tr.recommendation == "cleanup_candidate"
        {
            candidates.push(Candidate {
                path: PathBuf::from(&tr.path),
                reason: format!("Tool root cache: {}", tr.category),
            });
        }
    }

    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.path.clone()));

    // Sort path-wise
    candidates.sort_by(|a, b| a.path.cmp(&b.path));

    candidates
}
