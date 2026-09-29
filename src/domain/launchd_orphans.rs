//! Launchd orphan classification: pure judgement over inert plist facts.
//!
//! This module is **read-only reporting logic**. `integration::launchd_scan`
//! converts each LaunchAgent/LaunchDaemon plist and checks whether its
//! program path exists; this module receives those facts as inert DTOs and
//! decides which plists are orphans. It performs no filesystem, process, or
//! OS calls, and it never unloads or deletes anything (removal is
//! UNSUPPORTED here; any future removal must go through plan/approve/delete).
//!
//! # Conservatism
//!
//! Only an **absolute** program path can be judged: launchd resolves relative
//! program names against a search rule this module does not model, so a
//! relative name (or a plist with no program key at all) is
//! [`LaunchdVerdict::Unknown`], never an orphan.

use serde::{Deserialize, Serialize};

/// Inert facts about one plist, gathered by the integration layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchdPlistFacts {
    /// `Label` key, if present.
    pub label: Option<String>,
    /// Path of the plist file.
    pub plist_path: String,
    /// `Program`, else `ProgramArguments[0]`, if present.
    pub program: Option<String>,
    /// Whether `program` exists on disk. Only meaningful for absolute paths.
    pub program_exists: bool,
}

/// Why a plist could not be judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnknownReason {
    /// Neither `Program` nor `ProgramArguments[0]` is present (or it is empty).
    NoProgramKey,
    /// The program is not an absolute path; only absolute paths are judged.
    RelativeProgram,
}

/// Verdict for one plist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchdVerdict {
    /// Absolute program path exists.
    Ok,
    /// Absolute program path does not exist.
    Orphan,
    /// Could not be judged, with the reason.
    Unknown(UnknownReason),
}

/// One orphaned plist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchdOrphan {
    /// `Label` key, if present.
    pub label: Option<String>,
    /// Path of the plist file.
    pub plist_path: String,
    /// The missing absolute program path.
    pub program: String,
}

/// Counts over a set of judged plists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LaunchdSummary {
    /// Plists judged.
    pub total: usize,
    /// Program exists.
    pub ok: usize,
    /// Program missing.
    pub orphans: usize,
    /// Not judgeable (no program key or relative program).
    pub unknown: usize,
}

/// Classifies one plist.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::launchd_orphans::{
///     classify, LaunchdPlistFacts, LaunchdVerdict, UnknownReason,
/// };
///
/// let f = |program: Option<&str>, exists: bool| LaunchdPlistFacts {
///     label: Some("com.example.job".into()),
///     plist_path: "/x/com.example.job.plist".into(),
///     program: program.map(String::from),
///     program_exists: exists,
/// };
///
/// // Positive: absolute path that is missing is an orphan.
/// assert_eq!(classify(&f(Some("/gone/bin"), false)), LaunchdVerdict::Orphan);
/// // Negative: absolute path that exists is fine.
/// assert_eq!(classify(&f(Some("/bin/ls"), true)), LaunchdVerdict::Ok);
/// // Refusal: no program key, or a relative name, is UNKNOWN, not orphan.
/// assert_eq!(
///     classify(&f(None, false)),
///     LaunchdVerdict::Unknown(UnknownReason::NoProgramKey)
/// );
/// assert_eq!(
///     classify(&f(Some("ls"), false)),
///     LaunchdVerdict::Unknown(UnknownReason::RelativeProgram)
/// );
/// // A relative name is never an orphan even if `program_exists` is false.
/// assert_eq!(
///     classify(&f(Some("./run.sh"), false)),
///     LaunchdVerdict::Unknown(UnknownReason::RelativeProgram)
/// );
/// ```
pub fn classify(facts: &LaunchdPlistFacts) -> LaunchdVerdict {
    match facts.program.as_deref() {
        None | Some("") => LaunchdVerdict::Unknown(UnknownReason::NoProgramKey),
        Some(p) if !p.starts_with('/') => LaunchdVerdict::Unknown(UnknownReason::RelativeProgram),
        Some(_) if facts.program_exists => LaunchdVerdict::Ok,
        Some(_) => LaunchdVerdict::Orphan,
    }
}

/// Returns the orphaned plists, sorted by plist path.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::launchd_orphans::{find_orphans, LaunchdPlistFacts};
///
/// let mk = |path: &str, program: Option<&str>, exists: bool| LaunchdPlistFacts {
///     label: None,
///     plist_path: path.into(),
///     program: program.map(String::from),
///     program_exists: exists,
/// };
/// let facts = vec![
///     mk("/b.plist", Some("/gone/b"), false),
///     mk("/a.plist", Some("/gone/a"), false),
///     mk("/ok.plist", Some("/bin/ls"), true),
///     mk("/rel.plist", Some("ls"), false),
///     mk("/none.plist", None, false),
/// ];
/// let orphans = find_orphans(&facts);
/// // Positive: both missing absolute programs, sorted by path.
/// let paths: Vec<_> = orphans.iter().map(|o| o.plist_path.as_str()).collect();
/// assert_eq!(paths, ["/a.plist", "/b.plist"]);
/// // Negative / refusal: nothing to report for existing, relative, or absent programs.
/// assert!(find_orphans(&facts[2..]).is_empty());
/// assert!(find_orphans(&[]).is_empty());
/// ```
pub fn find_orphans(facts: &[LaunchdPlistFacts]) -> Vec<LaunchdOrphan> {
    let mut out: Vec<LaunchdOrphan> = facts
        .iter()
        .filter(|f| classify(f) == LaunchdVerdict::Orphan)
        .map(|f| LaunchdOrphan {
            label: f.label.clone(),
            plist_path: f.plist_path.clone(),
            program: f.program.clone().unwrap_or_default(),
        })
        .collect();
    out.sort_by(|a, b| a.plist_path.cmp(&b.plist_path));
    out
}

/// Counts verdicts over `facts`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::launchd_orphans::{summarize, LaunchdPlistFacts};
///
/// let mk = |program: Option<&str>, exists: bool| LaunchdPlistFacts {
///     label: None,
///     plist_path: "/p.plist".into(),
///     program: program.map(String::from),
///     program_exists: exists,
/// };
/// let s = summarize(&[mk(Some("/bin/ls"), true), mk(Some("/gone"), false), mk(None, false)]);
/// assert_eq!((s.total, s.ok, s.orphans, s.unknown), (3, 1, 1, 1));
/// // Refusal: an empty input yields all zeros.
/// assert_eq!(summarize(&[]).total, 0);
/// ```
pub fn summarize(facts: &[LaunchdPlistFacts]) -> LaunchdSummary {
    let mut s = LaunchdSummary { total: facts.len(), ..Default::default() };
    for f in facts {
        match classify(f) {
            LaunchdVerdict::Ok => s.ok += 1,
            LaunchdVerdict::Orphan => s.orphans += 1,
            LaunchdVerdict::Unknown(_) => s.unknown += 1,
        }
    }
    s
}
