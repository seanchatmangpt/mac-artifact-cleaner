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

use chrono::{DateTime, Duration};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A branch reference wrapper returned in defaultBranchRef.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhBranchRef {
    pub name: String,
}

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
    pub default_branch_ref: Option<GhBranchRef>,
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
    #[serde(default)]
    pub assets: Vec<GhReleaseAsset>,
}

/// A parsed GitHub URI target, referencing a Repository, Branch, Workflow Run, Release, Cache, Issue, PR, or Release Asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GithubTarget {
    Repo {
        owner: String,
        repo: String,
    },
    Branch {
        owner: String,
        repo: String,
        branch: String,
    },
    Run {
        owner: String,
        repo: String,
        run_id: u64,
    },
    Release {
        owner: String,
        repo: String,
        tag: String,
    },
    Cache {
        owner: String,
        repo: String,
        cache_id: u64,
        key: String,
    },
    Issue {
        owner: String,
        repo: String,
        number: u64,
    },
    Pr {
        owner: String,
        repo: String,
        number: u64,
    },
    ReleaseAsset {
        owner: String,
        repo: String,
        asset_id: u64,
        asset_name: String,
    },
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
    /// // Cache target containing slashes in key
    /// let target = GithubTarget::parse(Path::new("github://cache/my-owner/my-repo/54321/cargo-cache/1")).unwrap();
    /// assert_eq!(target, GithubTarget::Cache { owner: "my-owner".into(), repo: "my-repo".into(), cache_id: 54321, key: "cargo-cache/1".into() });
    ///
    /// // Issue target
    /// let target = GithubTarget::parse(Path::new("github://issue/my-owner/my-repo/42")).unwrap();
    /// assert_eq!(target, GithubTarget::Issue { owner: "my-owner".into(), repo: "my-repo".into(), number: 42 });
    ///
    /// // PR target
    /// let target = GithubTarget::parse(Path::new("github://pr/my-owner/my-repo/101")).unwrap();
    /// assert_eq!(target, GithubTarget::Pr { owner: "my-owner".into(), repo: "my-repo".into(), number: 101 });
    ///
    /// // ReleaseAsset target
    /// let target = GithubTarget::parse(Path::new("github://release-asset/my-owner/my-repo/123/asset-file.zip")).unwrap();
    /// assert_eq!(target, GithubTarget::ReleaseAsset { owner: "my-owner".into(), repo: "my-repo".into(), asset_id: 123, asset_name: "asset-file.zip".into() });
    ///
    /// // Invalid scheme
    /// assert!(GithubTarget::parse(Path::new("/Users/user/project")).is_none());
    ///
    /// // Empty owner or repo
    /// assert!(GithubTarget::parse(Path::new("github://repo//my-repo")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://repo/my-owner/")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://repo//")).is_none());
    ///
    /// // Trailing slashes
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/issue-1/")).is_none());
    ///
    /// // Double slashes
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo//feature")).is_none());
    ///
    /// // Invalid git ref characters/patterns in branch/tag name
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature?")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature.lock")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature@{ref}")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature\\ref")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/..")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/.feature")).is_none());
    /// assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/.ref")).is_none());
    /// ```
    pub fn parse(path: &Path) -> Option<Self> {
        let path_str = path.to_str()?;
        if !path_str.starts_with("github://") {
            return None;
        }

        let stripped = path_str.strip_prefix("github://")?;
        if stripped.ends_with('/') || stripped.contains("//") {
            return None;
        }

        let parts: Vec<&str> = stripped.split('/').collect();
        if parts.len() < 3 {
            return None;
        }

        let resource_type = parts[0];
        let owner = parts[1].to_string();
        let repo = parts[2].to_string();

        if owner.is_empty() || repo.is_empty() {
            return None;
        }

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
                if !branch.is_empty() && is_valid_git_ref_format(&branch) {
                    Some(GithubTarget::Branch {
                        owner,
                        repo,
                        branch,
                    })
                } else {
                    None
                }
            }
            "run" => {
                if parts.len() == 4 {
                    let run_id = parts[3].parse::<u64>().ok()?;
                    Some(GithubTarget::Run {
                        owner,
                        repo,
                        run_id,
                    })
                } else {
                    None
                }
            }
            "release" => {
                let tag = parts[3..].join("/");
                if !tag.is_empty() && is_valid_git_ref_format(&tag) {
                    Some(GithubTarget::Release { owner, repo, tag })
                } else {
                    None
                }
            }
            "cache" => {
                if parts.len() >= 5 {
                    let cache_id = parts[3].parse::<u64>().ok()?;
                    let key = parts[4..].join("/");
                    if !key.is_empty() {
                        Some(GithubTarget::Cache {
                            owner,
                            repo,
                            cache_id,
                            key,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            "issue" => {
                if parts.len() == 4 {
                    let number = parts[3].parse::<u64>().ok()?;
                    Some(GithubTarget::Issue {
                        owner,
                        repo,
                        number,
                    })
                } else {
                    None
                }
            }
            "pr" => {
                if parts.len() == 4 {
                    let number = parts[3].parse::<u64>().ok()?;
                    Some(GithubTarget::Pr {
                        owner,
                        repo,
                        number,
                    })
                } else {
                    None
                }
            }
            "release-asset" => {
                if parts.len() >= 5 {
                    let asset_id = parts[3].parse::<u64>().ok()?;
                    let asset_name = parts[4..].join("/");
                    if !asset_name.is_empty() {
                        Some(GithubTarget::ReleaseAsset {
                            owner,
                            repo,
                            asset_id,
                            asset_name,
                        })
                    } else {
                        None
                    }
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
            GithubTarget::Branch {
                owner,
                repo,
                branch,
            } => {
                format!("github://branch/{}/{}/{}", owner, repo, branch)
            }
            GithubTarget::Run {
                owner,
                repo,
                run_id,
            } => {
                format!("github://run/{}/{}/{}", owner, repo, run_id)
            }
            GithubTarget::Release { owner, repo, tag } => {
                format!("github://release/{}/{}/{}", owner, repo, tag)
            }
            GithubTarget::Cache {
                owner,
                repo,
                cache_id,
                key,
            } => {
                format!("github://cache/{}/{}/{}/{}", owner, repo, cache_id, key)
            }
            GithubTarget::Issue {
                owner,
                repo,
                number,
            } => {
                format!("github://issue/{}/{}/{}", owner, repo, number)
            }
            GithubTarget::Pr {
                owner,
                repo,
                number,
            } => {
                format!("github://pr/{}/{}/{}", owner, repo, number)
            }
            GithubTarget::ReleaseAsset {
                owner,
                repo,
                asset_id,
                asset_name,
            } => {
                format!(
                    "github://release-asset/{}/{}/{}/{}",
                    owner, repo, asset_id, asset_name
                )
            }
        };
        PathBuf::from(path_str)
    }
}

/// Helper function to validate Git ref format.
/// Follows the standard rules defined in `git-check-ref-format`.
fn is_valid_git_ref_format(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.starts_with('/') || s.ends_with('/') {
        return false;
    }
    if s.contains("//") {
        return false;
    }
    if s.contains("..") {
        return false;
    }
    if s.ends_with(".lock") {
        return false;
    }
    if s.contains("@{") {
        return false;
    }
    if s.contains('\\') {
        return false;
    }
    if s == "@" {
        return false;
    }
    for c in s.chars() {
        if c.is_ascii_control() {
            return false;
        }
        match c {
            ' ' | '~' | '^' | ':' | '?' | '*' | '[' => return false,
            _ => {}
        }
    }
    for part in s.split('/') {
        if part.starts_with('.') {
            return false;
        }
    }
    true
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
///     default_branch_ref: None,
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
///     default_branch_ref: None,
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
///     assets: vec![],
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
///     assets: vec![],
/// };
/// assert!(is_release_stale_or_draft(&old_release, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_release_stale_or_draft(
    release: &GhRelease,
    threshold_days: i64,
    current_time_iso: &str,
) -> bool {
    if release.is_draft {
        return true;
    }
    is_older_than(&release.created_at, threshold_days, current_time_iso)
}

/// Cache representation returned from `gh cache list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhCache {
    pub id: u64,
    pub key: String,
    pub size_in_bytes: u64,
    pub created_at: String,
    pub last_accessed_at: String,
}

/// Label representation returned inside issues/PRs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GhLabel {
    pub name: String,
}

/// Issue representation returned from `gh issue list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<GhLabel>,
}

/// Pull Request representation returned from `gh pr list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhPr {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<GhLabel>,
}

/// Release asset representation returned inside `GhRelease`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GhReleaseAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
}

/// Evaluates if a cache is stale.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhCache, is_cache_stale};
///
/// let cache = GhCache {
///     id: 123,
///     key: "my-cache-key".into(),
///     size_in_bytes: 1024,
///     created_at: "2026-05-10T12:00:00Z".into(),
///     last_accessed_at: "2026-05-10T12:00:00Z".into(),
/// };
///
/// // Stale cache older than 30 days
/// assert!(is_cache_stale(&cache, 30, "2026-06-14T12:00:00Z"));
///
/// // Cache not yet older than 30 days
/// assert!(!is_cache_stale(&cache, 60, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_cache_stale(cache: &GhCache, threshold_days: i64, current_time_iso: &str) -> bool {
    is_older_than(&cache.last_accessed_at, threshold_days, current_time_iso)
}

/// Helper to check if any label contains a protection marker.
fn has_protection_label(labels: &[GhLabel]) -> bool {
    labels.iter().any(|l| {
        let name = l.name.to_lowercase();
        name == "pinned" || name == "keep" || name == "no-stale" || name == "critical"
    })
}

/// Evaluates if an issue is stale.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhIssue, GhLabel, is_issue_stale};
///
/// let issue = GhIssue {
///     number: 42,
///     title: "A bug".into(),
///     state: "OPEN".into(),
///     created_at: "2026-05-10T12:00:00Z".into(),
///     updated_at: "2026-05-10T12:00:00Z".into(),
///     labels: vec![],
/// };
///
/// // Open issue older than 30 days
/// assert!(is_issue_stale(&issue, 30, "2026-06-14T12:00:00Z"));
///
/// // Open issue not older than 30 days
/// assert!(!is_issue_stale(&issue, 60, "2026-06-14T12:00:00Z"));
///
/// // Closed issue is not considered stale
/// let mut closed_issue = issue.clone();
/// closed_issue.state = "CLOSED".into();
/// assert!(!is_issue_stale(&closed_issue, 30, "2026-06-14T12:00:00Z"));
///
/// // Open issue older than 30 days but has a protection label ("keep")
/// let mut protected_issue = issue.clone();
/// protected_issue.labels.push(GhLabel { name: "keep".into() });
/// assert!(!is_issue_stale(&protected_issue, 30, "2026-06-14T12:00:00Z"));
///
/// // Open issue older than 30 days but has a protection label ("pinned" case-insensitive)
/// let mut protected_issue_2 = issue.clone();
/// protected_issue_2.labels.push(GhLabel { name: "PinNeD".into() });
/// assert!(!is_issue_stale(&protected_issue_2, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_issue_stale(issue: &GhIssue, threshold_days: i64, current_time_iso: &str) -> bool {
    if issue.state != "OPEN" {
        return false;
    }
    if has_protection_label(&issue.labels) {
        return false;
    }
    is_older_than(&issue.updated_at, threshold_days, current_time_iso)
}

/// Evaluates if a pull request is stale.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::github::{GhPr, GhLabel, is_pr_stale};
///
/// let pr = GhPr {
///     number: 101,
///     title: "A feature".into(),
///     state: "OPEN".into(),
///     created_at: "2026-05-10T12:00:00Z".into(),
///     updated_at: "2026-05-10T12:00:00Z".into(),
///     labels: vec![],
/// };
///
/// // Open PR older than 30 days
/// assert!(is_pr_stale(&pr, 30, "2026-06-14T12:00:00Z"));
///
/// // Open PR not older than 30 days
/// assert!(!is_pr_stale(&pr, 60, "2026-06-14T12:00:00Z"));
///
/// // Merged/closed PR is not considered stale
/// let mut closed_pr = pr.clone();
/// closed_pr.state = "MERGED".into();
/// assert!(!is_pr_stale(&closed_pr, 30, "2026-06-14T12:00:00Z"));
///
/// // Open PR older than 30 days but has a protection label ("critical")
/// let mut protected_pr = pr.clone();
/// protected_pr.labels.push(GhLabel { name: "critical".into() });
/// assert!(!is_pr_stale(&protected_pr, 30, "2026-06-14T12:00:00Z"));
///
/// // Open PR older than 30 days but has a protection label ("no-stale" case-insensitive)
/// let mut protected_pr_2 = pr.clone();
/// protected_pr_2.labels.push(GhLabel { name: "No-Stale".into() });
/// assert!(!is_pr_stale(&protected_pr_2, 30, "2026-06-14T12:00:00Z"));
/// ```
pub fn is_pr_stale(pr: &GhPr, threshold_days: i64, current_time_iso: &str) -> bool {
    if pr.state != "OPEN" {
        return false;
    }
    if has_protection_label(&pr.labels) {
        return false;
    }
    is_older_than(&pr.updated_at, threshold_days, current_time_iso)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_harden_github_target_parse() {
        // Valid targets
        assert!(GithubTarget::parse(Path::new("github://repo/my-owner/my-repo")).is_some());
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/main")).is_some());
        assert!(GithubTarget::parse(Path::new(
            "github://branch/my-owner/my-repo/feature/issue-12"
        ))
        .is_some());
        assert!(
            GithubTarget::parse(Path::new("github://release/my-owner/my-repo/v1.0.0")).is_some()
        );

        // Empty owner or repo
        assert!(GithubTarget::parse(Path::new("github://repo//my-repo")).is_none());
        assert!(GithubTarget::parse(Path::new("github://repo/my-owner/")).is_none());
        assert!(GithubTarget::parse(Path::new("github://repo//")).is_none());
        assert!(GithubTarget::parse(Path::new("github://branch//my-repo/main")).is_none());
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner//main")).is_none());

        // Trailing slash
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/main/")).is_none());
        assert!(GithubTarget::parse(Path::new("github://repo/my-owner/my-repo/")).is_none());

        // Double slash
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo//main")).is_none());
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner//my-repo/main")).is_none());

        // Invalid Git ref formats (e.g. invalid chars, lock extension, consecutive dots, start with dot)
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature?")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature*")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature~")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature^")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature:")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature[abc]"))
                .is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature.lock"))
                .is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/..")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/..feature")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/.feature")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature/.ref"))
                .is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature\\ref"))
                .is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/feature@{ref}"))
                .is_none()
        );
        assert!(GithubTarget::parse(Path::new("github://branch/my-owner/my-repo/@")).is_none());

        // Release targets with invalid ref formats
        assert!(
            GithubTarget::parse(Path::new("github://release/my-owner/my-repo/.v1.0")).is_none()
        );
        assert!(
            GithubTarget::parse(Path::new("github://release/my-owner/my-repo/v1.0.lock")).is_none()
        );
    }
}
