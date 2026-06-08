//! Deletion receipt representation.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionReceipt {
    pub version: u32,
    pub plan_created_unix: u64,
    pub execution_started_unix: u64,
    pub execution_completed_unix: u64,
    pub results: Vec<DeletionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionResult {
    pub path: PathBuf,
    pub status: DeletionStatus,
    pub error: Option<String>,
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

impl DeletionReceipt {
    /// Creates a new deletion receipt.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
    /// use std::path::PathBuf;
    ///
    /// // Positive case: construct a receipt and verify properties
    /// let receipt = DeletionReceipt::new(1716768000, 1716768100, 1716768200, vec![]);
    /// assert_eq!(receipt.version, 1);
    /// assert!(receipt.results.is_empty());
    /// ```
    pub fn new(
        plan_created_unix: u64,
        execution_started_unix: u64,
        execution_completed_unix: u64,
        results: Vec<DeletionResult>,
    ) -> Self {
        Self {
            version: 1,
            plan_created_unix,
            execution_started_unix,
            execution_completed_unix,
            results,
        }
    }

    /// Verifies the deletion receipt for consistency and correctness.
    ///
    /// Under normal circumstances, any path marked `Deleted` or `SkippedMissing`
    /// should not exist on the filesystem. Conversely, if a plan is supplied,
    /// we verify that all items in the plan are present in the receipt, and no
    /// extra items were deleted.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};
    /// use std::path::PathBuf;
    ///
    /// // Positive case: a correct, consistent receipt for nonexistent files
    /// let receipt = DeletionReceipt::new(1716768000, 1716768100, 1716768200, vec![
    ///     DeletionResult {
    ///         path: PathBuf::from("/nonexistent/path/12345/abc"),
    ///         status: DeletionStatus::Deleted,
    ///         error: None,
    ///     }
    /// ]);
    /// let report = receipt.verify(None);
    /// assert!(report.is_consistent);
    /// assert!(report.issues.is_empty());
    ///
    /// // Negative case: file marked as Deleted but still exists
    /// let bad_receipt = DeletionReceipt::new(1716768000, 1716768100, 1716768200, vec![
    ///     DeletionResult {
    ///         path: PathBuf::from("."),
    ///         status: DeletionStatus::Deleted,
    ///         error: None,
    ///     }
    /// ]);
    /// let report = bad_receipt.verify(None);
    /// assert!(!report.is_consistent);
    /// assert_eq!(report.issues.len(), 1);
    /// assert_eq!(report.issues[0].issue_type, IssueType::PathStillExists);
    /// ```
    pub fn verify(&self, plan: Option<&crate::domain::plan::DeletionPlan>) -> VerificationReport {
        let mut issues = Vec::new();

        // 1. Version check
        if self.version != 1 {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::UnsupportedVersion,
                message: format!("Unsupported receipt version: {}", self.version),
            });
        }

        // 2. Timestamp validation
        if self.execution_completed_unix < self.execution_started_unix {
            issues.push(VerificationIssue {
                path: PathBuf::new(),
                issue_type: IssueType::InvalidTimestamps,
                message: format!(
                    "Completed timestamp ({}) is before started timestamp ({})",
                    self.execution_completed_unix, self.execution_started_unix
                ),
            });
        }

        // 3. Filesystem consistency checks
        for result in &self.results {
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

        // 4. Plan correlation check
        if let Some(p) = plan {
            for item in &p.items {
                if !self.results.iter().any(|r| r.path == item.path) {
                    issues.push(VerificationIssue {
                        path: item.path.clone(),
                        issue_type: IssueType::MissingPlanItem,
                        message: "Plan item is missing from execution receipt".to_string(),
                    });
                }
            }

            for result in &self.results {
                if !p.items.iter().any(|item| item.path == result.path) {
                    issues.push(VerificationIssue {
                        path: result.path.clone(),
                        issue_type: IssueType::ExtraReceiptItem,
                        message: "Receipt contains path not scheduled in deletion plan".to_string(),
                    });
                }
            }
        }

        let is_consistent = issues.is_empty();
        VerificationReport {
            is_consistent,
            issues,
        }
    }
}
