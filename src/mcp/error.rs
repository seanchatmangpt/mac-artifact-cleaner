//! Error handling with recovery suggestions
//!
//! Comprehensive error types with contextual recovery paths.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // JSON-RPC standard errors
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,

    // Domain-specific errors
    InvalidStateTransition = 1001,
    AuditNotComplete = 1002,
    AuditFailed = 1003,
    PlanNotApproved = 1004,
    PlanApprovalSignatureInvalid = 1005,
    FilesystemChanged = 1006,
    ConfirmationRequired = 1007,
    FileNotFound = 1008,
    IoError = 1009,
    InvalidInput = 1010,
    SubprocessFailed = 1011,
    JsonParseError = 1012,
    StateDowngrade = 1013,
    PartialFailure = 1014,
    SnapshotNotFound = 1015,
    LowDiskSpace = 1016,
    PathSecurityViolation = 1017,
}

impl ErrorCode {
    pub fn as_i32(&self) -> i32 {
        *self as i32
    }

    pub fn message(&self) -> &'static str {
        match self {
            ErrorCode::ParseError => "Parse error",
            ErrorCode::InvalidRequest => "Invalid Request",
            ErrorCode::MethodNotFound => "Method not found",
            ErrorCode::InvalidParams => "Invalid method parameter(s)",
            ErrorCode::InternalError => "Internal error",
            ErrorCode::InvalidStateTransition => "Invalid state transition",
            ErrorCode::AuditNotComplete => "Audit has not been completed",
            ErrorCode::AuditFailed => "Audit failed",
            ErrorCode::PlanNotApproved => "Plan has not been approved",
            ErrorCode::PlanApprovalSignatureInvalid => {
                "Plan approval signature invalid (tampering detected)"
            }
            ErrorCode::FilesystemChanged => "Filesystem state changed since plan creation",
            ErrorCode::ConfirmationRequired => "Confirmation required (confirm=true)",
            ErrorCode::FileNotFound => "File not found",
            ErrorCode::IoError => "I/O error",
            ErrorCode::InvalidInput => "Invalid input",
            ErrorCode::SubprocessFailed => "Subprocess failed",
            ErrorCode::JsonParseError => "JSON parse error",
            ErrorCode::StateDowngrade => "State downgraded due to missing evidence",
            ErrorCode::PartialFailure => "Deletion completed with partial failures",
            ErrorCode::SnapshotNotFound => "Snapshot not found",
            ErrorCode::LowDiskSpace => "Low disk space",
            ErrorCode::PathSecurityViolation => "Path security violation",
        }
    }
}

/// Structured error response with recovery suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_suggestions: Option<Vec<String>>,
}

impl ErrorResponse {
    /// Create new error response
    pub fn new(code: ErrorCode, message: String) -> Self {
        Self {
            code: code.message().to_string(),
            message,
            path: None,
            context: None,
            recovery_suggestions: None,
        }
    }

    /// Add recovery suggestions
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.recovery_suggestions = Some(suggestions);
        self
    }

    /// Add path context
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path.display().to_string());
        self
    }

    /// Add contextual data
    pub fn with_context(mut self, context: Value) -> Self {
        self.context = Some(context);
        self
    }

    // Specific error constructors

    pub fn file_not_found(path: &std::path::Path, operation: &str) -> Self {
        Self::new(ErrorCode::FileNotFound, format!("Expected file not found: {}", path.display()))
            .with_path(path.to_path_buf())
            .with_suggestions(vec![
                format!("Check that {} was created by {}", path.display(), operation),
                "Re-run the operation to regenerate the file".to_string(),
                format!("Check file permissions on {}", path.display()),
            ])
    }

    pub fn plan_not_approved() -> Self {
        Self::new(ErrorCode::PlanNotApproved, "Cannot delete without an approved plan".to_string())
            .with_suggestions(vec![
                "Call plan_validate() to check plan safety".to_string(),
                "Call plan_approve() with confirm=true to approve".to_string(),
                "Review plan with plan_inspect() before approving".to_string(),
            ])
    }

    pub fn approval_signature_invalid() -> Self {
        Self::new(
            ErrorCode::PlanApprovalSignatureInvalid,
            "Plan approval signature is invalid (tampering detected)".to_string(),
        )
        .with_suggestions(vec![
            "Plan file was modified after approval".to_string(),
            "Discard plan and create a new one with plan_build()".to_string(),
            "Check that plan file has not been manually edited".to_string(),
        ])
    }

    pub fn filesystem_changed() -> Self {
        Self::new(
            ErrorCode::FilesystemChanged,
            "Filesystem state has changed since plan was created".to_string(),
        )
        .with_suggestions(vec![
            "Re-run audit_scan() to get current state".to_string(),
            "Rebuild the plan with plan_build()".to_string(),
            "Verify no external processes modified the filesystem".to_string(),
        ])
    }

    pub fn confirmation_required(operation: &str) -> Self {
        Self::new(
            ErrorCode::ConfirmationRequired,
            format!("Must set confirm=true to {} (irreversible action)", operation),
        )
        .with_suggestions(vec![
            format!("Review the operation carefully before confirming"),
            format!("Re-run with confirm=true to execute {}", operation),
            "This is a safety gate to prevent accidental operations".to_string(),
        ])
    }

    pub fn invalid_state_transition(current: &str, next: &str) -> Self {
        Self::new(
            ErrorCode::InvalidStateTransition,
            format!("Cannot transition from {} to {}", current, next),
        )
        .with_suggestions(vec![
            "Call query_workflow_state() to check current state".to_string(),
            "Call clear_artifacts() to reset if needed".to_string(),
            "Review the workflow state machine documentation".to_string(),
        ])
    }

    pub fn audit_not_complete() -> Self {
        Self::new(
            ErrorCode::AuditNotComplete,
            "Cannot build plan without completed audit".to_string(),
        )
        .with_suggestions(vec![
            "Call audit_scan() to run a full filesystem audit".to_string(),
            "Or call audit_parse() to load existing audit results".to_string(),
        ])
    }

    pub fn subprocess_failed(cmd: &str, stderr: &str) -> Self {
        Self::new(ErrorCode::SubprocessFailed, format!("Subprocess '{}' failed", cmd))
            .with_context(serde_json::json!({"stderr": stderr}))
            .with_suggestions(vec![
                "Check oclnr binary is installed and in PATH".to_string(),
                "Review subprocess stderr output for details".to_string(),
                "Run 'oclnr doctor' to diagnose system issues".to_string(),
            ])
    }

    pub fn json_parse_error(message: &str) -> Self {
        Self::new(ErrorCode::JsonParseError, format!("Failed to parse JSON: {}", message))
            .with_suggestions(vec![
                "Check that evidence files are valid JSON/JSONOCEL".to_string(),
                "Re-run the operation to regenerate evidence".to_string(),
            ])
    }

    pub fn path_security_violation(path: &std::path::Path, reason: &str) -> Self {
        Self::new(
            ErrorCode::PathSecurityViolation,
            format!("Path security violation: {} - {}", path.display(), reason),
        )
        .with_path(path.to_path_buf())
        .with_suggestions(vec![
            "Check plan does not include system directories".to_string(),
            "Verify plan respects path protection rules".to_string(),
            "Run plan_validate() to check for security issues".to_string(),
        ])
    }

    pub fn low_disk_space(available_gb: f64, needed_gb: f64) -> Self {
        Self::new(
            ErrorCode::LowDiskSpace,
            format!(
                "Insufficient disk space: {} GB available, {} GB needed",
                available_gb, needed_gb
            ),
        )
        .with_suggestions(vec![
            "Call emergency_reclaim() to free up space urgently".to_string(),
            "Reduce plan scope with max_reclaim_gb parameter".to_string(),
            "Run 'oclnr snapshot thin' to reclaim APFS snapshot space".to_string(),
        ])
    }

    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_messages() {
        assert_eq!(ErrorCode::ParseError.message(), "Parse error");
        assert_eq!(ErrorCode::AuditNotComplete.message(), "Audit has not been completed");
    }

    #[test]
    fn test_error_response_with_suggestions() {
        let err = ErrorResponse::file_not_found(&PathBuf::from("/tmp/test.json"), "audit_scan");
        assert!(err.recovery_suggestions.is_some());
        assert!(err.recovery_suggestions.unwrap().len() > 0);
    }

    #[test]
    fn test_error_response_serialization() {
        let err = ErrorResponse::new(ErrorCode::AuditNotComplete, "No audit".to_string())
            .with_suggestions(vec!["Run audit_scan".to_string()]);
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\""));
        assert!(json.contains("\"recovery_suggestions\""));
    }
}
