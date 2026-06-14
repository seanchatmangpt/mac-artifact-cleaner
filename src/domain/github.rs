//! GitHub resource representations and parsing/decision rules.
//!
//! This module performs no filesystem mutation or network communication.
//!
//! # Examples
//!
//! ```
//! use osx_clnr::domain::github::GithubTarget;
//! use std::path::Path;
//!
//! let path = Path::new("github://repo/owner/repo");
//! let target = GithubTarget::parse(path).unwrap();
//! assert_eq!(target, GithubTarget::Repo { owner: "owner".into(), repo: "repo".into() });
//! ```

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Duration};

/// Repositories returned from `gh repo list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhRepo {
    pub name: String,
    pub name_with_owner: String,
    pub owner: GhOwner,
    pub description: Option<String>,
    pub is_archived: bool,
    pub is_fork: bool,
    pub is_empty: bool,
    pub pushed_at: Option<String>,
    pub updated_at: String,
    pub created_at: String,
    pub disk_usage: u64,
    pub visibility: String,
}

/// The owner of a GitHub repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhOwner {
    pub login: String,
}

/// Actions Workflow Runs returned from `gh run list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhRun {
    pub database_id: u64,
    pub number: u32,
    pub name: String,
    pub head_branch: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Branch reference returned from `gh api repos/{owner}/{repo}/branches`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhBranchListRef {
    pub name: String,
    pub commit: GhBranchCommitRef,
    pub protected: bool,
}

/// Commit reference within a branch list reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhBranchCommitRef {
    pub sha: String,
    pub url: String,
}

/// Compare response returned from `gh api repos/{owner}/{repo}/compare/{default}...{branch}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhCompareResponse {
    pub status: String,
    pub ahead_by: u32,
    pub behind_by: u32,
    pub total_commits: u32,
}

/// Release representation returned from `gh release list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhRelease {
    pub tag_name: String,
    pub name: String,
    pub is_draft: bool,
    pub is_prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
}

/// A parsed GitHub URI target, referencing a Repository, Branch, Workflow Run, or Release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GithubTarget {
    Repo { owner: String, repo: String },
    Branch { owner: String, repo: String, branch: String },
    Run { owner: String, repo: String, run_id: u64 },
    Release { owner: String, repo: String, tag: String },
}

impl GithubTarget {
    /// Attempts to parse a `github://` URI from a `Path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::github::GithubTarget;
    /// use std::path::Path;
    ///
    /// // Repository target
    /// let target = GithubTarget::parse(Path::new("github://repo/my-owner/my-repo")).unwrap();
    /// assert_eq!(target, GithubTarget::Repo { owner: "my-owner".into(), repo: "my-repo".into() });
    ///
    /// // Branch target containing slashes
    /// let target = GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/issue-1")).unwrap();
    /// assert_eq!(target, GithubTarget::Branch { owner: "my-owner".into(), repo: "my-repo".into(), branch: "feature/issue-1".into() });
    ///
    /// // Workflow Run target
    /// let target = GithubTarget::parse(Path::new("github://run/my-owner/my-repo/12345")).unwrap();
    /// assert_eq!(target, GithubTarget::Run { owner: "my-owner".into(), repo: "my-repo".into(), run_id: 12345 });
    ///
    /// // Release target containing slashes
    /// let target = GithubTarget::parse(Path::new("github://release/my-owner/my-repo/v1.0.0-beta/3")).unwrap();
    /// assert_eq!(target, GithubTarget::Release { owner: "my-owner".into(), repo: "my-repo".into(), tag: "v1.0.0-beta/3".into() });
    ///
    /// // Invalid scheme
    /// assert!(GithubTarget::parse(Path::new("/Users/user/project")).is_none());
    /// ```
    pub fn parse(path: &Path) -> Option<Self> {
        let path_str = path.to_str()?;
        if !path_str.starts_with("github://") {
            return None;
        }
        
        let stripped = path_str.strip_prefix("github://")?;
        let parts: Vec<&str> = stripped.split('/').collect();
        if parts.len() < 3 {
            return None;
        }
        
        let resource_type = parts[0];
        let owner = parts[1].to_string();
        let repo = parts[2].to_string();
        
        match resource_type {
            "repo" => {
                if parts.len() == 3 {
                    Some(GithubTarget::Repo { owner, repo })
                } else {
                    None
                }
            }
            "branch" => {
                let branch = parts[3..].join("/");
                if !branch.is_empty() {
                    Some(GithubTarget::Branch { owner, repo, branch })
                } else {
                    None
                }
            }
            "run" => {
                if parts.len() == 4 {
                    let run_id = parts[3].parse::<u64>().ok()?;
                    Some(GithubTarget::Run { owner, repo, run_id })
                } else {
                    None
                }
            }
            "release" => {
                let tag = parts[3..].join("/");
                if !tag.is_empty() {
                    Some(GithubTarget::Release { owner, repo, tag })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Serializes the target back into a `PathBuf`.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::github::GithubTarget;
    /// use std::path::PathBuf;
    ///
    /// let target = GithubTarget::Repo { owner: "owner".into(), repo: "repo".into() };
    /// assert_eq!(target.to_path_buf(), PathBuf::from("github://repo/owner/repo"));
    /// ```
    pub fn to_path_buf(&self) -> PathBuf {
        let path_str = match self {
            GithubTarget::Repo { owner, repo } => {
                format!("github://repo/{}/{}", owner, repo)
            }
            GithubTarget::Branch { owner, repo, branch } => {
                format!("github://branch/{}/{}/{}", owner, repo, branch)
            }
            GithubTarget::Run { owner, repo, run_id } => {
                format!("github://run/{}/{}/{}", owner, repo, run_id)
            }
            GithubTarget::Release { owner, repo, tag } => {
                format!("github://release/{}/{}/{}", owner, repo, tag)
            }
        };
        PathBuf::from(path_str)
    }
}

/// Helper function to check if a date is older than `threshold_days` compared to `current_time_iso`.
fn is_older_than(date_str: &str, threshold_days: i64, current_time_iso: &str) -> bool {
    let date = match DateTime::parse_from_rfc3339(date_str) {
        Ok(dt) => dt,
        Err(_) => return false,
    };
    let now = match DateTime::parse_from_rfc3339(current_time_iso) {
        Ok(dt) => dt,
        Err(_) => return false,
    };
    let threshold = now - Duration::days(threshold_days);
    date < threshold
}

/// Evaluates if a repository is empty or stale.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhRepo, GhOwner, is_repo_stale_or_empty};
///
/// let empty_repo = GhRepo {
///     name: "empty".into(),
///     name_with_owner: "owner/empty".into(),
///     owner: GhOwner { login: "owner".into() },
///     description: None,
///     is_archived: false,
///     is_fork: false,
///     is_empty: true,
///     pushed_at: None,
///     updated_at: "2026-06-10T12:00:00Z".into(),
///     created_at: "2026-06-10T12:00:00Z".into(),
///     disk_usage: 0,
///     visibility: "PUBLIC".into(),
/// };
///
/// assert!(is_repo_stale_or_empty(&empty_repo, 30, "2026-06-14T12:00:00Z"));
///
/// let active_repo = GhRepo {
///     name: "active".into(),
///     name_with_owner: "owner/active".into(),
///     owner: GhOwner { login: "owner".into() },
///     description: None,
///     is_archived: false,
///     is_fork: false,
///     is_empty: false,
///     pushed_at: Some("2026-06-13T12:00:00Z".into()),
///     updated_at: "2026-06-13T12:00:00Z".into(),
///     created_at: "2026-06-10T12:00:00Z".into(),
///     disk_usage: 100,
///     visibility: "PUBLIC".into(),
/// };
/// assert!(!is_repo_stale_or_empty(&active_repo, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_repo_stale_or_empty(repo: &GhRepo, threshold_days: i64, current_time_iso: &str) -> bool {
    if repo.is_empty {
        return true;
    }
    if repo.is_archived {
        return false;
    }
    
    // Check if the latest activity (push or update) is older than the threshold.
    let push_date = repo.pushed_at.as_deref().unwrap_or(&repo.updated_at);
    is_older_than(push_date, threshold_days, current_time_iso)
        && is_older_than(&repo.updated_at, threshold_days, current_time_iso)
}

/// Evaluates if a workflow run is stale.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhRun, is_run_stale};
///
/// let run = GhRun {
///     database_id: 1,
///     number: 1,
///     name: "test".into(),
///     head_branch: "main".into(),
///     status: "completed".into(),
///     conclusion: Some("success".into()),
///     created_at: "2026-05-10T12:00:00Z".into(),
///     updated_at: "2026-05-10T12:30:00Z".into(),
/// };
///
/// // Completed run older than 30 days
/// assert!(is_run_stale(&run, 30, "2026-06-14T12:00:00Z"));
///
/// // Completed run but not yet older than 30 days
/// assert!(!is_run_stale(&run, 60, "2026-06-14T12:00:00Z"));
///
/// // In-progress run is never stale
/// let mut active_run = run.clone();
/// active_run.status = "in_progress".into();
/// assert!(!is_run_stale(&active_run, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_run_stale(run: &GhRun, threshold_days: i64, current_time_iso: &str) -> bool {
    if run.status != "completed" {
        return false;
    }
    is_older_than(&run.created_at, threshold_days, current_time_iso)
}

/// Evaluates if a branch is fully merged into default branch.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhCompareResponse, is_branch_merged};
///
/// let compare_behind = GhCompareResponse {
///     status: "behind".into(),
///     ahead_by: 0,
///     behind_by: 5,
///     total_commits: 5,
/// };
/// assert!(is_branch_merged(&compare_behind));
///
/// let compare_ahead = GhCompareResponse {
///     status: "ahead".into(),
///     ahead_by: 3,
///     behind_by: 0,
///     total_commits: 3,
/// };
/// assert!(!is_branch_merged(&compare_ahead));
/// ```
pub fn is_branch_merged(compare: &GhCompareResponse) -> bool {
    compare.status == "behind" || compare.status == "identical"
}

/// Evaluates if a release is stale (draft, or prerelease/release older than threshold).
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhRelease, is_release_stale_or_draft};
///
/// let draft = GhRelease {
///     tag_name: "v1.0.0".into(),
///     name: "v1.0.0".into(),
///     is_draft: true,
///     is_prerelease: false,
///     created_at: "2026-06-10T12:00:00Z".into(),
///     published_at: None,
/// };
/// assert!(is_release_stale_or_draft(&draft, 30, "2026-06-14T12:00:00Z"));
///
/// let old_release = GhRelease {
///     tag_name: "v0.9.0".into(),
///     name: "v0.9.0".into(),
///     is_draft: false,
///     is_prerelease: false,
///     created_at: "2026-05-10T12:00:00Z".into(),
///     published_at: Some("2026-05-10T12:00:00Z".into()),
/// };
/// assert!(is_release_stale_or_draft(&old_release, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_release_stale_or_draft(release: &GhRelease, threshold_days: i64, current_time_iso: &str) -> bool {
    if release.is_draft {
        return true;
    }
    is_older_than(&release.created_at, threshold_days, current_time_iso)
}
