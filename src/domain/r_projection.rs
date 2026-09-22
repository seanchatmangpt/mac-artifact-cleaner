//! `R = receipt(A)` projection of native oclnr receipts.
//!
//! oclnr's native receipts (`DeletionReceipt`, `SnapshotThinReceipt`) carry the
//! operation's own evidence — per-path results, free-space samples, snapshot
//! lists — in shapes the fleet-wide receipt schema
//! (`~/.claude/dfcm/receipt.schema.json`: identity, authority, consequence,
//! replay, standing) does not admit. Two receipt schemas for one actuation is
//! drift: a native receipt alone was REFUSED by `validate_receipt.py` on
//! 2026-09-22 (pressure-monitor thin), while a hand-built backfill passed.
//!
//! This module is the pure projection: native receipt + [`ProjectionContext`]
//! (who ran it, under what grant, which binary, which command) →
//! [`RReceipt`]. The native receipt stays the source of truth; the projection
//! references it by `output_sha256` so the two can never silently diverge. The
//! integration layer writes the result next to the native file as
//! `<stem>.r.json`. Zero `std::fs`/`std::process` here.

use serde::{Deserialize, Serialize};

use crate::domain::{
    receipt::{DeletionReceipt, DeletionStatus},
    time::SnapshotThinReceipt,
};

/// Everything about *this invocation* the native receipt does not record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionContext {
    /// Source repo of the binary that acted (the oclnr checkout).
    pub repo: String,
    /// 40-hex commit the acting binary was built from.
    pub build_sha: String,
    /// True when the binary was built with uncommitted tracked changes on top
    /// of `build_sha` — recorded in `identity.subject` so the commit is never
    /// presented as the exact source of a dirty build.
    pub build_dirty: bool,
    /// Who acted (e.g. `oclnr-cli`, `com.oclnr.pressure`, an approver name).
    pub actor: String,
    /// The authority reference: plan approval, policy threshold, or `NONE`.
    pub grant: String,
    /// The command line that performed the actuation.
    pub cmd: String,
    /// Working directory of that command.
    pub cwd: String,
    /// Exit status the actuation is being recorded with.
    pub exit: i32,
    /// Hex sha256 of the native receipt file's exact bytes.
    pub native_sha256: String,
    /// Path of the native receipt file.
    pub native_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RIdentity {
    pub subject: String,
    pub repo: String,
    pub subject_sha: String,
    pub base_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RAuthority {
    pub ceiling: String,
    pub grant: String,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RConsequence {
    pub commits: Vec<String>,
    pub files_changed: Vec<String>,
    pub remote_effects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RCommand {
    pub cmd: String,
    pub cwd: String,
    pub exit: i32,
    pub summary: String,
    pub output_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RReplay {
    pub commands: Vec<RCommand>,
    pub durable_location: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RStanding {
    pub value: String,
    pub derived_from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken_term: Option<String>,
}

/// A receipt in the fleet `R = receipt(A)` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RReceipt {
    pub identity: RIdentity,
    pub authority: RAuthority,
    pub consequence: RConsequence,
    pub replay: RReplay,
    pub standing: RStanding,
}

fn is_sha40(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn assemble(
    ctx: &ProjectionContext,
    subject: String,
    effects: Vec<String>,
    summary: String,
    standing: (&str, Option<&str>),
) -> RReceipt {
    // An unidentifiable build cannot claim any standing: the replay would not
    // be pinned to a subject. Downgrade rather than emit a lying ALIVE.
    let (value, broken_term) = if !is_sha40(&ctx.build_sha) {
        ("REFUSED(unidentified-build)".to_string(), Some("R_missing_identity".to_string()))
    } else if ctx.grant.trim().is_empty() {
        ("REFUSED(no-grant)".to_string(), Some("R_missing_authority".to_string()))
    } else {
        (standing.0.to_string(), standing.1.map(str::to_string))
    };
    RReceipt {
        identity: RIdentity {
            subject: if ctx.build_dirty {
                format!("{subject} [binary built from uncommitted changes on top of subject_sha]")
            } else {
                subject
            },
            repo: ctx.repo.clone(),
            subject_sha: ctx.build_sha.clone(),
            base_sha: ctx.build_sha.clone(),
        },
        authority: RAuthority {
            ceiling: "DO".to_string(),
            grant: if ctx.grant.trim().is_empty() { "NONE".into() } else { ctx.grant.clone() },
            actor: ctx.actor.clone(),
        },
        consequence: RConsequence {
            commits: vec![],
            files_changed: vec![],
            remote_effects: effects,
        },
        replay: RReplay {
            commands: vec![RCommand {
                cmd: ctx.cmd.clone(),
                cwd: ctx.cwd.clone(),
                exit: ctx.exit,
                summary,
                output_sha256: ctx.native_sha256.clone(),
            }],
            durable_location: ctx.native_path.clone(),
        },
        standing: RStanding {
            derived_from: format!(
                "replay[0] (`{}`, exit {}) at oclnr {}; native receipt {} (sha256 {})",
                ctx.cmd,
                ctx.exit,
                &ctx.build_sha.get(..7).unwrap_or(&ctx.build_sha),
                ctx.native_path,
                ctx.native_sha256
            ),
            value,
            broken_term,
        },
    }
}

/// Projects a plan-bound deletion receipt.
///
/// Standing: `ALIVE` when every attempted item was deleted or already missing
/// and the command exited 0; `PARTIAL_ALIVE` when some items failed/were
/// refused (the rest were observed deleted); `BLOCKED:` when nothing was
/// deleted and something failed.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::r_projection::{project_deletion, ProjectionContext};
/// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
///
/// let ctx = ProjectionContext {
///     repo: "/src/osx-clnr".into(), build_sha: "a".repeat(40), build_dirty: false, actor: "alice".into(),
///     grant: "plan-approval:alice".into(), cmd: "oclnr delete execute".into(),
///     cwd: "/w".into(), exit: 0, native_sha256: "b".repeat(64), native_path: "/w/r.jsonocel".into(),
/// };
/// let item = |status| DeletionResult { path: "/p/target".into(), status, error: None,
///     blake3_hash: None, bytes_freed: 10, reversibility: Default::default() };
///
/// // Positive: all deleted → ALIVE, consequence names the reclaim.
/// let r = project_deletion(&DeletionReceipt::new(0, 1, 2, vec![item(DeletionStatus::Deleted)], Some(0), Some(10)), &ctx);
/// assert_eq!(r.standing.value, "ALIVE");
/// assert_eq!(r.replay.commands[0].output_sha256, "b".repeat(64));
/// assert!(r.consequence.remote_effects[0].contains("deleted 1/1"));
///
/// // Negative: a failure alongside a deletion → PARTIAL_ALIVE.
/// let mixed = DeletionReceipt::new(0, 1, 2, vec![item(DeletionStatus::Deleted), item(DeletionStatus::Failed)], None, None);
/// assert_eq!(project_deletion(&mixed, &ctx).standing.value, "PARTIAL_ALIVE");
///
/// // Refusal: nothing deleted, something failed → BLOCKED with a broken term.
/// let dead = DeletionReceipt::new(0, 1, 2, vec![item(DeletionStatus::Failed)], None, None);
/// let b = project_deletion(&dead, &ctx);
/// assert!(b.standing.value.starts_with("BLOCKED"));
/// assert!(b.standing.broken_term.is_some());
///
/// // Refusal: an unidentified build never claims ALIVE.
/// let anon = ProjectionContext { build_sha: "unknown".into(), ..ctx.clone() };
/// let a = project_deletion(&DeletionReceipt::new(0, 1, 2, vec![item(DeletionStatus::Deleted)], None, None), &anon);
/// assert!(a.standing.value.starts_with("REFUSED"));
/// ```
pub fn project_deletion(receipt: &DeletionReceipt, ctx: &ProjectionContext) -> RReceipt {
    let results = &receipt.execution_record.results;
    let count = |s: DeletionStatus| results.iter().filter(|r| r.status == s).count();
    let (deleted, skipped, failed, refused) = (
        count(DeletionStatus::Deleted),
        count(DeletionStatus::SkippedMissing),
        count(DeletionStatus::Failed),
        count(DeletionStatus::Refused),
    );
    let bytes: u64 = results.iter().map(|r| r.bytes_freed).sum();
    let measured =
        match (receipt.execution_record.available_before, receipt.execution_record.available_after)
        {
            (Some(b), Some(a)) => format!("{} bytes", a as i64 - b as i64),
            _ => "not sampled".to_string(),
        };
    let effects = vec![
        format!(
            "local filesystem: deleted {deleted}/{} plan items ({bytes} bytes claimed), \
             {skipped} already missing, {failed} failed, {refused} refused",
            results.len()
        ),
        format!("volume free-space delta measured: {measured}"),
    ];
    let bad = failed + refused;
    let standing = if ctx.exit != 0 {
        ("BLOCKED:nonzero-exit", Some("mu_unlawful"))
    } else if bad == 0 {
        ("ALIVE", None)
    } else if deleted > 0 {
        ("PARTIAL_ALIVE", None)
    } else {
        ("BLOCKED:no-item-deleted", Some("mu_unlawful"))
    };
    assemble(
        ctx,
        format!("oclnr delete execute ({} items)", results.len()),
        effects,
        format!("deleted={deleted} skipped={skipped} failed={failed} refused={refused}"),
        standing,
    )
}

/// Projects a snapshot thin receipt (manual `snapshot thin` or the pressure
/// monitor). A thin that removed zero snapshots is still `ALIVE` when the
/// command succeeded — "nothing to thin" is an observed outcome, and the
/// consequence says so explicitly rather than implying reclaim.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::r_projection::{project_snapshot_thin, ProjectionContext};
/// use osx_clnr::domain::time::SnapshotThinReceipt;
///
/// let ctx = ProjectionContext {
///     repo: "/src/osx-clnr".into(), build_sha: "c".repeat(40), build_dirty: true, actor: "com.oclnr.pressure".into(),
///     grant: "pressure-policy:threshold=20GB".into(), cmd: "oclnr monitor --watch".into(),
///     cwd: "/".into(), exit: 0, native_sha256: "d".repeat(64), native_path: "/l/thin.json".into(),
/// };
///
/// // Positive: two snapshots thinned.
/// let r = SnapshotThinReceipt::new("/".into(), 10, 0, vec!["a".into(), "b".into()], vec![]);
/// let p = project_snapshot_thin(&r, &ctx);
/// assert_eq!(p.standing.value, "ALIVE");
/// assert!(p.consequence.remote_effects[0].contains("thinned 2"));
/// assert!(p.identity.subject.contains("uncommitted"));
///
/// // Negative: none existed → ALIVE, but the consequence says zero.
/// let none = SnapshotThinReceipt::new("/".into(), 10, 0, vec![], vec![]);
/// assert!(project_snapshot_thin(&none, &ctx).consequence.remote_effects[0].contains("thinned 0"));
///
/// // Refusal: empty grant → REFUSED, R_missing_authority.
/// let nogrant = ProjectionContext { grant: " ".into(), ..ctx };
/// let q = project_snapshot_thin(&r, &nogrant);
/// assert_eq!(q.standing.broken_term.as_deref(), Some("R_missing_authority"));
/// ```
pub fn project_snapshot_thin(receipt: &SnapshotThinReceipt, ctx: &ProjectionContext) -> RReceipt {
    let effects = vec![format!(
        "APFS local snapshots on {}: thinned {} (before {}, after {}), requested {} bytes",
        receipt.volume,
        receipt.snapshots_thinned.len(),
        receipt.snapshots_before.len(),
        receipt.snapshots_after.len(),
        receipt.requested_bytes
    )];
    let standing =
        if ctx.exit == 0 { ("ALIVE", None) } else { ("BLOCKED:nonzero-exit", Some("mu_unlawful")) };
    assemble(
        ctx,
        format!("oclnr snapshot thin {}", receipt.volume),
        effects,
        format!("thinned={}", receipt.snapshots_thinned.len()),
        standing,
    )
}
