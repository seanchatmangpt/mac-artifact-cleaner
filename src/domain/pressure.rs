//! Pressure-triggered reclaim: pure decision logic.
//!
//! `oclnr monitor --watch --reclaim ...` samples free space in a loop and, when
//! it falls below a threshold, reclaims through the existing receipted paths
//! (`snapshot thin`, and optionally the plan-bound `autoclean` pipeline
//! restricted to regenerable build dirs). Everything in this module is pure:
//! the caller supplies free-space samples, clocks, live process cwds and
//! mtimes gathered by the integration layer, and receives inert decisions.
//!
//! **Domain purity**: zero `std::fs`, `std::process`, or OS calls here.

use std::path::{Path, PathBuf};

use crate::domain::{
    artifact::{
        is_extended_cargo_target_name, is_lane_build_root_name, is_rust_fanout_target_name,
    },
    plan::PlanItem,
};

/// Which reclaim strategies a pressure monitor may use, parsed from
/// `--reclaim snapshots[,builds]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReclaimModes {
    pub snapshots: bool,
    pub builds: bool,
}

/// Parses a `--reclaim` value (`snapshots`, `builds`, `snapshots,builds`).
///
/// ```
/// use osx_clnr::domain::pressure::{parse_reclaim_modes, ReclaimModes};
///
/// // Positive: both strategies, whitespace and order tolerated.
/// assert_eq!(
///     parse_reclaim_modes("builds, snapshots").unwrap(),
///     ReclaimModes { snapshots: true, builds: true }
/// );
/// assert_eq!(
///     parse_reclaim_modes("snapshots").unwrap(),
///     ReclaimModes { snapshots: true, builds: false }
/// );
///
/// // Refusal: unknown strategy names are rejected, never ignored.
/// assert!(parse_reclaim_modes("snapshots,docker").is_err());
///
/// // Negative: an empty list selects nothing and is refused.
/// assert!(parse_reclaim_modes("").is_err());
/// ```
pub fn parse_reclaim_modes(value: &str) -> Result<ReclaimModes, String> {
    let mut modes = ReclaimModes::default();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "snapshots" => modes.snapshots = true,
            "builds" => modes.builds = true,
            other => {
                return Err(format!(
                    "unknown reclaim strategy '{other}' (expected snapshots|builds)"
                ))
            }
        }
    }
    if !modes.snapshots && !modes.builds {
        return Err("--reclaim selects no strategy (expected snapshots and/or builds)".into());
    }
    Ok(modes)
}

/// Outcome of one pressure-monitor tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Free space is at or above the threshold — nothing to do.
    Idle,
    /// Under pressure and outside the cooldown: reclaim `bytes`.
    Thin { bytes: u64 },
    /// Under pressure, but the previous action is younger than the cooldown;
    /// `remaining_secs` until another action is allowed.
    Cooldown { remaining_secs: u64 },
}

/// Pure pressure decision: given the current free space, the threshold, the
/// last time this strategy acted, and the cooldown, decide whether to act.
///
/// The reclaim target is `threshold - free + margin`: enough to get back above
/// the threshold with `margin_bytes` of headroom, so the very next sample does
/// not sit right at the boundary and re-trigger (hysteresis). The cooldown is
/// the second anti-thrash guard: a free-space dip that the last action could
/// not fix (e.g. a build wave still writing) cannot re-fire every interval.
///
/// A clock that went backwards (`now < last`) is treated as still in cooldown
/// for the full window — the conservative reading.
///
/// ```
/// use osx_clnr::domain::pressure::{decide_reclaim, Decision};
///
/// const GIB: u64 = 1 << 30;
/// let now = 1_000_000;
///
/// // Positive: 3.8 GiB free under a 20 GiB threshold, never acted before ->
/// // thin (20 - 3.8) GiB + 5 GiB margin.
/// let free = 3 * GIB + 8 * GIB / 10;
/// assert_eq!(
///     decide_reclaim(free, 20 * GIB, None, now, 600, 5 * GIB),
///     Decision::Thin { bytes: 20 * GIB - free + 5 * GIB }
/// );
///
/// // Negative: free space at/above the threshold is Idle, even with no history.
/// assert_eq!(decide_reclaim(20 * GIB, 20 * GIB, None, now, 600, 5 * GIB), Decision::Idle);
/// assert_eq!(decide_reclaim(65 * GIB, 20 * GIB, Some(now), now, 600, 0), Decision::Idle);
///
/// // Refusal: under pressure but acted 100 s ago with a 600 s cooldown.
/// assert_eq!(
///     decide_reclaim(GIB, 20 * GIB, Some(now - 100), now, 600, 0),
///     Decision::Cooldown { remaining_secs: 500 }
/// );
///
/// // Positive: exactly the cooldown age acts again (`>=`).
/// assert!(matches!(
///     decide_reclaim(GIB, 20 * GIB, Some(now - 600), now, 600, 0),
///     Decision::Thin { .. }
/// ));
///
/// // Refusal: a clock that ran backwards holds the full cooldown.
/// assert_eq!(
///     decide_reclaim(GIB, 20 * GIB, Some(now + 50), now, 600, 0),
///     Decision::Cooldown { remaining_secs: 600 }
/// );
/// ```
pub fn decide_reclaim(
    free_bytes: u64,
    threshold_bytes: u64,
    last_action_unix: Option<i64>,
    now_unix: i64,
    cooldown_secs: u64,
    margin_bytes: u64,
) -> Decision {
    if free_bytes >= threshold_bytes {
        return Decision::Idle;
    }
    if let Some(last) = last_action_unix {
        let cooldown = cooldown_secs.min(i64::MAX as u64) as i64;
        if now_unix < last {
            return Decision::Cooldown { remaining_secs: cooldown_secs };
        }
        let elapsed = now_unix - last;
        if elapsed < cooldown {
            return Decision::Cooldown { remaining_secs: (cooldown - elapsed) as u64 };
        }
    }
    let deficit = threshold_bytes - free_bytes;
    Decision::Thin { bytes: deficit.saturating_add(margin_bytes) }
}

/// Parses `lsof -a -d cwd -Fn` output into the set of live process cwds.
///
/// Field-output mode emits one field per line: `p<pid>`, `f<fd>`, and
/// `n<name>` — the name of the cwd. Only absolute `n` paths are kept;
/// duplicates are removed and the result is sorted.
///
/// ```
/// use osx_clnr::domain::pressure::parse_lsof_cwds;
/// use std::path::PathBuf;
///
/// let out = "p101\nfcwd\nn/h/me/dev/app\np102\nfcwd\nn/h/me/dev/app\np103\nfcwd\nn/\n";
///
/// // Positive: two distinct cwds, deduplicated and sorted.
/// assert_eq!(
///     parse_lsof_cwds(out),
///     vec![PathBuf::from("/"), PathBuf::from("/h/me/dev/app")]
/// );
///
/// // Negative: no `n` lines -> no cwds.
/// assert!(parse_lsof_cwds("p1\nfcwd\n").is_empty());
///
/// // Refusal: non-absolute names (lsof error text) are never admitted.
/// assert!(parse_lsof_cwds("p1\nfcwd\nnlsof: WARNING: can't stat\n").is_empty());
/// ```
pub fn parse_lsof_cwds(stdout: &str) -> Vec<PathBuf> {
    let mut cwds: Vec<PathBuf> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter(|p| p.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    cwds.sort();
    cwds.dedup();
    cwds
}

/// Drops cwds too broad to carry "this process is working in that project"
/// signal: `/` and any ancestor of (or equal to) `home`. Every launchd agent
/// sits at `/` and every idle login shell at `$HOME`; treating those as live
/// work would make every build dir look busy and the builds reclaim a no-op.
///
/// ```
/// use osx_clnr::domain::pressure::significant_cwds;
/// use std::path::{Path, PathBuf};
///
/// let cwds = vec![
///     PathBuf::from("/"),
///     PathBuf::from("/h"),
///     PathBuf::from("/h/me"),
///     PathBuf::from("/h/me/dev/app"),
/// ];
///
/// // Positive: only the project cwd survives.
/// assert_eq!(significant_cwds(cwds, Path::new("/h/me")), vec![PathBuf::from("/h/me/dev/app")]);
///
/// // Negative: nothing but broad cwds -> empty.
/// assert!(significant_cwds(vec![PathBuf::from("/")], Path::new("/h/me")).is_empty());
///
/// // Refusal: a sibling of home is not an ancestor of it and is kept.
/// assert_eq!(
///     significant_cwds(vec![PathBuf::from("/tmp/build")], Path::new("/h/me")),
///     vec![PathBuf::from("/tmp/build")]
/// );
/// ```
pub fn significant_cwds(cwds: Vec<PathBuf>, home: &Path) -> Vec<PathBuf> {
    cwds.into_iter().filter(|c| c.as_path() != Path::new("/") && !home.starts_with(c)).collect()
}

/// True when `candidate` is an ancestor of, equal to, or inside any live cwd.
fn touches_live_cwd(candidate: &Path, cwds: &[PathBuf]) -> bool {
    cwds.iter().any(|cwd| cwd.starts_with(candidate) || candidate.starts_with(cwd))
}

/// Splits plan candidates into `(kept, excluded)`, excluding any candidate
/// that is an ancestor of, equal to, or inside a live process cwd — a running
/// `cargo build` in `~/dev/app` has cwd `~/dev/app`, so `~/dev/app/target` is
/// excluded; a process sitting in `~/dev/app/target/debug` excludes it too.
///
/// Path comparison is component-wise (`Path::starts_with`), so `/a/app2` is
/// never treated as inside `/a/app`.
///
/// ```
/// use osx_clnr::domain::pressure::exclude_live_cwds;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: "rust target".into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
/// let items = vec![item("/d/app/target"), item("/d/app2/target"), item("/d/web/node_modules")];
///
/// // Positive: a build running in /d/app excludes only /d/app/target
/// // (candidate inside cwd); /d/app2 is a different component.
/// let (kept, excluded) = exclude_live_cwds(items.clone(), &[PathBuf::from("/d/app")]);
/// assert_eq!(excluded.len(), 1);
/// assert_eq!(excluded[0].path, PathBuf::from("/d/app/target"));
/// assert_eq!(kept.len(), 2);
///
/// // Positive: a cwd inside a candidate (candidate is its ancestor) excludes it.
/// let (_, excluded) =
///     exclude_live_cwds(items.clone(), &[PathBuf::from("/d/web/node_modules/.bin")]);
/// assert_eq!(excluded[0].path, PathBuf::from("/d/web/node_modules"));
///
/// // Negative: no live cwds -> nothing excluded.
/// let (kept, excluded) = exclude_live_cwds(items.clone(), &[]);
/// assert_eq!((kept.len(), excluded.len()), (3, 0));
///
/// // Refusal: a cwd at the common parent refuses every candidate beneath it.
/// let (kept, _) = exclude_live_cwds(items, &[PathBuf::from("/d")]);
/// assert!(kept.is_empty());
/// ```
pub fn exclude_live_cwds(
    candidates: Vec<PlanItem>,
    cwds: &[PathBuf],
) -> (Vec<PlanItem>, Vec<PlanItem>) {
    candidates.into_iter().partition(|c| !touches_live_cwd(&c.path, cwds))
}

/// True for plan items that are regenerable build output the pressure path
/// may reclaim: rust `target`, elixir `_build`/`deps`, `node_modules`, and
/// python `.venv`. Both the detector reason and the leaf name must agree, so a
/// mislabeled item (or a same-named dir the detector did not produce) is
/// never admitted.
///
/// ```
/// use osx_clnr::domain::pressure::is_regenerable_build_dir;
/// use std::path::Path;
///
/// // Positive: each admitted kind.
/// assert!(is_regenerable_build_dir(Path::new("/d/app/target"), "rust target"));
/// assert!(is_regenerable_build_dir(Path::new("/d/app/target_wasm"), "rust target (target_wasm)"));
/// assert!(is_regenerable_build_dir(Path::new("/d/ex/_build"), "elixir build"));
/// assert!(is_regenerable_build_dir(Path::new("/d/ex/deps"), "elixir dependencies"));
/// assert!(is_regenerable_build_dir(Path::new("/d/web/node_modules"), "node dependencies"));
/// assert!(is_regenerable_build_dir(Path::new("/d/py/.venv"), "python virtualenv"));
///
/// // Negative: other cleanup kinds are outside the pressure path.
/// assert!(!is_regenerable_build_dir(Path::new("/h/Library/Caches"), "global cache"));
/// assert!(!is_regenerable_build_dir(Path::new("/d/py/venv"), "python virtualenv"));
///
/// // Refusal: reason and leaf name must agree.
/// assert!(!is_regenerable_build_dir(Path::new("/h/.npm/_cacache"), "node dependencies"));
/// assert!(!is_regenerable_build_dir(Path::new("/d/app/target"), "elixir build"));
///
/// // Positive: Cargo fan-out leases, including names that are not `target-lane*`.
/// assert!(is_regenerable_build_dir(
///     Path::new("/d/app/target-m10"),
///     "rust lane target root (fan-out lease)"
/// ));
/// assert!(is_regenerable_build_dir(
///     Path::new("/d/app/doctor-target"),
///     "rust lane target root (fan-out lease)"
/// ));
/// assert!(is_regenerable_build_dir(
///     Path::new("/d/app/_t-fix"),
///     "rust lane target root (fan-out lease)"
/// ));
///
/// // Refusal: the lease reason does not make an arbitrary directory regenerable.
/// assert!(!is_regenerable_build_dir(
///     Path::new("/d/app/src"),
///     "rust lane target root (fan-out lease)"
/// ));
/// ```
pub fn is_regenerable_build_dir(path: &Path, reason: &str) -> bool {
    let Some(leaf) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    match reason {
        r if r == "rust target" || r.starts_with("rust target (") => {
            leaf == "target" || leaf.starts_with("target_")
        }
        "elixir build" => leaf == "_build",
        "elixir dependencies" => leaf == "deps",
        "node dependencies" => leaf == "node_modules",
        "python virtualenv" => leaf == ".venv",
        "rust lane target root (fan-out lease)"
        | "elixir lane build root (fan-out lease)"
        | "erlang lane build root (fan-out lease)"
        | "lane build root (fan-out lease)" => is_fanout_lease_leaf(leaf),
        _ => false,
    }
}

fn is_fanout_lease_leaf(leaf: &str) -> bool {
    is_lane_build_root_name(leaf)
        || is_rust_fanout_target_name(leaf)
        || is_extended_cargo_target_name(leaf)
}

/// A plan item the fan-out lease rule may reclaim once it is stale and no
/// process is working inside it. This is not a plain `target/` directory:
/// those are deleted even while a build is running.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::pressure::is_fanout_lease_item;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, r: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
///
/// assert!(is_fanout_lease_item(&item("/d/app/target-m10", "rust lane target root (fan-out lease)")));
/// assert!(is_fanout_lease_item(&item("/d/app/_t-fix", "rust lane target root (fan-out lease)")));
/// assert!(!is_fanout_lease_item(&item("/d/app/target", "rust target")));
/// assert!(!is_fanout_lease_item(&item("/d/app/src", "rust lane target root (fan-out lease)")));
/// ```
pub fn is_fanout_lease_item(item: &PlanItem) -> bool {
    is_regenerable_build_dir(&item.path, &item.reason)
        && matches!(
            item.reason.as_str(),
            "rust lane target root (fan-out lease)"
                | "elixir lane build root (fan-out lease)"
                | "erlang lane build root (fan-out lease)"
                | "lane build root (fan-out lease)"
        )
}

/// Splits items into `(kept, dropped)` by [`is_regenerable_build_dir`].
///
/// ```
/// use osx_clnr::domain::pressure::retain_regenerable_build_dirs;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, r: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
/// let (kept, dropped) = retain_regenerable_build_dirs(vec![
///     item("/d/app/target", "rust target"),
///     item("/h/Library/Caches/x", "global cache"),
/// ]);
/// // Positive + negative in one split.
/// assert_eq!(kept.len(), 1);
/// assert_eq!(dropped.len(), 1);
/// // Refusal: an empty plan stays empty.
/// assert!(retain_regenerable_build_dirs(vec![]).0.is_empty());
/// ```
pub fn retain_regenerable_build_dirs(items: Vec<PlanItem>) -> (Vec<PlanItem>, Vec<PlanItem>) {
    items.into_iter().partition(|i| is_regenerable_build_dir(&i.path, &i.reason))
}

/// True when the plan item is a detector-confirmed Rust build directory
/// (`target` or `target_*` with a `rust target` reason). Both signals must
/// agree, same as [`is_regenerable_build_dir`].
///
/// ```
/// use osx_clnr::domain::pressure::is_rust_target_item;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, r: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
///
/// // Positive: cargo output, including named target profiles.
/// assert!(is_rust_target_item(&item("/d/app/target", "rust target")));
/// assert!(is_rust_target_item(&item("/d/app/target_wasm", "rust target (target_wasm)")));
///
/// // Negative: another regenerable build dir is not a rust target.
/// assert!(!is_rust_target_item(&item("/d/web/node_modules", "node dependencies")));
///
/// // Refusal: the leaf and the reason must agree.
/// assert!(!is_rust_target_item(&item("/d/app/target", "elixir build")));
///
/// // Refusal: a fan-out lease is not the always-reclaim `target/` class.
/// // A live compile inside it, or a fresh directory mtime, must still hold.
/// assert!(!is_rust_target_item(&item(
///     "/d/app/target-m10",
///     "rust lane target root (fan-out lease)"
/// )));
/// ```
pub fn is_rust_target_item(item: &PlanItem) -> bool {
    let reason = item.reason.as_str();
    (reason == "rust target" || reason.starts_with("rust target ("))
        && is_regenerable_build_dir(&item.path, reason)
}

/// Puts Rust `target` directories back into the kept set.
///
/// Disk pressure may delete any of them. A live `cargo` cwd or a fresh
/// mtime is not a reason to leave the directory: the build can be rerun.
/// Other excluded items stay excluded.
///
/// ```
/// use osx_clnr::domain::pressure::{exclude_live_cwds, reclaim_rust_targets};
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, r: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
/// let items = vec![
///     item("/d/app/target", "rust target"),
///     item("/d/web/node_modules", "node dependencies"),
/// ];
/// let (kept, excluded) = exclude_live_cwds(
///     items,
///     &[PathBuf::from("/d/app"), PathBuf::from("/d/web")],
/// );
/// let (kept, excluded) = reclaim_rust_targets(kept, excluded);
///
/// // Positive: the live project's target dir comes back.
/// assert_eq!(kept.len(), 1);
/// assert_eq!(kept[0].path, PathBuf::from("/d/app/target"));
/// // Negative: node_modules under a live cwd stays excluded.
/// assert_eq!(excluded.len(), 1);
/// assert_eq!(excluded[0].path, PathBuf::from("/d/web/node_modules"));
///
/// // Refusal: nothing excluded stays nothing reclaimed.
/// let (kept, excluded) = reclaim_rust_targets(vec![item("/d/app/target", "rust target")], vec![]);
/// assert_eq!(kept.len(), 1);
/// assert!(excluded.is_empty());
/// ```
pub fn reclaim_rust_targets(
    mut kept: Vec<PlanItem>,
    excluded: Vec<PlanItem>,
) -> (Vec<PlanItem>, Vec<PlanItem>) {
    let (targets, still_excluded) = excluded.into_iter().partition(is_rust_target_item);
    kept.extend(targets);
    (kept, still_excluded)
}

/// Puts idle fan-out lease directories back after a project-wide live-cwd
/// exclusion.
///
/// A shell sitting in `~/graphlaw` must not protect `~/graphlaw/target-m10`.
/// A process whose cwd is that directory, or inside it, still protects it.
/// Recency is a separate gate and is not undone here.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::pressure::restore_unbusy_fanout_leases;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, r: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
/// let excluded = vec![
///     item("/d/app/target-m10", "rust lane target root (fan-out lease)"),
///     item("/d/app/target-s1", "rust lane target root (fan-out lease)"),
///     item("/d/web/node_modules", "node dependencies"),
/// ];
/// let (kept, still) = restore_unbusy_fanout_leases(
///     vec![],
///     excluded,
///     &[PathBuf::from("/d/app"), PathBuf::from("/d/app/target-s1/debug")],
/// );
///
/// // Positive: the project cwd does not keep the idle lease excluded.
/// assert_eq!(kept.len(), 1);
/// assert_eq!(kept[0].path, PathBuf::from("/d/app/target-m10"));
/// // Positive: a compile inside target-s1 keeps that lease excluded.
/// assert!(still.iter().any(|i| i.path == PathBuf::from("/d/app/target-s1")));
/// // Negative: node_modules is not a lease and stays excluded.
/// assert!(still.iter().any(|i| i.path == PathBuf::from("/d/web/node_modules")));
///
/// // Refusal: an empty exclusion stays empty.
/// let (kept, still) = restore_unbusy_fanout_leases(vec![], vec![], &[]);
/// assert!(kept.is_empty() && still.is_empty());
/// ```
pub fn restore_unbusy_fanout_leases(
    mut kept: Vec<PlanItem>,
    excluded: Vec<PlanItem>,
    cwds: &[PathBuf],
) -> (Vec<PlanItem>, Vec<PlanItem>) {
    let (still_excluded, restore): (Vec<_>, Vec<_>) = excluded.into_iter().partition(|item| {
        !is_fanout_lease_item(item) || cwds.iter().any(|cwd| cwd.starts_with(&item.path))
    });
    kept.extend(restore);
    (kept, still_excluded)
}

/// Splits a plan for an unattended run.
///
/// The admitted set is what the run may delete. Rust `target` directories
/// are always admitted. Every other `Reversible` or `Compensatable` item is
/// admitted. `Unknown` and `Irreversible` items are deferred: they stay on
/// disk, and they do not cancel the admitted set.
///
/// ```
/// use osx_clnr::domain::pressure::split_unattended_items;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str, kind: PlanItemKind, r: &str, rev: Reversibility| PlanItem {
///     path: PathBuf::from(p),
///     kind,
///     reason: r.into(),
///     bytes: 1,
///     reversibility: rev,
/// };
/// let (admitted, deferred) = split_unattended_items(vec![
///     item("/tmp/probe/target", PlanItemKind::Dir, "rust target", Reversibility::Reversible),
///     item(
///         "/h/proj/erl_crash.dump",
///         PlanItemKind::File,
///         "erlang crash dump (repo root)",
///         Reversibility::Unknown,
///     ),
///     item("/d/web/node_modules", PlanItemKind::Dir, "node dependencies", Reversibility::Reversible),
/// ]);
///
/// // Positive: the target dir and the other reversible item proceed.
/// assert_eq!(admitted.len(), 2);
/// // Negative: the unknown crash dump is deferred, not a veto of the run.
/// assert_eq!(deferred.len(), 1);
/// assert_eq!(deferred[0].path, PathBuf::from("/h/proj/erl_crash.dump"));
///
/// // Positive: a rust target stays admitted even if its label is Unknown.
/// let (admitted, deferred) = split_unattended_items(vec![item(
///     "/d/app/target",
///     PlanItemKind::Dir,
///     "rust target",
///     Reversibility::Unknown,
/// )]);
/// assert_eq!(admitted.len(), 1);
/// assert!(deferred.is_empty());
///
/// // Refusal: an empty plan stays empty.
/// assert!(split_unattended_items(vec![]).0.is_empty());
/// ```
pub fn split_unattended_items(items: Vec<PlanItem>) -> (Vec<PlanItem>, Vec<PlanItem>) {
    items.into_iter().partition(|item| {
        is_rust_target_item(item)
            || !matches!(
                item.reversibility,
                crate::domain::dcm::Reversibility::Unknown
                    | crate::domain::dcm::Reversibility::Irreversible
            )
    })
}

/// Splits items into `(kept, excluded)`, excluding any item whose newest
/// observed mtime (supplied per item by the caller, `None` = unknown) falls
/// within `window_secs` of `now_unix`. An unknown mtime is excluded: a
/// candidate whose recency cannot be established is not reclaimed.
///
/// ```
/// use osx_clnr::domain::pressure::exclude_recent;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |p: &str| PlanItem {
///     path: PathBuf::from(p),
///     kind: PlanItemKind::Dir,
///     reason: "rust target".into(),
///     bytes: 1,
///     reversibility: Reversibility::Reversible,
/// };
/// let now = 100_000;
/// let items = vec![item("/old"), item("/fresh"), item("/unknown")];
/// let mtimes = vec![Some(now - 3 * 3600), Some(now - 600), None];
///
/// let (kept, excluded) = exclude_recent(items, &mtimes, now, 2 * 3600);
/// // Positive: the 3 h old item is kept.
/// assert_eq!(kept.len(), 1);
/// assert_eq!(kept[0].path, PathBuf::from("/old"));
/// // Negative + refusal: the 10-minute-old item and the unknown one are excluded.
/// assert_eq!(excluded.len(), 2);
/// ```
pub fn exclude_recent(
    items: Vec<PlanItem>,
    newest_mtimes: &[Option<i64>],
    now_unix: i64,
    window_secs: u64,
) -> (Vec<PlanItem>, Vec<PlanItem>) {
    let cutoff = now_unix.saturating_sub(window_secs.min(i64::MAX as u64) as i64);
    let mut kept = Vec::new();
    let mut excluded = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        match newest_mtimes.get(i).copied().flatten() {
            Some(m) if m < cutoff => kept.push(item),
            _ => excluded.push(item),
        }
    }
    (kept, excluded)
}
