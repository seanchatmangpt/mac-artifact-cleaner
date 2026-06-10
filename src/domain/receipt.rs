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
    pub fn new(
        chain_id: String,
        plan_created_unix: u64,
        execution_started_unix: u64,
        execution_completed_unix: u64,
        results: Vec<DeletionResult>,
    ) -> Self {
        let execution_record = DeletionExecutionRecord {
            version: 1,
            plan_created_unix,
            execution_started_unix,
            execution_completed_unix,
            results,
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

        let is_consistent = issues.is_empty();
        VerificationReport {
            is_consistent,
            issues,
        }
    }
}
