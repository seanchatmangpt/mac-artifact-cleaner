//! MCP Tool implementations
//!
//! All 19 MCP tools for the cleanup workflow.

use std::{collections::HashMap, path::PathBuf};

use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

// ============================================================================
// INPUT/OUTPUT TYPES
// ============================================================================

/// Artifact kind
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum ArtifactKind {
    Dir,
    File,
}

/// Project type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Docker,
    Xcode,
    Generic,
    ToolRoot,
}

/// Deletion status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DeletionStatus {
    Deleted,
    Failed,
    SkippedMissing,
    Refused,
    Timeout,
}

/// Cleanup candidate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub bytes: u64,
    pub reason: String,
    pub project_type: ProjectType,
}

/// Deletion result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionResult {
    pub path: PathBuf,
    pub status: DeletionStatus,
    pub bytes_freed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blake3_hash: Option<String>,
}

/// Safety checks result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyChecks {
    pub os_directory_protection: bool,
    pub no_dotfiles_in_home: bool,
    pub max_reclaim_respected: bool,
    pub audit_integrity_ok: bool,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Approval metadata with HMAC signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalMetadata {
    pub approved_at_unix: i64,
    pub approved_at_iso: String,
    pub approver: String,
    pub approval_reason: String,
    pub approval_signature: String,
}

impl ApprovalMetadata {
    pub fn new(approver: String, approval_reason: String) -> Self {
        let now = Utc::now();
        Self {
            approved_at_unix: now.timestamp(),
            approved_at_iso: now.to_rfc3339(),
            approver,
            approval_reason,
            approval_signature: String::new(),
        }
    }

    /// Sign approval with HMAC-SHA256
    pub fn sign(&mut self, plan_content: &str, secret: &[u8]) -> Result<(), String> {
        let mut mac =
            HmacSha256::new_from_slice(secret).map_err(|e| format!("HMAC key error: {}", e))?;

        mac.update(plan_content.as_bytes());
        mac.update(self.approver.as_bytes());
        mac.update(self.approval_reason.as_bytes());

        let result = mac.finalize();
        self.approval_signature = hex::encode(result.into_bytes());
        Ok(())
    }

    /// Verify approval signature
    pub fn verify(&self, plan_content: &str, secret: &[u8]) -> Result<(), String> {
        let mut mac =
            HmacSha256::new_from_slice(secret).map_err(|e| format!("HMAC key error: {}", e))?;

        mac.update(plan_content.as_bytes());
        mac.update(self.approver.as_bytes());
        mac.update(self.approval_reason.as_bytes());

        let result = mac.finalize();
        let expected = hex::encode(result.into_bytes());
        if expected == self.approval_signature {
            Ok(())
        } else {
            Err("Approval signature verification failed: plan was modified after approval"
                .to_string())
        }
    }
}

// ============================================================================
// AUDIT INPUTS/OUTPUTS
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AuditScanInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub include_deps: bool,
    #[serde(default)]
    pub include_aggressive: bool,
    #[serde(default = "default_ignore_recent_hours")]
    pub ignore_recent_hours: u32,
    #[serde(default)]
    pub tool_roots: bool,
}

fn default_ignore_recent_hours() -> u32 {
    168
}
fn default_max_concurrent() -> usize {
    4
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditSummary {
    pub total_dirs: usize,
    pub total_files: usize,
    pub total_bytes: u64,
    pub total_candidates: usize,
    pub projects_detected: HashMap<String, usize>,
    pub largest_candidates: Vec<Candidate>,
    #[serde(default)]
    pub errors: Vec<String>,
    pub scan_duration_secs: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditScanOutput {
    pub state: String,
    pub audit_file: String,
    pub summary: AuditSummary,
    pub message: String,
}

// ============================================================================
// PLAN INPUTS/OUTPUTS
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct PlanBuildInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub audit_file: Option<PathBuf>,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub candidates: Option<Vec<Value>>,
    #[serde(default)]
    pub deps: bool,
    #[serde(default)]
    pub aggressive: bool,
    #[serde(default)]
    pub include_global_caches: bool,
    #[serde(default)]
    pub max_reclaim_gb: Option<f64>,
    /// Recency override for this plan build. When omitted, falls back to the
    /// workflow context's audit_scan recency choice (and finally the CLI's
    /// 168h default) so a plan stays consistent with the audit that produced
    /// it, unless the caller explicitly overrides it.
    #[serde(default)]
    pub ignore_recent_hours: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub created_unix: i64,
    pub created_iso: String,
    pub audit_referenced: String,
    pub total_items: usize,
    pub total_bytes: u64,
    pub items_by_type: HashMap<String, usize>,
    pub items_by_reason: HashMap<String, usize>,
    #[serde(default)]
    pub exclusions: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanBuildOutput {
    pub state: String,
    pub plan_file: String,
    pub plan_summary: PlanSummary,
    pub safety_checks: SafetyChecks,
    pub message: String,
}

// ============================================================================
// PLAN APPROVAL/VALIDATION
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct PlanValidateInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    pub plan_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanValidateOutput {
    pub valid: bool,
    pub safety_checks: SafetyChecks,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanApproveInput {
    pub plan_file: PathBuf,
    pub approver_name: String,
    pub approval_reason: String,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanApproveOutput {
    pub state: String,
    pub plan_file: String,
    pub approval_metadata: ApprovalMetadata,
    pub message: String,
}

// ============================================================================
// DELETION
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteDryRunInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    pub plan_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePreview {
    pub total_items: usize,
    pub total_bytes: u64,
    pub items_by_status: HashMap<String, usize>,
    pub preview_items: Vec<DeletionResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteDryRunOutput {
    pub message: String,
    pub preview: DeletePreview,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteExecuteInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    pub plan_file: PathBuf,
    #[serde(default)]
    pub receipt_file: Option<PathBuf>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
}

fn default_timeout_secs() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    pub total_attempted: usize,
    pub successful: usize,
    pub failed: usize,
    pub skipped: usize,
    pub refused: usize,
    pub total_bytes_freed: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskSpaceInfo {
    pub free_before_bytes: u64,
    pub free_after_bytes: u64,
    pub freed_delta_bytes: i64,
    pub measurement_time: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionRecord {
    pub plan_file: String,
    pub execution_started_unix: i64,
    pub execution_completed_unix: i64,
    pub duration_secs: f64,
    pub results: Vec<DeletionResult>,
    pub summary: ExecutionSummary,
    pub disk_space: DiskSpaceInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affidavit_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteExecuteOutput {
    pub state: String,
    pub execution_record: ExecutionRecord,
    pub receipt_file: String,
    pub message: String,
}

// ============================================================================
// RECEIPT
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ReceiptVerifyInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub receipt_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiptVerifyOutput {
    pub state: String,
    pub receipt_file: String,
    pub verification_summary: VerificationSummary,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationSummary {
    pub verified_unix: i64,
    pub verified_iso: String,
    pub total_deletions_recorded: usize,
    pub total_bytes_freed_recorded: u64,
    pub actual_free_space_delta: i64,
    pub all_targets_gone: bool,
    pub affidavit_verified: bool,
}

// ============================================================================
// QUERY & STATE
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct QueryWorkflowStateOutput {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_audit_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_plan_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_delete_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affidavit_file: Option<String>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClearArtifactsInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub archive_to: Option<PathBuf>,
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    #[serde(default)]
    pub confirm: bool,
}

fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedFile {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearArtifactsOutput {
    pub success: bool,
    pub archived_files: Vec<ArchivedFile>,
    pub archive_location: String,
    pub timestamp: String,
}

// ============================================================================
// SNAPSHOT
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotAuditInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotInfo {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub age_hours: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotAuditOutput {
    pub state: String,
    pub total_snapshots: usize,
    pub total_bytes: u64,
    pub snapshots: Vec<SnapshotInfo>,
    pub message: String,
}

// ============================================================================
// EMERGENCY
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct EmergencyReclaimInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// Real volume mount point to reclaim space on (e.g. "/"). This operation
    /// is NOT scoped to `workspace` — it sweeps real home-directory caches
    /// and real APFS snapshots on the given mount. Callers must name the
    /// mount explicitly; there is no silent default.
    pub mount: String,
    pub target_free_gb: f64,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmergencyReclaimOutput {
    pub state: String,
    pub space_freed: u64,
    pub snapshots_thinned: usize,
    pub caches_cleared: usize,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_metadata_sign_verify() {
        let mut metadata = ApprovalMetadata::new("tester".to_string(), "testing".to_string());
        let secret = b"secret";
        let plan_content = "plan content";

        assert!(metadata.sign(plan_content, secret).is_ok());
        assert!(metadata.verify(plan_content, secret).is_ok());
        assert!(metadata.verify("modified", secret).is_err());
    }

    #[test]
    fn test_artifact_kind_serialization() {
        let kind = ArtifactKind::Dir;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, r#""Dir""#);
    }

    #[test]
    fn test_project_type_serialization() {
        let ptype = ProjectType::Rust;
        let json = serde_json::to_string(&ptype).unwrap();
        assert_eq!(json, r#""rust""#);
    }
}
