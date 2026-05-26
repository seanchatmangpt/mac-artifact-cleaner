//! Root-level tool and dependency cache tracking.

use crate::domain::time::{seconds_to_days, system_time_to_unix};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRootReport {
    pub path: String,
    pub category: String,
    pub bytes: u64,
    pub human: String,
    pub files: u64,
    pub dirs: u64,
    pub created_unix: Option<i64>,
    pub last_accessed_unix: Option<i64>,
    pub last_modified_unix: Option<i64>,
    pub metadata_changed_unix: Option<i64>,
    pub newest_descendant_modified_unix: Option<i64>,
    pub newest_descendant_path: Option<String>,
    pub days_since_modified: Option<i64>,
    pub days_since_accessed: Option<i64>,
    pub days_since_newest_descendant_modified: Option<i64>,
    pub recommendation: String,
    pub rationale: String,
}

pub struct ToolRootAcc {
    pub bytes: AtomicU64,
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub newest_mtime: AtomicI64,
    pub newest_mtime_path: Mutex<Option<PathBuf>>,
}

impl Default for ToolRootAcc {
    fn default() -> Self {
        Self {
            bytes: AtomicU64::new(0),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            newest_mtime: AtomicI64::new(0),
            newest_mtime_path: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRootDef {
    pub path: PathBuf,
    pub category: &'static str,
    pub default_disposition: &'static str,
}

/// Builds tool root definitions based on home directory if it exists.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::tool_roots::build_tool_root_defs;
///
/// // Positive case: builds definitions based on existing home directories
/// let defs = build_tool_root_defs();
/// // The returned list may contain any of the predefined tool roots depending on the host setup
/// ```
pub fn build_tool_root_defs() -> Vec<ToolRootDef> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    let defs = [
        (".gemini", "ai_tool_state", "review"),
        (".claude", "ai_tool_state", "review"),
        (".codex", "ai_tool_state", "review"),
        (".cursor", "ai_tool_state", "review"),
        (".continue", "ai_tool_state", "review"),
        (".vscode", "editor_state", "review"),
        (".idea", "editor_state", "review"),
        (".cargo", "rust_package_cache", "review"),
        (".rustup", "rust_toolchains", "review"),
        (".npm", "node_package_cache", "cleanup_candidate"),
        (".pnpm-store", "node_package_cache", "cleanup_candidate"),
        (".yarn", "node_package_cache", "cleanup_candidate"),
        (".bun", "js_runtime_cache", "cleanup_candidate"),
        (".deno", "js_runtime_cache", "cleanup_candidate"),
        (".cache/uv", "python_package_cache", "cleanup_candidate"),
        (".cache/pip", "python_package_cache", "cleanup_candidate"),
        (".pyenv", "python_toolchains", "review"),
        (".gradle", "jvm_package_cache", "cleanup_candidate"),
        (".m2", "maven_package_cache", "review"),
        (".mix", "elixir_package_cache", "cleanup_candidate"),
        (".hex", "elixir_package_cache", "cleanup_candidate"),
        ("go", "go_workspace_or_cache", "review"),
        (".android", "android_tool_state", "review"),
        (".pub-cache", "dart_flutter_cache", "cleanup_candidate"),
        (".docker", "container_state", "review"),
        (".minikube", "kubernetes_local_state", "review"),
        (".kube", "kubernetes_config", "keep"),
        (".ollama", "local_model_store", "review"),
        (".cache/huggingface", "model_cache", "review"),
        (".cache", "general_cache", "review"),
        (".local", "local_app_state", "review"),
        (".config", "config_state", "keep"),
        ("Library/Developer", "apple_developer_state", "review"),
        ("Library/Caches", "macos_user_caches", "cleanup_candidate"),
        (
            "Library/Application Support/Docker Desktop",
            "docker_desktop_state",
            "review",
        ),
        (
            "Library/Containers/com.docker.docker",
            "docker_desktop_state",
            "review",
        ),
        (
            "Library/Application Support/MobileSync/Backup",
            "ios_backup",
            "review",
        ),
        (
            "Library/Messages/Attachments",
            "messages_attachments",
            "review",
        ),
    ];

    defs.iter()
        .map(|(rel, category, disposition)| ToolRootDef {
            path: home.join(rel),
            category,
            default_disposition: disposition,
        })
        .filter(|d| d.path.exists())
        .collect()
}

/// Classifies a tool root folder based on size and age metrics.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::tool_roots::{recommend_tool_root, ToolRootDef};
/// use std::path::PathBuf;
///
/// let def = ToolRootDef {
///     path: PathBuf::from("/Users/user/.npm"),
///     category: "node_package_cache",
///     default_disposition: "cleanup_candidate",
/// };
///
/// // Positive case: cache is large and stale
/// let (rec, rat) = recommend_tool_root(&def, 50 * 1024 * 1024 * 1024, Some(100), Some(100), Some(100));
/// assert_eq!(rec, "cleanup_candidate");
///
/// // Negative case: cache is small and not stale
/// let (rec2, rat2) = recommend_tool_root(&def, 1024, Some(0), Some(0), Some(0));
/// assert_eq!(rec2, "low_priority");
/// ```
pub fn recommend_tool_root(
    def: &ToolRootDef,
    bytes: u64,
    days_since_modified: Option<i64>,
    days_since_accessed: Option<i64>,
    days_since_newest_descendant: Option<i64>,
) -> (String, String) {
    let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;

    let stale_by_descendant = days_since_newest_descendant.unwrap_or(0) >= 90;
    let stale_by_modified = days_since_modified.unwrap_or(0) >= 90;
    let very_large = gb >= 10.0;
    let huge = gb >= 50.0;

    let access_note = match days_since_accessed {
        Some(days) => format!("last accessed approximately {} days ago", days),
        None => "last accessed unavailable".to_string(),
    };

    match def.category {
        "kubernetes_config" | "config_state" => (
            "keep".to_string(),
            format!(
                "configuration state; do not delete blindly; {}",
                access_note
            ),
        ),

        "ios_backup" => {
            if huge {
                (
                    "review_high_value".to_string(),
                    format!("large iOS backup store ({:.1} GB); remove through Finder/device backup UI if obsolete", gb),
                )
            } else {
                (
                    "review".to_string(),
                    format!("iOS backup store; {}", access_note),
                )
            }
        }

        "local_model_store" | "model_cache" => {
            if very_large && stale_by_descendant {
                (
                    "review_delete_candidates_inside".to_string(),
                    format!("large model/cache store ({:.1} GB) with no recent descendant writes; remove unused models via tool commands", gb),
                )
            } else {
                (
                    "review".to_string(),
                    format!(
                        "model/cache store ({:.1} GB); inspect model list before deleting",
                        gb
                    ),
                )
            }
        }

        "container_state" | "docker_desktop_state" => {
            if very_large {
                (
                    "review_with_tool".to_string(),
                    format!("large container state ({:.1} GB); prefer docker system df/prune over rm -rf", gb),
                )
            } else {
                (
                    "review".to_string(),
                    format!("container state; {}", access_note),
                )
            }
        }

        "rust_toolchains" | "python_toolchains" => (
            "review_with_tool".to_string(),
            format!(
                "toolchain store; remove old versions using the language manager; {}",
                access_note
            ),
        ),

        "rust_package_cache"
        | "node_package_cache"
        | "python_package_cache"
        | "jvm_package_cache"
        | "maven_package_cache"
        | "elixir_package_cache"
        | "dart_flutter_cache"
        | "general_cache"
        | "macos_user_caches" => {
            if very_large && (stale_by_descendant || stale_by_modified) {
                (
                    "cleanup_candidate".to_string(),
                    format!(
                        "large cache ({:.1} GB) and appears stale; {}",
                        gb, access_note
                    ),
                )
            } else if very_large {
                (
                    "review".to_string(),
                    format!(
                        "large cache ({:.1} GB), but recent activity exists; {}",
                        gb, access_note
                    ),
                )
            } else {
                (
                    "low_priority".to_string(),
                    format!("cache below high-priority threshold; {}", access_note),
                )
            }
        }

        "ai_tool_state" => {
            if huge && stale_by_descendant {
                (
                    "review_archive_or_delete".to_string(),
                    format!("very large AI/tool state ({:.1} GB) with stale worktrees; inspect sessions/worktrees first", gb),
                )
            } else if very_large {
                (
                    "review".to_string(),
                    format!(
                        "large AI/tool state ({:.1} GB); inspect before deleting",
                        gb
                    ),
                )
            } else {
                (
                    "low_priority".to_string(),
                    format!("AI/tool state not huge; {}", access_note),
                )
            }
        }

        _ => (
            def.default_disposition.to_string(),
            format!("default category policy; {}", access_note),
        ),
    }
}

/// Builds the final tool root reports from accumulated statistics.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::tool_roots::{build_tool_root_report, ToolRootDef, ToolRootAcc};
/// use dashmap::DashMap;
/// use std::path::PathBuf;
///
/// let defs = vec![ToolRootDef {
///     path: PathBuf::from("/nonexistent"),
///     category: "test_category",
///     default_disposition: "review",
/// }];
/// let accs = DashMap::new();
/// accs.insert(PathBuf::from("/nonexistent"), ToolRootAcc::default());
///
/// // Negative/refusal case: nonexistent paths are skipped and not reported
/// let reports = build_tool_root_report(&defs, &accs, 0);
/// assert!(reports.is_empty());
/// ```
pub fn build_tool_root_report(
    defs: &[ToolRootDef],
    accs: &DashMap<PathBuf, ToolRootAcc>,
    min_bytes: u64,
) -> Vec<ToolRootReport> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();

    for def in defs {
        let Ok(meta) = std::fs::symlink_metadata(&def.path) else {
            continue;
        };

        let Some(acc) = accs.get(&def.path) else {
            continue;
        };

        let bytes = acc.bytes.load(Ordering::Relaxed);
        if bytes < min_bytes {
            continue;
        }

        let created = meta.created().ok().map(system_time_to_unix);
        let accessed = meta.accessed().ok().map(system_time_to_unix);
        let modified = meta.modified().ok().map(system_time_to_unix);
        let changed = Some(std::os::unix::fs::MetadataExt::ctime(&meta));

        let newest_descendant = acc.newest_mtime.load(Ordering::Relaxed);
        let newest_descendant = if newest_descendant > 0 {
            Some(newest_descendant)
        } else {
            None
        };

        let newest_path = acc
            .newest_mtime_path
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.display().to_string());

        let days_since_modified = modified.map(|t| seconds_to_days(now - t));
        let days_since_accessed = accessed.map(|t| seconds_to_days(now - t));
        let days_since_newest = newest_descendant.map(|t| seconds_to_days(now - t));

        let (recommendation, rationale) = recommend_tool_root(
            def,
            bytes,
            days_since_modified,
            days_since_accessed,
            days_since_newest,
        );

        out.push(ToolRootReport {
            path: def.path.display().to_string(),
            category: def.category.to_string(),
            bytes,
            human: crate::domain::tool_roots::human_bytes(bytes),
            files: acc.files.load(Ordering::Relaxed),
            dirs: acc.dirs.load(Ordering::Relaxed),

            created_unix: created,
            last_accessed_unix: accessed,
            last_modified_unix: modified,
            metadata_changed_unix: changed,

            newest_descendant_modified_unix: newest_descendant,
            newest_descendant_path: newest_path,

            days_since_modified,
            days_since_accessed,
            days_since_newest_descendant_modified: days_since_newest,

            recommendation,
            rationale,
        });
    }

    out.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    out
}

/// Formats a byte count into a human-readable string.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::tool_roots::human_bytes;
///
/// // Positive case
/// assert_eq!(human_bytes(1024), "1.00 KB");
/// assert_eq!(human_bytes(0), "0.00 B");
/// ```
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    format!("{:.2} {}", size, UNITS[unit])
}
