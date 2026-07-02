//! Workflow state machine management
//!
//! Maintains the state of the cleanup workflow (UNSTARTED → CLEANUP_COMPLETE)
//! with validation of legal state transitions.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Workflow states (12-state machine)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowState {
    /// Initial state, no operations performed
    Unstarted,
    /// Audit needed, ready to run
    AuditNeeded,
    /// Audit is currently running
    AuditInProgress,
    /// Audit completed successfully
    AuditComplete,
    /// Audit failed
    AuditFailed,
    /// Plan needs to be built
    PlanNeeded,
    /// Plan building in progress
    PlanInProgress,
    /// Plan ready for review
    PlanReady,
    /// Plan validation failed
    PlanValidationFailed,
    /// Plan approved and signed
    PlanApproved,
    /// Delete operation ready
    DeleteNeeded,
    /// Delete operation in progress
    DeleteInProgress,
    /// Delete operation completed
    DeleteComplete,
    /// Delete operation failed
    DeleteFailed,
    /// Receipt ready for verification
    ReceiptReady,
    /// Receipt verification in progress
    VerificationInProgress,
    /// Cleanup complete and verified
    CleanupComplete,
    /// Cleanup failed
    CleanupFailed,
}

impl WorkflowState {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowState::Unstarted => "UNSTARTED",
            WorkflowState::AuditNeeded => "AUDIT_NEEDED",
            WorkflowState::AuditInProgress => "AUDIT_IN_PROGRESS",
            WorkflowState::AuditComplete => "AUDIT_COMPLETE",
            WorkflowState::AuditFailed => "AUDIT_FAILED",
            WorkflowState::PlanNeeded => "PLAN_NEEDED",
            WorkflowState::PlanInProgress => "PLAN_IN_PROGRESS",
            WorkflowState::PlanReady => "PLAN_READY",
            WorkflowState::PlanValidationFailed => "PLAN_VALIDATION_FAILED",
            WorkflowState::PlanApproved => "PLAN_APPROVED",
            WorkflowState::DeleteNeeded => "DELETE_NEEDED",
            WorkflowState::DeleteInProgress => "DELETE_IN_PROGRESS",
            WorkflowState::DeleteComplete => "DELETE_COMPLETE",
            WorkflowState::DeleteFailed => "DELETE_FAILED",
            WorkflowState::ReceiptReady => "RECEIPT_READY",
            WorkflowState::VerificationInProgress => "VERIFICATION_IN_PROGRESS",
            WorkflowState::CleanupComplete => "CLEANUP_COMPLETE",
            WorkflowState::CleanupFailed => "CLEANUP_FAILED",
        }
    }
}

/// Workflow context - tracks state and artifacts throughout cleanup process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContext {
    /// Unique ID for this workflow instance
    pub context_id: String,
    /// Current state
    pub state: WorkflowState,
    /// Workspace root directory
    pub workspace: PathBuf,
    /// Path to disk-audit.jsonocel
    pub audit_file: Option<PathBuf>,
    /// Path to cleanup-plan.json
    pub plan_file: Option<PathBuf>,
    /// Path to deletion-receipt.jsonocel
    pub receipt_file: Option<PathBuf>,
    /// Path to affidavit certification file
    pub affidavit_file: Option<PathBuf>,
    /// Last successful audit timestamp
    pub last_audit_time: Option<DateTime<Utc>>,
    /// Last plan creation timestamp
    pub last_plan_time: Option<DateTime<Utc>>,
    /// Last deletion execution timestamp
    pub last_delete_time: Option<DateTime<Utc>>,
    /// Context creation time
    pub created_at: DateTime<Utc>,
    /// Latest update time
    pub updated_at: DateTime<Utc>,
    /// Optional error message for failed states
    pub last_error: Option<String>,
}

impl WorkflowContext {
    /// Create new workflow context
    pub fn new(workspace: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            context_id: Uuid::new_v4().to_string(),
            state: WorkflowState::Unstarted,
            workspace,
            audit_file: None,
            plan_file: None,
            receipt_file: None,
            affidavit_file: None,
            last_audit_time: None,
            last_plan_time: None,
            last_delete_time: None,
            created_at: now,
            updated_at: now,
            last_error: None,
        }
    }

    /// Validate state transition
    pub fn can_transition_to(&self, next: WorkflowState) -> Result<(), String> {
        match (&self.state, next) {
            // Can start audit from unstarted or after completion
            (WorkflowState::Unstarted, WorkflowState::AuditNeeded) => Ok(()),
            (WorkflowState::CleanupComplete, WorkflowState::AuditNeeded) => Ok(()),
            (WorkflowState::AuditNeeded, WorkflowState::AuditInProgress) => Ok(()),
            (WorkflowState::AuditInProgress, WorkflowState::AuditComplete) => Ok(()),

            // Audit → Plan
            (WorkflowState::AuditComplete, WorkflowState::PlanNeeded) => Ok(()),
            (WorkflowState::PlanNeeded, WorkflowState::PlanInProgress) => Ok(()),

            // Plan validation
            (WorkflowState::PlanInProgress, WorkflowState::PlanReady) => Ok(()),
            (WorkflowState::PlanReady, WorkflowState::PlanInProgress) => Ok(()), // Re-validate

            // Plan approval
            (WorkflowState::PlanReady, WorkflowState::PlanApproved) => Ok(()),
            (WorkflowState::PlanValidationFailed, WorkflowState::PlanInProgress) => Ok(()), // Retry

            // Delete
            (WorkflowState::PlanApproved, WorkflowState::DeleteNeeded) => Ok(()),
            (WorkflowState::DeleteNeeded, WorkflowState::DeleteInProgress) => Ok(()),
            (WorkflowState::DeleteInProgress, WorkflowState::DeleteComplete) => Ok(()),

            // Verify
            (WorkflowState::DeleteComplete, WorkflowState::VerificationInProgress) => Ok(()),
            (WorkflowState::VerificationInProgress, WorkflowState::ReceiptReady) => Ok(()),
            (WorkflowState::ReceiptReady, WorkflowState::CleanupComplete) => Ok(()),

            // Error transitions (always allowed)
            (_, WorkflowState::AuditFailed) => Ok(()),
            (_, WorkflowState::PlanValidationFailed) => Ok(()),
            (_, WorkflowState::DeleteFailed) => Ok(()),
            (_, WorkflowState::CleanupFailed) => Ok(()),

            (current, next) => {
                Err(format!("Cannot transition from {} to {}", current.as_str(), next.as_str()))
            }
        }
    }

    /// Transition to new state
    pub fn transition(&mut self, next: WorkflowState) -> Result<(), String> {
        self.can_transition_to(next)?;
        self.state = next;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Get human-readable guidance for current state
    pub fn next_step_guidance(&self) -> String {
        match &self.state {
            WorkflowState::Unstarted | WorkflowState::AuditNeeded => {
                "No cleanup in progress. Call audit_scan() to start.".to_string()
            }
            WorkflowState::AuditInProgress => {
                "Audit in progress. Check back soon or poll query_workflow_state().".to_string()
            }
            WorkflowState::AuditComplete => {
                "Audit complete. Call plan_build() to generate a cleanup plan.".to_string()
            }
            WorkflowState::AuditFailed => {
                "Audit failed. Check error details and retry with audit_scan().".to_string()
            }
            WorkflowState::PlanNeeded | WorkflowState::PlanInProgress => {
                "Plan being built. Call query_workflow_state() to check progress.".to_string()
            }
            WorkflowState::PlanReady => {
                "Plan ready. Call plan_validate() to check safety, then plan_approve() to approve."
                    .to_string()
            }
            WorkflowState::PlanValidationFailed => {
                "Plan validation failed. Review issues and rebuild with plan_build().".to_string()
            }
            WorkflowState::PlanApproved => {
                "Plan approved. Call delete_dry_run() to preview, then delete_execute() to run."
                    .to_string()
            }
            WorkflowState::DeleteNeeded | WorkflowState::DeleteInProgress => {
                "Deletion in progress. Check query_workflow_state() for progress.".to_string()
            }
            WorkflowState::DeleteComplete => {
                "Deletion complete. Call receipt_verify() to validate results.".to_string()
            }
            WorkflowState::DeleteFailed => {
                "Deletion failed. Review error details and check receipt for partial results."
                    .to_string()
            }
            WorkflowState::ReceiptReady | WorkflowState::VerificationInProgress => {
                "Receipt being verified. Check query_workflow_state() for progress.".to_string()
            }
            WorkflowState::CleanupComplete => {
                "Cleanup complete and verified. Call clear_artifacts() to reset for next run."
                    .to_string()
            }
            WorkflowState::CleanupFailed => {
                "Cleanup failed. Review errors and consider plan_rollback() to restore.".to_string()
            }
        }
    }

    /// Record error and update state
    pub fn record_error(&mut self, error: String) {
        self.last_error = Some(error);
        self.updated_at = Utc::now();
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.last_error = None;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transitions() {
        let mut ctx = WorkflowContext::new(PathBuf::from("/tmp"));
        assert_eq!(ctx.state, WorkflowState::Unstarted);

        // Valid transition
        assert!(ctx.transition(WorkflowState::AuditNeeded).is_ok());
        assert_eq!(ctx.state, WorkflowState::AuditNeeded);

        // Invalid transition
        assert!(ctx.transition(WorkflowState::DeleteComplete).is_err());
    }

    #[test]
    fn test_error_transitions_allowed() {
        let ctx = WorkflowContext::new(PathBuf::from("/tmp"));
        // Error states should be reachable from any state
        assert!(ctx.can_transition_to(WorkflowState::AuditFailed).is_ok());
        assert!(ctx.can_transition_to(WorkflowState::DeleteFailed).is_ok());
    }

    #[test]
    fn test_guidance_updates_with_state() {
        let ctx = WorkflowContext::new(PathBuf::from("/tmp"));
        let guidance = ctx.next_step_guidance();
        assert!(guidance.contains("audit_scan"));
    }
}
