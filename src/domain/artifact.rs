//! Artifact and project classification for developer environments.
//!
//! This module classifies files and directories to identify rebuildable
//! developer artifacts and dependency trees safely.
//!
//! **Domain purity rule**: This module contains zero `std::fs` calls.
//! All filesystem evidence arrives as inert DTOs (`EntrySnapshot`, `DirSnapshot`).
//! The integration layer is responsible for reading the filesystem and
//! constructing these snapshots before calling domain functions.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── DTOs ──────────────────────────────────────────────────────────────────────

/// The kind of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
}

/// An inert snapshot of a single filesystem entry.
///
/// Constructed by the integration layer from `std::fs::DirEntry` +
/// `std::fs::Metadata`. Domain functions never receive live OS handles.
#[derive(Debug, Clone)]
pub struct EntrySnapshot {
    /// Absolute path.
    pub path: PathBuf,
    /// The file/directory name component only.
    pub file_name: String,
    /// Extension (lowercase), if any.
    pub extension: Option<String>,
    /// Whether the entry is a file or directory.
    pub kind: EntryKind,
}

impl EntrySnapshot {
    /// Construct an `EntrySnapshot` from its components.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// // Positive case: a Rust source file entry.
    /// let snap = EntrySnapshot::new(
    ///     PathBuf::from("/project/src/main.rs"),
    ///     "main.rs".to_string(),
    ///     Some("rs".to_string()),
    ///     EntryKind::File,
    /// );
    /// assert_eq!(snap.file_name, "main.rs");
    /// assert_eq!(snap.extension.as_deref(), Some("rs"));
    ///
    /// // Negative case: a directory has no extension.
    /// let dir = EntrySnapshot::new(
    ///     PathBuf::from("/project/src"),
    ///     "src".to_string(),
    ///     None,
    ///     EntryKind::Dir,
    /// );
    /// assert_eq!(dir.extension, None);
    /// ```
    pub fn new(
        path: PathBuf,
        file_name: String,
        extension: Option<String>,
        kind: EntryKind,
    ) -> Self {
        Self {
            path,
            file_name,
            extension,
            kind,
        }
    }

    /// Returns `true` if this entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// Returns `true` if this entry is a file.
    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }
}

/// An inert snapshot of the immediate children of a single directory.
///
/// The integration layer reads one directory level and packages the entries
/// into this struct before invoking domain functions.
#[derive(Debug, Clone, Default)]
pub struct DirSnapshot {
    /// All immediate children (files and dirs) of the directory.
    pub children: Vec<EntrySnapshot>,
}

impl DirSnapshot {
    /// Returns `true` if any child is a file with the given name.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{DirSnapshot, EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// let snap = DirSnapshot {
    ///     children: vec![
    ///         EntrySnapshot::new(PathBuf::from("/p/Cargo.toml"), "Cargo.toml".into(), Some("toml".into()), EntryKind::File),
    ///     ],
    /// };
    ///
    /// // Positive case
    /// assert!(snap.has_file("Cargo.toml"));
    ///
    /// // Negative case
    /// assert!(!snap.has_file("package.json"));
    /// ```
    pub fn has_file(&self, name: &str) -> bool {
        self.children
            .iter()
            .any(|e| e.is_file() && e.file_name == name)
    }

    /// Returns `true` if any child is a directory with the given name.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{DirSnapshot, EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// let snap = DirSnapshot {
    ///     children: vec![
    ///         EntrySnapshot::new(PathBuf::from("/p/target"), "target".into(), None, EntryKind::Dir),
    ///     ],
    /// };
    ///
    /// // Positive case
    /// assert!(snap.has_dir("target"));
    ///
    /// // Negative case
    /// assert!(!snap.has_dir("src"));
    /// ```
    pub fn has_dir(&self, name: &str) -> bool {
        self.children
            .iter()
            .any(|e| e.is_dir() && e.file_name == name)
    }

    /// Returns `true` if any child file has the given extension.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{DirSnapshot, EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// let snap = DirSnapshot {
    ///     children: vec![
    ///         EntrySnapshot::new(PathBuf::from("/p/App.csproj"), "App.csproj".into(), Some("csproj".into()), EntryKind::File),
    ///     ],
    /// };
    ///
    /// // Positive case
    /// assert!(snap.has_file_ext("csproj"));
    ///
    /// // Negative case
    /// assert!(!snap.has_file_ext("rs"));
    /// ```
    pub fn has_file_ext(&self, ext: &str) -> bool {
        self.children
            .iter()
            .any(|e| e.is_file() && e.extension.as_deref().map(|x| x == ext).unwrap_or(false))
    }

    /// Returns names of all child directories whose names end with the given suffix.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{DirSnapshot, EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// let snap = DirSnapshot {
    ///     children: vec![
    ///         EntrySnapshot::new(PathBuf::from("/p/mylib.egg-info"), "mylib.egg-info".into(), None, EntryKind::Dir),
    ///         EntrySnapshot::new(PathBuf::from("/p/src"), "src".into(), None, EntryKind::Dir),
    ///     ],
    /// };
    ///
    /// let results: Vec<_> = snap.dirs_with_suffix(".egg-info").collect();
    /// assert_eq!(results.len(), 1);
    /// assert_eq!(results[0].file_name, "mylib.egg-info");
    /// ```
    pub fn dirs_with_suffix<'a>(
        &'a self,
        suffix: &'a str,
    ) -> impl Iterator<Item = &'a EntrySnapshot> {
        self.children
            .iter()
            .filter(move |e| e.is_dir() && e.file_name.ends_with(suffix))
    }

    /// Returns names of all child directories whose names start with the given prefix.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::artifact::{DirSnapshot, EntrySnapshot, EntryKind};
    /// use std::path::PathBuf;
    ///
    /// let snap = DirSnapshot {
    ///     children: vec![
    ///         EntrySnapshot::new(PathBuf::from("/p/cmake-build-debug"), "cmake-build-debug".into(), None, EntryKind::Dir),
    ///         EntrySnapshot::new(PathBuf::from("/p/src"), "src".into(), None, EntryKind::Dir),
    ///     ],
    /// };
    ///
    /// let results: Vec<_> = snap.dirs_with_prefix("cmake-build-").collect();
    /// assert_eq!(results.len(), 1);
    /// assert_eq!(results[0].file_name, "cmake-build-debug");
    /// ```
    pub fn dirs_with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = &'a EntrySnapshot> {
        self.children
            .iter()
            .filter(move |e| e.is_dir() && e.file_name.starts_with(prefix))
    }

    /// Returns names of all child files whose extension matches the given one.
    pub fn files_with_ext<'a>(&'a self, ext: &'a str) -> impl Iterator<Item = &'a EntrySnapshot> {
        self.children.iter().filter(move |e| {
            e.is_file()
                && e.extension
                    .as_ref()
                    .map(|e_ext| e_ext == ext)
                    .unwrap_or(false)
        })
    }
}

// ── Domain types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Candidate {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug)]
pub struct ProjectKind {
    pub names: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct ArgsSnapshot {
    pub deps: bool,
    pub aggressive: bool,
    pub verbose: bool,
    pub tool_roots: bool,
    pub ignore_recent_hours: u64,
}

// ── Classification predicates ──────────────────────────────────────────────────

/// Returns true when a directory path represents a system/macOS directory
/// that must never be traversed or deleted.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::is_macos_os_dir;
/// use std::path::Path;
///
/// // Positive case: macOS/system paths are identified.
/// assert!(is_macos_os_dir(Path::new("/System")));
/// assert!(is_macos_os_dir(Path::new("/usr")));
///
/// // Negative case: typical user project folders are not marked.
/// assert!(!is_macos_os_dir(Path::new("/Users/user/projects")));
/// ```
pub fn is_macos_os_dir(path: &Path) -> bool {
    let s = path.to_string_lossy();

    // Explicitly allow scanning of temporary directories even if they are in /private
    if s == "/tmp" || s == "/private/tmp" || s == "private/tmp" || s.contains(".antigravitycli") {
        return false;
    }

    // Allow everything inside the user's home directory (e.g. /Users/name/Library/...)
    // but block the root-level /Library, /System, etc.
    if s.starts_with("/Users/") && !s.contains("/Library/Application Support/CloudDocs") {
        // We still want to block some very specific user paths that are too noisy or sensitive
        if s.contains("/Library/Application Support/CloudDocs") || s.contains("/Library/Mail") || s.contains("/Library/Messages") {
             return true;
        }
        return false;
    }

    s == "/System"
        || s == "/Library"
        || s == "/Applications"
        || s == "/Volumes"
        || s == "/Network"
        || s == "/private"
        || s == "/usr"
        || s == "/bin"
        || s == "/sbin"
        || s == "/etc"
        || s == "/var"
        || s == "/opt"
        || s == "/Library/Application Support"
        || s == "/Library/Caches"
        || s == "/Library/Developer"
        || s == "/Library/Containers"
        || s == "/Library/Group Containers"
}

/// Returns true when a path represents a global package or tool cache directory.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::is_global_cache;
/// use std::path::Path;
///
/// // Positive case: global cache paths are identified.
/// assert!(is_global_cache(Path::new("/Users/user/.pnpm-store")));
///
/// // Negative case: typical user project folders are not marked.
/// assert!(!is_global_cache(Path::new("/Users/user/dev/project")));
/// ```
pub fn is_global_cache(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let global_caches = [
        "/.pnpm-store",
        "/.rustup",
        "/.asdf",
        "/.pyenv",
        "/.rbenv",
        "/.mix",
        "/.hex",
        "/.osa",
    ];

    global_caches.iter().any(|cache| s.contains(cache))
}

/// Returns true when a directory name should be treated as a rebuildable
/// artifact/dependency leaf during scanning.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::is_artifact_leaf_name;
///
/// // Positive cases
/// assert!(is_artifact_leaf_name("node_modules"));
/// assert!(is_artifact_leaf_name("_build"));
/// assert!(is_artifact_leaf_name(".venv"));
///
/// // Negative case
/// assert!(!is_artifact_leaf_name("src"));
/// ```
pub fn is_artifact_leaf_name(name: &str) -> bool {
    if name.starts_with("target_") {
        return true;
    }
    matches!(
        name,
        "node_modules"
            | "target"
            | ".next"
            | ".nuxt"
            | ".output"
            | ".vercel"
            | ".turbo"
            | ".vite"
            | ".parcel-cache"
            | ".venv"
            | "venv"
            | "env"
            | "_build"
            | "deps"
            | ".elixir_ls"
            | ".gradle"
            | "vendor"
            | ".bundle"
            | "build"
            | "dist"
            | "coverage"
            | "htmlcov"
            | "CMakeFiles"
            | "bin"
            | "obj"
    )
}

/// Returns true if the given directory name represents a traversal barrier.
/// This includes standard barrier names and custom prefix barriers (e.g. `target_`).
pub fn is_traversal_barrier_name(name: &str) -> bool {
    if name.starts_with("target_") {
        return true;
    }
    traversal_barrier_names().contains(name)
}

/// Returns a set of default traversal barrier directory names.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::traversal_barrier_names;
///
/// let barriers = traversal_barrier_names();
///
/// // Positive case
/// assert!(barriers.contains(".git"));
/// assert!(barriers.contains("node_modules"));
///
/// // Negative case
/// assert!(!barriers.contains("src"));
/// ```
pub fn traversal_barrier_names() -> HashSet<&'static str> {
    HashSet::from([
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        ".next",
        ".nuxt",
        ".output",
        ".vercel",
        ".turbo",
        ".vite",
        ".parcel-cache",
        "target",
        ".venv",
        "venv",
        "env",
        ".tox",
        ".nox",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        "__pycache__",
        "_build",
        "deps",
        ".elixir_ls",
        "build",
        "dist",
        "coverage",
        "htmlcov",
        "CMakeFiles",
        "bin",
        "obj",
        "vendor",
        ".bundle",
        ".Trash",
        ".Spotlight-V100",
        ".fseventsd",
        ".DocumentRevisions-V100",
        ".TemporaryItems",
        ".Trashes",
        ".MobileBackups",
    ])
}

// ── Pure classification ────────────────────────────────────────────────────────

/// Detects project kinds based on an inert snapshot of the directory's children.
///
/// This function is pure: it reads no filesystem state and makes no OS calls.
/// The integration layer constructs `DirSnapshot` from `std::fs::read_dir`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::{
///     detect_project_from_snapshot, DirSnapshot, EntrySnapshot, EntryKind
/// };
/// use std::path::PathBuf;
///
/// // Positive case: Cargo.toml marker → rust project.
/// let snap = DirSnapshot {
///     children: vec![
///         EntrySnapshot::new(PathBuf::from("/p/Cargo.toml"), "Cargo.toml".into(), Some("toml".into()), EntryKind::File),
///     ],
/// };
/// let project = detect_project_from_snapshot(&snap).unwrap();
/// assert!(project.names.contains(&"rust"));
///
/// // Negative case: no recognized markers → None.
/// let empty = DirSnapshot::default();
/// assert!(detect_project_from_snapshot(&empty).is_none());
/// ```
pub fn detect_project_from_snapshot(snap: &DirSnapshot) -> Option<ProjectKind> {
    let mut names = Vec::new();

    if snap.has_file("package.json") {
        names.push("node");
    }

    if snap.has_file("next.config.js")
        || snap.has_file("next.config.mjs")
        || snap.has_file("next.config.ts")
    {
        names.push("next");
    }

    if snap.has_file("nuxt.config.js")
        || snap.has_file("nuxt.config.ts")
        || snap.has_file("nuxt.config.mjs")
    {
        names.push("nuxt");
    }

    if snap.has_file("pyproject.toml")
        || snap.has_file("setup.py")
        || snap.has_file("setup.cfg")
        || snap.has_file("requirements.txt")
        || snap.has_file("poetry.lock")
        || snap.has_file("Pipfile")
        || snap.has_file("uv.lock")
    {
        names.push("python");
    }

    if snap.has_file("pom.xml") {
        names.push("maven");
    }

    if snap.has_file("build.gradle")
        || snap.has_file("build.gradle.kts")
        || snap.has_file("settings.gradle")
        || snap.has_file("settings.gradle.kts")
        || snap.has_file("gradlew")
    {
        names.push("gradle");
    }

    if snap.has_file("Cargo.toml") {
        names.push("rust");
    }

    if snap.has_dir(".agents") || snap.has_dir(".gemini") || snap.has_dir(".claude") {
        names.push("ai_project");
    }

    if snap.has_dir("tmp") || snap.has_dir("logs") || snap.has_dir("chats") || snap.has_dir("agents") {
        names.push("ai_project");
    }

    if snap.has_dir("tmp") && (snap.has_file("Cargo.toml") || snap.has_dir(".gemini")) {
        names.push("session_logs");
    }

    if snap.has_file("go.mod") {
        names.push("go");
    }

    if snap.has_file("mix.exs") {
        names.push("elixir");
    }

    if snap.has_file("rebar.config") || snap.has_file("erlang.mk") {
        names.push("erlang");
    }

    if snap.has_file("composer.json") {
        names.push("php");
    }

    if snap.has_file("Gemfile") {
        names.push("ruby");
    }

    if snap.has_file("Package.swift") {
        names.push("swift");
    }

    if snap.has_file("CMakeLists.txt") || snap.has_file("Makefile") {
        names.push("native");
    }

    if snap.has_file_ext("csproj") || snap.has_file_ext("fsproj") || snap.has_file_ext("sln") {
        names.push("dotnet");
    }

    if names.is_empty() {
        None
    } else {
        Some(ProjectKind { names })
    }
}

/// Identifies cleanup candidates for a given project from a directory snapshot.
///
/// This function is pure: it reads no filesystem state. The integration layer
/// provides the `DirSnapshot` from `std::fs::read_dir`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::artifact::{
///     artifact_candidates_from_snapshot, DirSnapshot, EntrySnapshot, EntryKind,
///     ProjectKind, ArgsSnapshot,
/// };
/// use std::path::{Path, PathBuf};
///
/// let root = Path::new("/project");
/// let project = ProjectKind { names: vec!["rust"] };
/// let args = ArgsSnapshot { deps: true, aggressive: true, verbose: false, tool_roots: false, ignore_recent_hours: 1 };
///
/// // Positive case: "target" dir in snapshot → candidate.
/// let snap = DirSnapshot {
///     children: vec![
///         EntrySnapshot::new(PathBuf::from("/project/target"), "target".into(), None, EntryKind::Dir),
///     ],
/// };
/// let candidates = artifact_candidates_from_snapshot(root, &project, &args, &snap);
/// assert!(!candidates.is_empty());
/// assert!(candidates.iter().any(|c| c.path.ends_with("target")));
///
/// // Negative case: empty snapshot → no candidates.
/// let empty = DirSnapshot::default();
/// let none = artifact_candidates_from_snapshot(root, &project, &args, &empty);
/// assert!(none.is_empty());
/// ```
pub fn artifact_candidates_from_snapshot(
    root: &Path,
    project: &ProjectKind,
    args: &ArgsSnapshot,
    snap: &DirSnapshot,
) -> Vec<Candidate> {
    let mut out = Vec::new();

    for name in &project.names {
        match *name {
            "node" => {
                add_dir(&mut out, root, ".turbo", "node turbo cache", snap);
                add_dir(&mut out, root, ".parcel-cache", "node parcel cache", snap);
                add_dir(&mut out, root, ".vite", "node vite cache", snap);
                add_dir(&mut out, root, "coverage", "node coverage", snap);
                add_file(&mut out, root, ".eslintcache", "eslint cache", snap);

                if args.aggressive {
                    add_dir(&mut out, root, "dist", "node dist output", snap);
                    add_dir(&mut out, root, "build", "node build output", snap);
                }

                if args.deps {
                    add_dir(&mut out, root, "node_modules", "node dependencies", snap);
                }
            }

            "next" => {
                add_dir(&mut out, root, ".next", "next build output", snap);
                add_dir(&mut out, root, "out", "next static export", snap);
            }

            "nuxt" => {
                add_dir(&mut out, root, ".nuxt", "nuxt build cache", snap);
                add_dir(&mut out, root, ".output", "nuxt output", snap);
                add_dir(&mut out, root, "dist", "nuxt dist output", snap);
            }

            "python" => {
                add_dir(&mut out, root, ".pytest_cache", "python pytest cache", snap);
                add_dir(&mut out, root, ".mypy_cache", "python mypy cache", snap);
                add_dir(&mut out, root, ".ruff_cache", "python ruff cache", snap);
                add_dir(&mut out, root, ".tox", "python tox", snap);
                add_dir(&mut out, root, ".nox", "python nox", snap);
                add_dir(&mut out, root, "htmlcov", "python coverage html", snap);
                add_file(&mut out, root, ".coverage", "python coverage db", snap);

                if args.aggressive {
                    add_dir(&mut out, root, "build", "python build output", snap);
                    add_dir(&mut out, root, "dist", "python dist output", snap);
                }

                if args.deps {
                    add_dir(&mut out, root, ".venv", "python virtualenv", snap);
                    add_dir(&mut out, root, "venv", "python virtualenv", snap);
                    add_dir(&mut out, root, "env", "python virtualenv", snap);
                }

                for e in snap.dirs_with_suffix(".egg-info") {
                    out.push(Candidate {
                        path: e.path.clone(),
                        reason: "python egg-info".to_string(),
                    });
                }
            }

            "maven" => {
                add_dir(&mut out, root, "target", "maven target", snap);
            }

            "gradle" => {
                add_dir(&mut out, root, "build", "gradle build output", snap);
                add_dir(&mut out, root, ".gradle", "gradle cache", snap);
            }

            "rust" => {
                add_dir(&mut out, root, "target", "rust target", snap);
                for e in snap.dirs_with_prefix("target_") {
                    out.push(Candidate {
                        path: e.path.clone(),
                        reason: format!("rust target ({})", e.file_name),
                    });
                }
            }

            "go" => {
                add_file(&mut out, root, "coverage.out", "go coverage", snap);
                add_file(&mut out, root, "cover.out", "go coverage", snap);

                if args.aggressive {
                    add_dir(&mut out, root, "bin", "go local bin", snap);
                    add_dir(&mut out, root, "dist", "go dist output", snap);
                }
            }

            "elixir" => {
                add_dir(&mut out, root, "_build", "elixir build", snap);
                add_dir(&mut out, root, ".elixir_ls", "elixir ls cache", snap);

                if args.deps {
                    add_dir(&mut out, root, "deps", "elixir dependencies", snap);
                }
            }

            "erlang" => {
                add_dir(&mut out, root, "_build", "erlang build", snap);
                add_dir(&mut out, root, "ebin", "erlang beam output", snap);
                add_file(&mut out, root, "erl_crash.dump", "erlang crash dump", snap);

                if args.deps {
                    add_dir(&mut out, root, "deps", "erlang dependencies", snap);
                }
            }

            "php" => {
                add_dir(&mut out, root, "var/cache", "php cache", snap);
                add_dir(&mut out, root, "cache", "php cache", snap);

                if args.deps {
                    add_dir(&mut out, root, "vendor", "php composer vendor", snap);
                }
            }

            "ruby" => {
                add_dir(&mut out, root, ".bundle", "ruby bundle cache", snap);
                add_dir(&mut out, root, "coverage", "ruby coverage", snap);

                if args.deps {
                    add_dir(&mut out, root, "vendor/bundle", "ruby bundled gems", snap);
                }
            }

            "swift" => {
                add_dir(&mut out, root, ".build", "swift package build", snap);

                if args.aggressive {
                    add_dir(&mut out, root, "build", "swift build output", snap);
                }
            }

            "native" => {
                add_dir(&mut out, root, "CMakeFiles", "cmake files", snap);
                add_file(&mut out, root, "CMakeCache.txt", "cmake cache", snap);

                if args.aggressive {
                    add_dir(&mut out, root, "build", "native build dir", snap);
                    for e in snap.dirs_with_prefix("cmake-build-") {
                        out.push(Candidate {
                            path: e.path.clone(),
                            reason: "cmake build dir".to_string(),
                        });
                    }
                }
            }

            "ai_project" => {
                add_dir(&mut out, root, ".agents", "ai agents dir", snap);
                add_dir(&mut out, root, "agents", "ai agents dir", snap);
                add_dir(&mut out, root, ".gemini/tmp", "gemini temp artifacts", snap);
                add_dir(&mut out, root, ".claude/tmp", "claude temp artifacts", snap);
                add_dir(&mut out, root, "tmp", "ai temp artifacts", snap);
                add_dir(&mut out, root, "logs", "ai tool logs", snap);
                add_dir(&mut out, root, "chats", "ai chat history", snap);
            }

            "session_logs" => {
                if snap.has_dir("tmp") {
                    // This specifically targets massive session files seen in lsp-max and .gemini
                    for e in snap.files_with_ext("jsonl") {
                        out.push(Candidate {
                            path: e.path.clone(),
                            reason: "massive ai session logs".to_string(),
                        });
                    }
                    // Also check for logs in tmp/
                    let tmp_path = root.join("tmp");
                    out.push(Candidate {
                        path: tmp_path,
                        reason: "temporary session logs".to_string(),
                    });
                }
            }

            "dotnet" => {
                add_dir(&mut out, root, "bin", "dotnet bin", snap);
                add_dir(&mut out, root, "obj", "dotnet obj", snap);
            }

            _ => {}
        }
    }

    out
}

// ── Private pure helpers ───────────────────────────────────────────────────────

fn add_dir(out: &mut Vec<Candidate>, root: &Path, rel: &str, reason: &str, snap: &DirSnapshot) {
    // For nested paths like "var/cache" we check the first component against
    // the snapshot and construct the full path without touching the filesystem.
    let first = rel.split('/').next().unwrap_or(rel);
    if snap.has_dir(first) {
        out.push(Candidate {
            path: root.join(rel),
            reason: reason.to_string(),
        });
    }
}

fn add_file(out: &mut Vec<Candidate>, root: &Path, rel: &str, reason: &str, snap: &DirSnapshot) {
    if snap.has_file(rel) {
        out.push(Candidate {
            path: root.join(rel),
            reason: reason.to_string(),
        });
    }
}
