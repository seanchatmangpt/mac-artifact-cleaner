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
    /// Allow the walk to cross onto other filesystems/APFS volumes reachable
    /// from a scan root (e.g. from `/` onto the System volume, or other
    /// mounted volumes). Default false: the walk is pinned to each root's
    /// own filesystem, so passing `roots: ["/"]` alone does NOT audit the
    /// whole disk on macOS -- `/` and `/Users` are typically separate APFS
    /// volumes joined by firmlinks. Set this true to actually cover them.
    #[serde(default)]
    pub all_filesystems: bool,
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

#[derive(Debug, Clone, Deserialize)]
pub struct AuditBreakdownInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// Root to scan (defaults to the user's home directory).
    #[serde(default)]
    pub root: Option<PathBuf>,
    /// How many path components below `root` to bucket by. 1 = immediate
    /// children only; 2+ splits large catch-all directories (e.g. Library)
    /// into their own children.
    #[serde(default = "default_breakdown_depth")]
    pub depth: u32,
    #[serde(default = "default_breakdown_top")]
    pub top: usize,
    #[serde(default)]
    pub min_mb: u64,
}

fn default_breakdown_depth() -> u32 {
    2
}
fn default_breakdown_top() -> usize {
    40
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakdownEntryOutput {
    pub path: String,
    pub bytes: u64,
    pub percent_of_total: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditBreakdownOutput {
    pub state: String,
    pub root: String,
    pub depth: u32,
    pub disk_total_bytes: u64,
    pub disk_available_bytes: u64,
    pub disk_percent_used: u8,
    pub total_bytes: u64,
    pub entry_count: usize,
    pub entries: Vec<BreakdownEntryOutput>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_verification_warning: Option<String>,
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
    /// Also seal the receipt with an affidavit cryptographic proof chain
    /// (absorbs the former standalone `receipt_certify` tool). Requires
    /// `confirm: true` since it writes a `.affidavit.json` file.
    #[serde(default)]
    pub seal: bool,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiptVerifyOutput {
    pub state: String,
    pub receipt_file: String,
    pub verification_summary: VerificationSummary,
    /// Present only when `seal: true` was requested and sealing succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seal: Option<ReceiptSealOutput>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiptSealOutput {
    pub certified: bool,
    pub chain_hash: String,
    pub content_address: String,
    pub affidavit_file: String,
    pub verdict_reason: String,
    pub profile: String,
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
    /// True when this response describes a preview only: no filesystem
    /// writes were performed and no workflow state was reset.
    pub dry_run: bool,
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

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotThinInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// Real volume mount point to thin snapshots on (e.g. "/"). Callers must
    /// name the mount explicitly; there is no silent default.
    pub mount: String,
    /// Target bytes to reclaim, human-readable (e.g. "10GB", "500MB") or raw digits.
    pub bytes: String,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotThinOutput {
    pub state: String,
    pub mount: String,
    pub requested_bytes: u64,
    pub snapshots_before: usize,
    pub snapshots_after: usize,
    pub snapshots_thinned: Vec<String>,
    pub receipt_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affidavit_file: Option<String>,
    pub message: String,
}

fn default_oldest_n() -> usize {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotDeleteInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// Real volume mount point to delete snapshots on (e.g. "/"). Callers must
    /// name the mount explicitly; there is no silent default.
    pub mount: String,
    /// "oldest" (see `oldest_n`), "all", or an explicit snapshot name/date.
    pub which: String,
    #[serde(default = "default_oldest_n")]
    pub oldest_n: usize,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotDeleteOutput {
    pub state: String,
    pub mount: String,
    pub snapshots_before: usize,
    pub snapshots_after: usize,
    pub snapshots_deleted: Vec<String>,
    pub receipt_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affidavit_file: Option<String>,
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

// ============================================================================
// DOCKER
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct DockerScanInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerScanOutput {
    pub state: String,
    pub raw: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DockerPlanInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerPlanOutput {
    pub state: String,
    pub raw: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DockerPruneInput {
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub skip_colima: bool,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerPruneOutput {
    pub state: String,
    pub raw: String,
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
