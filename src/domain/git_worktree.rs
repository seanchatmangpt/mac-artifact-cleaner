//! Git worktree standing: pure classification of linked worktrees and
//! `.git` bloat assessment.
//!
//! This module is **read-only reporting logic**. It receives inert facts
//! (DTOs) gathered by `integration::git_health` — which is the only place
//! `git` is ever executed — and returns a classification plus a typed
//! reason. It performs no filesystem, process, or OS calls.
//!
//! # Conservatism
//!
//! Classification is deliberately one-sided: a worktree is reported as
//! reclaimable only when every fact needed to prove it is present and
//! affirmative. Any missing fact (`None`) yields a non-reclaimable
//! [`WorktreeClass::Unknown`] with a typed [`WorktreeReason`] naming what
//! could not be established. Merged status is never assumed.
//!
//! # Removal: UNSUPPORTED
//!
//! Removing a worktree (`git worktree remove` / `git worktree prune`) or
//! running `git gc` is **UNSUPPORTED** here. Any future removal must go
//! through the plan → approve → delete → receipt pipeline so that the
//! destructive step is plan-bound and receipted; this module only produces
//! the evidence such a plan would consume.

use serde::{Deserialize, Serialize};

/// One record from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PorcelainWorktree {
    /// Absolute worktree path as reported by git.
    pub path: String,
    /// `HEAD` commit SHA, if reported.
    pub head: Option<String>,
    /// Short branch name (without `refs/heads/`), if a branch is checked out.
    pub branch: Option<String>,
    /// `detached` line present.
    pub detached: bool,
    /// `bare` line present.
    pub bare: bool,
    /// `locked` line present (with optional reason).
    pub locked: bool,
    /// `prunable` line present (with optional reason).
    pub prunable: bool,
}

/// Parses `git worktree list --porcelain` output into records.
///
/// The first record is always the main worktree (or the bare repo).
///
/// # Examples
///
/// Positive: a main worktree plus a branch worktree and a prunable one.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_worktree_porcelain;
///
/// let text = "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\n\
///             worktree /r-wt\nHEAD bbb\nbranch refs/heads/feat/x\n\n\
///             worktree /gone\nHEAD ccc\ndetached\nprunable gitdir file points to non-existent location\n";
/// let wts = parse_worktree_porcelain(text);
/// assert_eq!(wts.len(), 3);
/// assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
/// assert!(wts[2].prunable && wts[2].detached);
/// ```
///
/// Negative: empty or garbage input yields no records.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_worktree_porcelain;
///
/// assert!(parse_worktree_porcelain("").is_empty());
/// assert!(parse_worktree_porcelain("not porcelain\n").is_empty());
/// ```
///
/// Refusal: attribute lines before any `worktree` line are ignored rather
/// than attached to an invented record.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_worktree_porcelain;
///
/// let wts = parse_worktree_porcelain("HEAD aaa\nlocked\n\nworktree /r\nHEAD bbb\n");
/// assert_eq!(wts.len(), 1);
/// assert!(!wts[0].locked);
/// ```
pub fn parse_worktree_porcelain(text: &str) -> Vec<PorcelainWorktree> {
    let mut out: Vec<PorcelainWorktree> = Vec::new();
    let mut cur: Option<PorcelainWorktree> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(done) = cur.take() {
                out.push(done);
            }
            cur = Some(PorcelainWorktree { path: p.to_string(), ..Default::default() });
            continue;
        }
        if line.is_empty() {
            if let Some(done) = cur.take() {
                out.push(done);
            }
            continue;
        }
        let Some(wt) = cur.as_mut() else { continue };
        if let Some(h) = line.strip_prefix("HEAD ") {
            wt.head = Some(h.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            wt.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if line == "detached" {
            wt.detached = true;
        } else if line == "bare" {
            wt.bare = true;
        } else if line == "locked" || line.starts_with("locked ") {
            wt.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            wt.prunable = true;
        }
    }
    if let Some(done) = cur.take() {
        out.push(done);
    }
    out
}

/// Counts of `git status --porcelain=v1` lines, split by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StatusCounts {
    /// Modified / staged / deleted / renamed / conflicted tracked entries.
    pub tracked_changes: u64,
    /// Untracked, non-ignored entries (`??`).
    pub untracked: u64,
}

/// Parses `git status --porcelain=v1` output. Ignored entries (`!!`) are
/// not counted (they only appear with `--ignored`).
///
/// # Examples
///
/// Positive:
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_status_porcelain;
///
/// let c = parse_status_porcelain(" M a.rs\n?? new.txt\nA  b.rs\n");
/// assert_eq!(c.tracked_changes, 2);
/// assert_eq!(c.untracked, 1);
/// ```
///
/// Negative: clean output.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_status_porcelain;
///
/// assert_eq!(parse_status_porcelain("").tracked_changes, 0);
/// assert_eq!(parse_status_porcelain("").untracked, 0);
/// ```
///
/// Refusal: ignored-file lines never count as dirt, but any other
/// non-empty line conservatively counts as a tracked change.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_status_porcelain;
///
/// let c = parse_status_porcelain("!! target/\nUU conflict.rs\n");
/// assert_eq!(c.untracked, 0);
/// assert_eq!(c.tracked_changes, 1);
/// ```
pub fn parse_status_porcelain(text: &str) -> StatusCounts {
    let mut c = StatusCounts::default();
    for line in text.lines() {
        if line.is_empty() || line.starts_with("!!") {
            continue;
        }
        if line.starts_with("??") {
            c.untracked += 1;
        } else {
            c.tracked_changes += 1;
        }
    }
    c
}

/// Counts stash entries (subjects from `git stash list --format=%gs`) that
/// were created on `branch`. Git writes `WIP on <branch>: ...` for plain
/// stashes and `On <branch>: ...` for stashes with a message.
///
/// # Examples
///
/// Positive:
///
/// ```
/// use osx_clnr::domain::git_worktree::count_stash_refs;
///
/// let subjects = ["WIP on feat: 1234 msg", "On feat: saved", "WIP on main: 99 x"];
/// assert_eq!(count_stash_refs(&subjects, "feat"), 2);
/// ```
///
/// Negative:
///
/// ```
/// use osx_clnr::domain::git_worktree::count_stash_refs;
///
/// assert_eq!(count_stash_refs(&["WIP on main: 1 x"], "feat"), 0);
/// assert_eq!(count_stash_refs::<&str>(&[], "feat"), 0);
/// ```
///
/// Refusal: a branch name that is merely a prefix of the stash's branch
/// does not match.
///
/// ```
/// use osx_clnr::domain::git_worktree::count_stash_refs;
///
/// assert_eq!(count_stash_refs(&["WIP on feature: 1 x"], "feat"), 0);
/// ```
pub fn count_stash_refs<S: AsRef<str>>(subjects: &[S], branch: &str) -> u64 {
    let wip = format!("WIP on {branch}:");
    let on = format!("On {branch}:");
    subjects
        .iter()
        .filter(|s| {
            let s = s.as_ref();
            s.starts_with(&wip) || s.starts_with(&on)
        })
        .count() as u64
}

/// Facts about one worktree, gathered by the integration layer. Every
/// `Option` is `None` when the corresponding git query failed or could not
/// be run — never defaulted to a reassuring value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorktreeFacts {
    /// The first record of `git worktree list` (main worktree / bare repo).
    pub is_main: bool,
    /// Git reported `prunable`.
    pub prunable: bool,
    /// Git reported `locked`.
    pub locked: bool,
    /// The worktree directory exists on disk.
    pub exists: bool,
    /// Checked-out branch (short name); `None` if detached or unknown.
    pub branch: Option<String>,
    /// Git reported `detached`.
    pub detached: bool,
    /// Default branch name of the owning repo, if it could be resolved.
    pub default_branch: Option<String>,
    /// Whether `branch` is an ancestor of the default branch (local or
    /// `origin/`). `None` = could not be determined.
    pub merged_into_default: Option<bool>,
    /// `git status --porcelain` counts; `None` = status failed.
    pub status: Option<StatusCounts>,
    /// Stash entries referencing `branch`; `None` = stash list failed.
    pub stash_refs: Option<u64>,
}

/// Worktree classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorktreeClass {
    /// Git reports it prunable (directory missing); only admin metadata remains.
    Prunable,
    /// Branch fully merged into the default branch, clean, no stash.
    MergedClean,
    /// Uncommitted, untracked, or stashed work present.
    Dirty,
    /// Branch has commits not in the default branch.
    Unmerged,
    /// Detached HEAD — no branch to prove merged.
    Detached,
    /// Locked by `git worktree lock`.
    Locked,
    /// The repository's main worktree — never reclaimable.
    Main,
    /// A required fact could not be established.
    Unknown,
}

impl WorktreeClass {
    /// Only [`WorktreeClass::Prunable`] and [`WorktreeClass::MergedClean`]
    /// are reclaimable.
    pub fn is_reclaimable(self) -> bool {
        matches!(self, WorktreeClass::Prunable | WorktreeClass::MergedClean)
    }
}

/// Typed reason accompanying a [`WorktreeClass`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorktreeReason {
    /// Git reports the worktree prunable.
    GitReportsPrunable,
    /// Merged, clean, and stash-free.
    MergedAndClean { default_branch: String },
    /// Main worktree of the repository.
    MainWorktree,
    /// Locked worktree.
    LockedWorktree,
    /// Directory missing but git does not report it prunable.
    DirectoryMissingNotPrunable,
    /// `git status` could not be run.
    StatusUnavailable,
    /// Tracked changes present.
    TrackedChanges { count: u64 },
    /// Untracked non-ignored files present.
    UntrackedFiles { count: u64 },
    /// Detached HEAD.
    DetachedHead,
    /// Neither a branch nor `detached` was reported.
    BranchUnknown,
    /// Default branch of the repo could not be resolved.
    DefaultBranchUnknown,
    /// The worktree has the default branch itself checked out.
    ChecksOutDefaultBranch,
    /// Merge-base check failed.
    MergeStatusUnknown,
    /// Branch not merged into the default branch.
    NotMerged { default_branch: String },
    /// `git stash list` could not be run.
    StashUnknown,
    /// Stash entries reference the branch.
    StashEntries { count: u64 },
}

/// Classification result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeVerdict {
    pub class: WorktreeClass,
    pub reason: WorktreeReason,
    pub reclaimable: bool,
}

fn verdict(class: WorktreeClass, reason: WorktreeReason) -> WorktreeVerdict {
    WorktreeVerdict { class, reason, reclaimable: class.is_reclaimable() }
}

/// Conservatively classifies one worktree from its facts.
///
/// Order of checks: main → locked → prunable → missing dir → status →
/// detached → branch → default branch → merged → stash. The first failing
/// check determines the (non-reclaimable) verdict.
///
/// # Examples
///
/// Positive: a clean, merged, stash-free branch worktree is reclaimable.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let f = WorktreeFacts {
///     exists: true,
///     branch: Some("feat".into()),
///     default_branch: Some("main".into()),
///     merged_into_default: Some(true),
///     status: Some(StatusCounts::default()),
///     stash_refs: Some(0),
///     ..Default::default()
/// };
/// let v = classify_worktree(&f);
/// assert_eq!(v.class, WorktreeClass::MergedClean);
/// assert!(v.reclaimable);
/// ```
///
/// Negative: unmerged or dirty worktrees are not reclaimable.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let base = WorktreeFacts {
///     exists: true,
///     branch: Some("feat".into()),
///     default_branch: Some("main".into()),
///     merged_into_default: Some(false),
///     status: Some(StatusCounts::default()),
///     stash_refs: Some(0),
///     ..Default::default()
/// };
/// assert_eq!(classify_worktree(&base).class, WorktreeClass::Unmerged);
///
/// let dirty = WorktreeFacts {
///     merged_into_default: Some(true),
///     status: Some(StatusCounts { tracked_changes: 0, untracked: 3 }),
///     ..base
/// };
/// let v = classify_worktree(&dirty);
/// assert_eq!(v.class, WorktreeClass::Dirty);
/// assert_eq!(v.reason, WorktreeReason::UntrackedFiles { count: 3 });
/// assert!(!v.reclaimable);
/// ```
///
/// Refusal: an undeterminable merge status is never assumed merged.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let f = WorktreeFacts {
///     exists: true,
///     branch: Some("feat".into()),
///     default_branch: Some("main".into()),
///     merged_into_default: None,
///     status: Some(StatusCounts::default()),
///     stash_refs: Some(0),
///     ..Default::default()
/// };
/// let v = classify_worktree(&f);
/// assert_eq!(v.class, WorktreeClass::Unknown);
/// assert_eq!(v.reason, WorktreeReason::MergeStatusUnknown);
/// assert!(!v.reclaimable);
///
/// // The main worktree is refused even if every other fact looks clean.
/// let main = WorktreeFacts { is_main: true, merged_into_default: Some(true), ..f };
/// assert_eq!(classify_worktree(&main).class, WorktreeClass::Main);
/// ```
pub fn classify_worktree(f: &WorktreeFacts) -> WorktreeVerdict {
    use WorktreeClass as C;
    use WorktreeReason as R;

    if f.is_main {
        return verdict(C::Main, R::MainWorktree);
    }
    if f.locked {
        return verdict(C::Locked, R::LockedWorktree);
    }
    if f.prunable {
        return verdict(C::Prunable, R::GitReportsPrunable);
    }
    if !f.exists {
        return verdict(C::Unknown, R::DirectoryMissingNotPrunable);
    }
    let Some(status) = f.status else {
        return verdict(C::Unknown, R::StatusUnavailable);
    };
    if status.tracked_changes > 0 {
        return verdict(C::Dirty, R::TrackedChanges { count: status.tracked_changes });
    }
    if status.untracked > 0 {
        return verdict(C::Dirty, R::UntrackedFiles { count: status.untracked });
    }
    if f.detached {
        return verdict(C::Detached, R::DetachedHead);
    }
    let Some(branch) = f.branch.as_deref() else {
        return verdict(C::Unknown, R::BranchUnknown);
    };
    let Some(default_branch) = f.default_branch.as_deref() else {
        return verdict(C::Unknown, R::DefaultBranchUnknown);
    };
    if branch == default_branch {
        return verdict(C::Unknown, R::ChecksOutDefaultBranch);
    }
    match f.merged_into_default {
        None => return verdict(C::Unknown, R::MergeStatusUnknown),
        Some(false) => {
            return verdict(
                C::Unmerged,
                R::NotMerged { default_branch: default_branch.to_string() },
            )
        }
        Some(true) => {}
    }
    match f.stash_refs {
        None => verdict(C::Unknown, R::StashUnknown),
        Some(n) if n > 0 => verdict(C::Dirty, R::StashEntries { count: n }),
        Some(_) => verdict(
            C::MergedClean,
            R::MergedAndClean { default_branch: default_branch.to_string() },
        ),
    }
}

/// Parsed `git count-objects -v` output (sizes converted to bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CountObjects {
    pub loose_count: u64,
    pub loose_bytes: u64,
    pub in_pack: u64,
    pub packs: u64,
    pub pack_bytes: u64,
    pub prune_packable: u64,
    pub garbage: u64,
    pub garbage_bytes: u64,
}

/// Parses `git count-objects -v` (non-human form; sizes are KiB).
///
/// Returns `None` unless the `count:` and `size-pack:` keys are both
/// present and numeric — partial output is refused rather than defaulted.
///
/// # Examples
///
/// Positive:
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_count_objects;
///
/// let t = "count: 10\nsize: 4\nin-pack: 100\npacks: 2\nsize-pack: 2048\n\
///          prune-packable: 1\ngarbage: 0\nsize-garbage: 0\n";
/// let c = parse_count_objects(t).unwrap();
/// assert_eq!(c.loose_count, 10);
/// assert_eq!(c.loose_bytes, 4096);
/// assert_eq!(c.pack_bytes, 2 * 1024 * 1024);
/// assert_eq!(c.prune_packable, 1);
/// ```
///
/// Negative: empty input.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_count_objects;
///
/// assert!(parse_count_objects("").is_none());
/// ```
///
/// Refusal: a non-numeric value for a required key is refused.
///
/// ```
/// use osx_clnr::domain::git_worktree::parse_count_objects;
///
/// assert!(parse_count_objects("count: lots\nsize-pack: 1\n").is_none());
/// ```
pub fn parse_count_objects(text: &str) -> Option<CountObjects> {
    let mut c = CountObjects::default();
    let mut saw_count = false;
    let mut saw_pack = false;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        let parsed = v.parse::<u64>();
        match k.trim() {
            "count" => {
                c.loose_count = parsed.ok()?;
                saw_count = true;
            }
            "size" => c.loose_bytes = parsed.ok()?.saturating_mul(1024),
            "in-pack" => c.in_pack = parsed.ok()?,
            "packs" => c.packs = parsed.ok()?,
            "size-pack" => {
                c.pack_bytes = parsed.ok()?.saturating_mul(1024);
                saw_pack = true;
            }
            "prune-packable" => c.prune_packable = parsed.ok()?,
            "garbage" => c.garbage = parsed.ok()?,
            "size-garbage" => c.garbage_bytes = parsed.ok()?.saturating_mul(1024),
            _ => {}
        }
    }
    (saw_count && saw_pack).then_some(c)
}

/// Git's default `gc.auto` loose-object threshold.
pub const GC_AUTO_LOOSE_THRESHOLD: u64 = 6700;
/// Git's default `gc.autoPackLimit`.
pub const GC_AUTO_PACK_LIMIT: u64 = 50;

/// Why `git gc` would plausibly help.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GcSignal {
    /// `size-garbage > 0` / `garbage > 0`.
    Garbage { files: u64, bytes: u64 },
    /// Loose objects above git's `gc.auto` threshold.
    ManyLooseObjects { count: u64 },
    /// Loose objects already present in packs.
    PrunePackable { count: u64 },
    /// More packs than git's `gc.autoPackLimit`.
    ManyPacks { packs: u64 },
}

/// Returns the signals indicating `git gc` would plausibly help. Empty =
/// no signal (not a guarantee that gc would reclaim nothing).
///
/// # Examples
///
/// Positive:
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let c = CountObjects { loose_count: 10_000, packs: 60, ..Default::default() };
/// let s = gc_signals(&c);
/// assert!(s.contains(&GcSignal::ManyLooseObjects { count: 10_000 }));
/// assert!(s.contains(&GcSignal::ManyPacks { packs: 60 }));
/// ```
///
/// Negative: a tidy repo has no signals.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let c = CountObjects { loose_count: 5, packs: 1, pack_bytes: 1 << 30, ..Default::default() };
/// assert!(gc_signals(&c).is_empty());
/// ```
///
/// Refusal: loose objects exactly at the threshold do not trigger (git's own
/// rule is strictly greater).
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let c = CountObjects { loose_count: GC_AUTO_LOOSE_THRESHOLD, ..Default::default() };
/// assert!(gc_signals(&c).is_empty());
/// ```
pub fn gc_signals(c: &CountObjects) -> Vec<GcSignal> {
    let mut s = Vec::new();
    if c.garbage > 0 || c.garbage_bytes > 0 {
        s.push(GcSignal::Garbage { files: c.garbage, bytes: c.garbage_bytes });
    }
    if c.loose_count > GC_AUTO_LOOSE_THRESHOLD {
        s.push(GcSignal::ManyLooseObjects { count: c.loose_count });
    }
    if c.prune_packable > 0 {
        s.push(GcSignal::PrunePackable { count: c.prune_packable });
    }
    if c.packs > GC_AUTO_PACK_LIMIT {
        s.push(GcSignal::ManyPacks { packs: c.packs });
    }
    s
}

/// One worktree row in the standing report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeStanding {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    /// Unix seconds of the HEAD commit; `None` if unknown.
    pub last_commit_unix: Option<i64>,
    /// RFC 3339 rendering of `last_commit_unix`.
    pub last_commit: Option<String>,
    /// Physical on-disk bytes (0 for a missing directory).
    pub size_bytes: u64,
    pub facts: WorktreeFacts,
    pub verdict: WorktreeVerdict,
}

/// One main repository in the standing report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStanding {
    /// Main worktree path (or bare repo path).
    pub repo: String,
    /// Common git dir (`.git`).
    pub git_dir: String,
    pub git_dir_bytes: u64,
    /// Bytes under `<git_dir>/modules` (submodule object stores) — not
    /// reclaimable by `git gc` of the superproject.
    #[serde(default)]
    pub modules_bytes: u64,
    /// Bytes under `<git_dir>/worktrees` (per-worktree admin dirs, which
    /// can hold per-worktree submodule stores). Measured by an independent
    /// walk from `modules_bytes`/`git_dir_bytes`: hardlinked packs shared
    /// between them are counted in each, so the parts can sum past the whole.
    #[serde(default)]
    pub worktrees_admin_bytes: u64,
    pub default_branch: Option<String>,
    /// `None` if `git count-objects -v` failed.
    pub count_objects: Option<CountObjects>,
    pub gc_signals: Vec<GcSignal>,
    pub worktrees: Vec<WorktreeStanding>,
    /// Non-fatal query failures for this repo.
    pub errors: Vec<String>,
}

/// Whole report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeStandingReport {
    pub version: u32,
    pub generated_unix: i64,
    pub roots: Vec<String>,
    pub repos: Vec<RepoStanding>,
    pub summary: StandingSummary,
    /// Always `"UNSUPPORTED"`: this report never removes anything.
    pub removal: String,
}

/// Aggregate totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StandingSummary {
    pub repos: u64,
    pub linked_worktrees: u64,
    pub reclaimable_worktrees: u64,
    pub reclaimable_bytes: u64,
    pub prunable: u64,
    pub merged_clean: u64,
    pub dirty: u64,
    pub unmerged: u64,
    pub detached: u64,
    pub locked: u64,
    pub unknown: u64,
    pub git_dir_bytes: u64,
    pub repos_with_gc_signal: u64,
}

/// Aggregates a summary over repos. Main worktrees are excluded from the
/// linked-worktree counts.
///
/// # Examples
///
/// Positive:
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let f = WorktreeFacts {
///     exists: true, branch: Some("f".into()), default_branch: Some("main".into()),
///     merged_into_default: Some(true), status: Some(StatusCounts::default()),
///     stash_refs: Some(0), ..Default::default()
/// };
/// let wt = WorktreeStanding {
///     path: "/w".into(), branch: Some("f".into()), head: None,
///     last_commit_unix: None, last_commit: None, size_bytes: 100,
///     verdict: classify_worktree(&f), facts: f,
/// };
/// let repo = RepoStanding {
///     repo: "/r".into(), git_dir: "/r/.git".into(), git_dir_bytes: 7, modules_bytes: 0, worktrees_admin_bytes: 0,
///     default_branch: Some("main".into()), count_objects: None,
///     gc_signals: vec![], worktrees: vec![wt], errors: vec![],
/// };
/// let s = summarize(&[repo]);
/// assert_eq!(s.reclaimable_worktrees, 1);
/// assert_eq!(s.reclaimable_bytes, 100);
/// assert_eq!(s.git_dir_bytes, 7);
/// ```
///
/// Negative: no repos, zero totals.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// assert_eq!(summarize(&[]), StandingSummary::default());
/// ```
///
/// Refusal: a main worktree's bytes are never counted as reclaimable.
///
/// ```
/// use osx_clnr::domain::git_worktree::*;
///
/// let f = WorktreeFacts { is_main: true, exists: true, ..Default::default() };
/// let wt = WorktreeStanding {
///     path: "/r".into(), branch: None, head: None, last_commit_unix: None,
///     last_commit: None, size_bytes: 999, verdict: classify_worktree(&f), facts: f,
/// };
/// let repo = RepoStanding {
///     repo: "/r".into(), git_dir: "/r/.git".into(), git_dir_bytes: 0, modules_bytes: 0, worktrees_admin_bytes: 0,
///     default_branch: None, count_objects: None, gc_signals: vec![],
///     worktrees: vec![wt], errors: vec![],
/// };
/// let s = summarize(&[repo]);
/// assert_eq!(s.linked_worktrees, 0);
/// assert_eq!(s.reclaimable_bytes, 0);
/// ```
pub fn summarize(repos: &[RepoStanding]) -> StandingSummary {
    let mut s = StandingSummary { repos: repos.len() as u64, ..Default::default() };
    for r in repos {
        s.git_dir_bytes = s.git_dir_bytes.saturating_add(r.git_dir_bytes);
        if !r.gc_signals.is_empty() {
            s.repos_with_gc_signal += 1;
        }
        for w in &r.worktrees {
            if w.verdict.class == WorktreeClass::Main {
                continue;
            }
            s.linked_worktrees += 1;
            if w.verdict.reclaimable {
                s.reclaimable_worktrees += 1;
                s.reclaimable_bytes = s.reclaimable_bytes.saturating_add(w.size_bytes);
            }
            match w.verdict.class {
                WorktreeClass::Prunable => s.prunable += 1,
                WorktreeClass::MergedClean => s.merged_clean += 1,
                WorktreeClass::Dirty => s.dirty += 1,
                WorktreeClass::Unmerged => s.unmerged += 1,
                WorktreeClass::Detached => s.detached += 1,
                WorktreeClass::Locked => s.locked += 1,
                WorktreeClass::Unknown => s.unknown += 1,
                WorktreeClass::Main => {}
            }
        }
    }
    s
}
