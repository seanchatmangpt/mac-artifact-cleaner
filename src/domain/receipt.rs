//! Deletion receipt representation.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use wasm4pm_compat::receipt::{
    Digest, ReceiptChain, ReceiptEnvelope, ReceiptRefusal, ReceiptShape, ReplayHint,
};

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
    #[serde(skip)]
    pub chain: Option<ReceiptChain>,
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
    MissingPlanItem,
    ExtraReceiptItem,
    BytesFreedMismatch,
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
        ReclaimCheck::Shortfall {
            claimed: claimed_bytes,
            measured,
        }
    } else {
        ReclaimCheck::Witnessed
    }
}

impl DeletionReceipt {
    pub fn new(
        chain_id: String,
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

        let record_json = serde_json::to_string(&execution_record).unwrap_or_default();
        let digest_str = blake3::hash(record_json.as_bytes()).to_hex().to_string();

        let link = ReceiptEnvelope::new(
            "deletion-execution",
            "osx-clnr-engine",
            Digest::new(format!("blake3:{}", digest_str)),
            ReplayHint::new(format!("osx-clnr://verify/{}", chain_id)),
        );

        let chain = ReceiptChain::try_new(chain_id, vec![link]).unwrap();

        Self {
            chain: Some(chain),
            execution_record,
        }
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
    ///     "c".to_string(), 0, 1, 2,
    ///     vec![DeletionResult {
    ///         path: "/nonexistent/path/aaa".into(),
    ///         status: DeletionStatus::SkippedMissing,
    ///         error: None,
    ///         blake3_hash: None,
    ///         bytes_freed: 2_000_000_000,
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
    ///     "c".to_string(), 0, 1, 2,
    ///     vec![DeletionResult {
    ///         path: "/nonexistent/path/bbb".into(),
    ///         status: DeletionStatus::SkippedMissing,
    ///         error: None,
    ///         blake3_hash: None,
    ///         bytes_freed: 2_000_000_000,
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
        let mut issues = Vec::new();

        if self.execution_record.version != 1 {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::UnsupportedVersion,
                message: format!(
                    "Unsupported receipt version: {}",
                    self.execution_record.version
                ),
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
                    if result.path.exists() =>
                {
                    issues.push(VerificationIssue {
                        path: result.path.clone(),
                        issue_type: IssueType::PathStillExists,
                        message: format!(
                            "Path still exists on disk despite status {:?}",
                            result.status
                        ),
                    });
                }
                _ => {}
            }
        }

        if let Some(p) = plan {
            for item in &p.items {
                if !self
                    .execution_record
                    .results
                    .iter()
                    .any(|r| r.path == item.path)
                {
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
        let bytes_freed_total: u64 = self
            .execution_record
            .results
            .iter()
            .map(|r| r.bytes_freed)
            .sum();
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

        let is_consistent = issues.is_empty();
        VerificationReport {
            is_consistent,
            issues,
        }
    }
}
