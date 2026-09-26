//! Deletion receipt representation.
//!
//! `DeletionReceipt` holds the *operational facts* of an execution — paths,
//! statuses, bytes freed, and the volume free-space samples that the physical
//! reality law ([`DeletionReceipt::verify`]) discharges. The *provenance seal*
//! (the BLAKE3 rolling chain and its structural verification) is owned by
//! affidavit and produced at the
//! [`crate::domain::affidavit_integration`] seam — Pentecost no longer
//! hand-rolls its own receipt chain.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::dcm::Reversibility;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionExecutionRecord {
    pub version: u32,
    pub plan_created_unix: u64,
    pub execution_started_unix: u64,
    pub execution_completed_unix: u64,
    pub results: Vec<DeletionResult>,
    /// Available bytes on the target volume sampled before execution started.
    #[serde(default)]
    pub available_before: Option<u64>,
    /// Available bytes on the target volume sampled after execution completed.
    #[serde(default)]
    pub available_after: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionReceipt {
    pub execution_record: DeletionExecutionRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionResult {
    pub path: PathBuf,
    pub status: DeletionStatus,
    pub error: Option<String>,
    pub blake3_hash: Option<String>,
    /// Physical bytes actually reclaimed by this deletion (the planned size on
    /// `Deleted`, 0 otherwise). Proves total reclaim in the receipt.
    #[serde(default)]
    pub bytes_freed: u64,
    /// The reversibility classification this item carried on the plan at
    /// execution time (copied from `PlanItem::reversibility`). Per DCM §3
    /// ("reversibility is admission, not optimism"), this must survive into
    /// the sealed receipt — the permanent audit record — rather than living
    /// only on the (possibly since-deleted or overwritten) plan file.
    /// `#[serde(default)]` (resolving to [`Reversibility::Unknown`], the
    /// fence value) keeps old receipts deserializable.
    #[serde(default)]
    pub reversibility: Reversibility,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeletionStatus {
    Deleted,
    SkippedMissing,
    Refused,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
    UnsupportedVersion,
    InvalidTimestamps,
    PathStillExists,
    /// A path the receipt marks deleted exists again, but the object there was
    /// created *after* execution started — a live build/process regenerated it
    /// (observed 2026-09-22: `cargo test -p ferroplan` recreated
    /// `ferroplan/target` inside the deletion window). Informational: the
    /// deletion itself happened, so this does not make a receipt inconsistent.
    PathRecreated,
    MissingPlanItem,
    ExtraReceiptItem,
    BytesFreedMismatch,
}

/// What the integration layer observed at a receipt path when verifying.
/// Built outside the domain (a `symlink_metadata` + birthtime read) so this
/// module stays free of filesystem calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathObservation {
    /// Nothing exists at the path.
    Absent,
    /// Something exists at the path; `born_unix` is its creation time when
    /// the filesystem reports one (APFS does), `None` otherwise. `is_symlink`
    /// is true when the entry itself is a symbolic link (observed without
    /// following it).
    Present { born_unix: Option<i64>, is_symlink: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationIssue {
    pub path: PathBuf,
    pub issue_type: IssueType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReport {
    pub is_consistent: bool,
    pub issues: Vec<VerificationIssue>,
}

/// Floor below which a claimed reclaim is too small to witness against volume
/// noise. Below this, [`check_reclaim`] is `NotApplicable`.
pub const RECLAIM_WITNESS_FLOOR_BYTES: u64 = 1_000_000_000;
/// Fraction of the claim the measured volume delta must recover to be witnessed.
pub const RECLAIM_TOLERANCE: f64 = 0.5;

/// The verdict on whether a *claimed* reclaim is witnessed by the *measured*
/// free-space delta of the volume.
///
/// This is the single load-bearing reality law shared by every path that claims
/// to free disk space: receipt `verify()` (post-`delete`) and `emergency`. Both
/// claims are type-identical — "we freed N bytes" — so both must discharge the
/// same witness, here, rather than each re-implementing (or, worse, only one
/// asserting while the other merely prints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimCheck {
    /// Claim too small (< [`RECLAIM_WITNESS_FLOOR_BYTES`]) or a volume sample was
    /// missing — no behavioral claim is being made, so the witness is dormant.
    NotApplicable,
    /// Measured delta recovered at least [`RECLAIM_TOLERANCE`] of the claim.
    Witnessed,
    /// Measured delta fell short of the claim beyond tolerance — the bytes were
    /// not actually returned to the volume (e.g. APFS snapshot still pins them).
    Shortfall { claimed: u64, measured: i128 },
}

/// Pure reality law: does the measured free-space delta witness the claimed
/// reclaim? No filesystem access — callers supply the samples.
///
/// APFS snapshot caveat: blocks pinned by a local snapshot are not returned to
/// free space when their files are deleted, so a large claim can show ~0 measured
/// delta. That is correct signal (a `Shortfall`), not a false positive — the
/// remedy is thinning snapshots, not suppressing the verdict.
///
/// Running example: [`examples/reclaim_check.rs`](../../examples/reclaim_check.rs)
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::receipt::{check_reclaim, ReclaimCheck};
///
/// // Witnessed: measured delta recovers the full claim.
/// assert_eq!(
///     check_reclaim(2_000_000_000, Some(8_000_000_000), Some(10_000_000_000)),
///     ReclaimCheck::Witnessed
/// );
///
/// // Shortfall: large claim, zero movement (snapshot-pinned or never deleted).
/// assert_eq!(
///     check_reclaim(2_000_000_000, Some(5_000_000_000), Some(5_000_000_000)),
///     ReclaimCheck::Shortfall { claimed: 2_000_000_000, measured: 0 }
/// );
///
/// // NotApplicable: claim below the witness floor.
/// assert_eq!(
///     check_reclaim(500_000_000, Some(0), Some(0)),
///     ReclaimCheck::NotApplicable
/// );
///
/// // NotApplicable: a volume sample is missing (back-compat / no measurement).
/// assert_eq!(
///     check_reclaim(5_000_000_000, None, Some(10_000_000_000)),
///     ReclaimCheck::NotApplicable
/// );
/// ```
pub fn check_reclaim(
    claimed_bytes: u64,
    available_before: Option<u64>,
    available_after: Option<u64>,
) -> ReclaimCheck {
    let (Some(before), Some(after)) = (available_before, available_after) else {
        return ReclaimCheck::NotApplicable;
    };
    if claimed_bytes <= RECLAIM_WITNESS_FLOOR_BYTES {
        return ReclaimCheck::NotApplicable;
    }
    let measured = after as i128 - before as i128;
    if (measured as f64) < (claimed_bytes as f64) * RECLAIM_TOLERANCE {
        ReclaimCheck::Shortfall { claimed: claimed_bytes, measured }
    } else {
        ReclaimCheck::Witnessed
    }
}

impl DeletionReceipt {
    /// Constructs a receipt from the operational facts of a deletion run.
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
    ///
    /// let r = DeletionReceipt::new(
    ///     0, 1, 2,
    ///     vec![DeletionResult {
    ///         path: "/tmp/proj/target".into(),
    ///         status: DeletionStatus::Deleted,
    ///         error: None,
    ///         blake3_hash: None,
    ///         bytes_freed: 1024,
    ///         reversibility: Default::default(),
    ///     }],
    ///     Some(8_000_000_000),
    ///     Some(8_000_001_024),
    /// );
    /// assert_eq!(r.execution_record.version, 1);
    /// assert_eq!(r.execution_record.results.len(), 1);
    /// assert_eq!(r.execution_record.available_before, Some(8_000_000_000));
    /// ```
    pub fn new(
        plan_created_unix: u64,
        execution_started_unix: u64,
        execution_completed_unix: u64,
        results: Vec<DeletionResult>,
        available_before: Option<u64>,
        available_after: Option<u64>,
    ) -> Self {
        let execution_record = DeletionExecutionRecord {
            version: 1,
            plan_created_unix,
            execution_started_unix,
            execution_completed_unix,
            results,
            available_before,
            available_after,
        };

        Self { execution_record }
    }

    /// Verify a receipt against an optional plan.
    ///
    /// The BytesFreedMismatch law compares claimed reclaim against the measured
    /// volume free-space delta (floor: measured must be >= 50% of claimed when
    /// claimed > 1 GB).
    ///
    /// Positive case — measured delta ~= claimed, so consistent:
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
    /// let r = DeletionReceipt::new(
    ///     0, 1, 2,
    ///     vec![DeletionResult {
    ///         path: "/nonexistent/path/aaa".into(),
    ///         status: DeletionStatus::SkippedMissing,
    ///         error: None,
    ///         blake3_hash: None,
    ///         bytes_freed: 2_000_000_000,
    ///         reversibility: Default::default(),
    ///     }],
    ///     Some(8_000_000_000),
    ///     Some(10_000_000_000), // delta = +2_000_000_000 == claimed
    /// );
    /// assert!(r.verify(None).is_consistent);
    /// ```
    ///
    /// Refusal case — large claim but zero volume movement raises BytesFreedMismatch:
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};
    /// let r = DeletionReceipt::new(
    ///     0, 1, 2,
    ///     vec![DeletionResult {
    ///         path: "/nonexistent/path/bbb".into(),
    ///         status: DeletionStatus::SkippedMissing,
    ///         error: None,
    ///         blake3_hash: None,
    ///         bytes_freed: 2_000_000_000,
    ///         reversibility: Default::default(),
    ///     }],
    ///     Some(5_000_000_000),
    ///     Some(5_000_000_000), // delta = 0, claim = 2 GB
    /// );
    /// let report = r.verify(None);
    /// assert!(!report.is_consistent);
    /// assert!(report
    ///     .issues
    ///     .iter()
    ///     .any(|i| i.issue_type == IssueType::BytesFreedMismatch));
    /// ```
    pub fn verify(&self, plan: Option<&crate::domain::plan::DeletionPlan>) -> VerificationReport {
        self.verify_with(plan, &|_| PathObservation::Absent)
    }

    /// Verifies the receipt, consulting `observe` for the on-disk state of
    /// every path marked `Deleted`/`SkippedMissing`. [`Self::verify`] is the
    /// offline form (every path treated as absent); the integration layer's
    /// `verify_receipt_on_disk` supplies a real observer.
    ///
    /// A present path born *after* `execution_started_unix` is classified
    /// `PathRecreated` (informational, receipt stays consistent); one born
    /// before — or with unknown birth time — is `PathStillExists` (the delete
    /// did not happen, receipt inconsistent).
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{
    ///     DeletionReceipt, DeletionResult, DeletionStatus, IssueType, PathObservation,
    /// };
    /// let r = DeletionReceipt::new(0, 1_000, 2_000, vec![DeletionResult {
    ///     path: "/p/target".into(), status: DeletionStatus::Deleted, error: None,
    ///     blake3_hash: None, bytes_freed: 0, reversibility: Default::default(),
    /// }], None, None);
    ///
    /// // Positive: absent after deletion → consistent, no issues.
    /// let ok = r.verify_with(None, &|_| PathObservation::Absent);
    /// assert!(ok.is_consistent && ok.issues.is_empty());
    ///
    /// // Recreated by a live build during/after the run → informational only.
    /// let rec = r.verify_with(None, &|_| PathObservation::Present { born_unix: Some(1_500), is_symlink: false });
    /// assert!(rec.is_consistent);
    /// assert_eq!(rec.issues[0].issue_type, IssueType::PathRecreated);
    ///
    /// // Refusal: the same object predates the run → the delete never happened.
    /// let stale = r.verify_with(None, &|_| PathObservation::Present { born_unix: Some(500), is_symlink: false });
    /// assert!(!stale.is_consistent);
    /// assert_eq!(stale.issues[0].issue_type, IssueType::PathStillExists);
    ///
    /// // Refusal: unknown birth time is never assumed to be a recreation.
    /// let unknown = r.verify_with(None, &|_| PathObservation::Present { born_unix: None, is_symlink: false });
    /// assert!(!unknown.is_consistent);
    ///
    /// // A dangling symlink at a `SkippedMissing` path is what "missing" meant
    /// // (the deleter follows links and never deletes one) → consistent.
    /// let skipped = DeletionReceipt::new(0, 1_000, 2_000, vec![DeletionResult {
    ///     path: "/wt/x/node_modules".into(), status: DeletionStatus::SkippedMissing, error: None,
    ///     blake3_hash: None, bytes_freed: 0, reversibility: Default::default(),
    /// }], None, None);
    /// let link = skipped.verify_with(None, &|_| PathObservation::Present { born_unix: Some(500), is_symlink: true });
    /// assert!(link.is_consistent && link.issues.is_empty());
    ///
    /// // …but a symlink at a path the receipt claims `Deleted` is still a lie.
    /// let lied = r.verify_with(None, &|_| PathObservation::Present { born_unix: Some(500), is_symlink: true });
    /// assert!(!lied.is_consistent);
    /// ```
    pub fn verify_with(
        &self,
        plan: Option<&crate::domain::plan::DeletionPlan>,
        observe: &dyn Fn(&std::path::Path) -> PathObservation,
    ) -> VerificationReport {
        let mut issues = Vec::new();

        if self.execution_record.version != 1 {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::UnsupportedVersion,
                message: format!("Unsupported receipt version: {}", self.execution_record.version),
            });
        }

        if self.execution_record.execution_completed_unix
            < self.execution_record.execution_started_unix
        {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::InvalidTimestamps,
                message: format!(
                    "Completed timestamp ({}) is before started timestamp ({})",
                    self.execution_record.execution_completed_unix,
                    self.execution_record.execution_started_unix
                ),
            });
        }

        for result in &self.execution_record.results {
            match result.status {
                DeletionStatus::Deleted | DeletionStatus::SkippedMissing
                    if !result.path.to_string_lossy().starts_with("github://") =>
                {
                    if let PathObservation::Present { born_unix, is_symlink } =
                        observe(&result.path)
                    {
                        // SkippedMissing means "no target when followed": a
                        // dangling symlink there is exactly that state, and the
                        // deleter never removes symlinks.
                        if is_symlink && result.status == DeletionStatus::SkippedMissing {
                            continue;
                        }
                        let started = self.execution_record.execution_started_unix as i64;
                        match born_unix {
                            Some(born) if born > started => issues.push(VerificationIssue {
                                path: result.path.clone(),
                                issue_type: IssueType::PathRecreated,
                                message: format!(
                                    "Path was deleted but recreated at unix {born} (after \
                                     execution started at {started}) — a live process \
                                     regenerated it"
                                ),
                            }),
                            _ => issues.push(VerificationIssue {
                                path: result.path.clone(),
                                issue_type: IssueType::PathStillExists,
                                message: format!(
                                    "Path still exists on disk despite status {:?}",
                                    result.status
                                ),
                            }),
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(p) = plan {
            for item in &p.items {
                if !self.execution_record.results.iter().any(|r| r.path == item.path) {
                    issues.push(VerificationIssue {
                        path: item.path.clone(),
                        issue_type: IssueType::MissingPlanItem,
                        message: "Plan item is missing from execution receipt".to_string(),
                    });
                }
            }

            for result in &self.execution_record.results {
                if !p.items.iter().any(|item| item.path == result.path) {
                    issues.push(VerificationIssue {
                        path: result.path.clone(),
                        issue_type: IssueType::ExtraReceiptItem,
                        message: "Receipt contains path not scheduled in deletion plan".to_string(),
                    });
                }
            }
        }

        // BytesFreedMismatch REALITY law — delegated to the shared `check_reclaim`
        // witness so this path and `emergency` discharge the *same* obligation
        // (the type-identical "we freed N bytes" claim). The old per-item
        // `bytes_freed == item.bytes` check was tautological — `bytes_freed` is
        // copied from `item.bytes` at execution time — so it proved nothing about
        // the real volume; this compares claimed total reclaim against the
        // measured free-space delta instead.
        let bytes_freed_total: u64 =
            self.execution_record.results.iter().map(|r| r.bytes_freed).sum();
        if let ReclaimCheck::Shortfall { claimed, measured } = check_reclaim(
            bytes_freed_total,
            self.execution_record.available_before,
            self.execution_record.available_after,
        ) {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::BytesFreedMismatch,
                message: format!(
                    "Receipt claims {} bytes freed but volume free-space delta measured \
                     only {} bytes (floor={:.0}%)",
                    claimed,
                    measured,
                    RECLAIM_TOLERANCE * 100.0
                ),
            });
        }

        let is_consistent = issues.iter().all(|i| i.issue_type == IssueType::PathRecreated);
        VerificationReport { is_consistent, issues }
    }
}
