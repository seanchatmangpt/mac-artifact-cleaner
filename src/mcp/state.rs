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
    /// Roots the most recent audit_scan was actually run against. plan_build
    /// must inherit these when the caller does not pass explicit roots, so a
    /// plan can never silently diverge in scope from the audit it was built
    /// from (e.g. audit scoped to a test dir, plan falling back to broad
    /// defaults like /Users/<user> and /tmp).
    pub audit_roots: Option<Vec<PathBuf>>,
    /// Recency window (hours) the most recent audit_scan was actually run
    /// with (0 means the recency guard was disabled). plan_build inherits
    /// this when the caller does not pass an explicit override, so a plan's
    /// recency decision never silently diverges from the audit it was built
    /// from.
    pub audit_ignore_recent_hours: Option<u32>,
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
            audit_roots: None,
            audit_ignore_recent_hours: None,
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
            // A failed delete should not be a dead end: allow re-scanning
            // directly instead of forcing clear_artifacts as an undocumented
            // mandatory recovery step.
            (WorkflowState::DeleteFailed, WorkflowState::AuditNeeded) => Ok(()),
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

    /// All 18 WorkflowState variants, in declaration order.
    const ALL_STATES: [WorkflowState; 18] = [
        WorkflowState::Unstarted,
        WorkflowState::AuditNeeded,
        WorkflowState::AuditInProgress,
        WorkflowState::AuditComplete,
        WorkflowState::AuditFailed,
        WorkflowState::PlanNeeded,
        WorkflowState::PlanInProgress,
        WorkflowState::PlanReady,
        WorkflowState::PlanValidationFailed,
        WorkflowState::PlanApproved,
        WorkflowState::DeleteNeeded,
        WorkflowState::DeleteInProgress,
        WorkflowState::DeleteComplete,
        WorkflowState::DeleteFailed,
        WorkflowState::ReceiptReady,
        WorkflowState::VerificationInProgress,
        WorkflowState::CleanupComplete,
        WorkflowState::CleanupFailed,
    ];

    fn ctx_in_state(state: WorkflowState) -> WorkflowContext {
        let mut ctx = WorkflowContext::new(PathBuf::from("/tmp"));
        ctx.state = state;
        ctx
    }

    /// Exhaustive grid test: every (current, next) pair in the 18x18 state
    /// space is checked against an explicit expected-valid-pairs set mirroring
    /// `can_transition_to`'s match arms. Any accidental addition, removal, or
    /// widening (e.g. an error-catch-all shadowing a specific arm) of a legal
    /// transition changes this test's outcome. This is what would have caught
    /// the AUDIT_COMPLETE -> PLAN_IN_PROGRESS bug (skipping PLAN_NEEDED).
    #[test]
    fn test_exhaustive_transition_grid() {
        use WorkflowState::*;

        // Non-error "happy path" and retry transitions explicitly allowed by
        // can_transition_to, mirrored 1:1 from its match arms above the
        // catch-all error arms.
        let explicit_valid: &[(WorkflowState, WorkflowState)] = &[
            (Unstarted, AuditNeeded),
            (CleanupComplete, AuditNeeded),
            (DeleteFailed, AuditNeeded),
            (AuditNeeded, AuditInProgress),
            (AuditInProgress, AuditComplete),
            (AuditComplete, PlanNeeded),
            (PlanNeeded, PlanInProgress),
            (PlanInProgress, PlanReady),
            (PlanReady, PlanInProgress),
            (PlanReady, PlanApproved),
            (PlanValidationFailed, PlanInProgress),
            (PlanApproved, DeleteNeeded),
            (DeleteNeeded, DeleteInProgress),
            (DeleteInProgress, DeleteComplete),
            (DeleteComplete, VerificationInProgress),
            (VerificationInProgress, ReceiptReady),
            (ReceiptReady, CleanupComplete),
        ];

        // Error/failure states are reachable from *any* current state
        // (catch-all `(_, X) => Ok(())` arms).
        let error_targets: &[WorkflowState] =
            &[AuditFailed, PlanValidationFailed, DeleteFailed, CleanupFailed];

        for &current in ALL_STATES.iter() {
            for &next in ALL_STATES.iter() {
                let ctx = ctx_in_state(current);
                let actual = ctx.can_transition_to(next);

                let expected_ok =
                    explicit_valid.contains(&(current, next)) || error_targets.contains(&next);

                assert_eq!(
                    actual.is_ok(),
                    expected_ok,
                    "transition {} -> {}: expected {}, got {:?}",
                    current.as_str(),
                    next.as_str(),
                    if expected_ok { "Ok" } else { "Err" },
                    actual
                );
            }
        }
    }

    /// Regression test for the specific bug found this session: a handler
    /// tried to transition AUDIT_COMPLETE directly to PLAN_IN_PROGRESS,
    /// skipping the required PLAN_NEEDED intermediate step.
    #[test]
    fn test_known_invalid_skips_rejected() {
        use WorkflowState::*;

        let invalid_skips: &[(WorkflowState, WorkflowState)] = &[
            // The exact bug: AUDIT_COMPLETE -> PLAN_IN_PROGRESS skips PLAN_NEEDED.
            (AuditComplete, PlanInProgress),
            // Skipping straight from AuditComplete to later plan/delete stages.
            (AuditComplete, PlanReady),
            (AuditComplete, PlanApproved),
            (AuditComplete, DeleteNeeded),
            // Can't approve a plan that hasn't been readied.
            (PlanNeeded, PlanApproved),
            (PlanInProgress, PlanApproved),
            // Can't delete without an approved plan.
            (PlanReady, DeleteNeeded),
            (PlanApproved, DeleteInProgress),
            // Can't jump straight to verification/receipt/cleanup.
            (DeleteNeeded, DeleteComplete),
            (DeleteComplete, ReceiptReady),
            (DeleteComplete, CleanupComplete),
            (VerificationInProgress, CleanupComplete),
            // Unstarted cannot skip straight into the middle of the pipeline.
            (Unstarted, PlanNeeded),
            (Unstarted, DeleteNeeded),
            (Unstarted, CleanupComplete),
        ];

        for &(current, next) in invalid_skips {
            let ctx = ctx_in_state(current);
            assert!(
                ctx.can_transition_to(next).is_err(),
                "expected {} -> {} to be rejected, but it was allowed",
                current.as_str(),
                next.as_str()
            );
        }
    }

    /// Every state must be able to reach every error/failure variant
    /// (the catch-all `(_, ErrorState) => Ok(())` arms).
    #[test]
    fn test_every_state_can_reach_every_error_variant() {
        use WorkflowState::*;

        let error_targets: [WorkflowState; 4] =
            [AuditFailed, PlanValidationFailed, DeleteFailed, CleanupFailed];

        for &current in ALL_STATES.iter() {
            for &err in error_targets.iter() {
                let ctx = ctx_in_state(current);
                assert!(
                    ctx.can_transition_to(err).is_ok(),
                    "expected {} -> {} (error transition) to be allowed",
                    current.as_str(),
                    err.as_str()
                );
            }
        }
    }

    /// Walks the full documented happy-path sequence end-to-end, including
    /// the loop back from CleanupComplete to AuditNeeded, asserting every
    /// step succeeds via the real `transition` method (not just
    /// `can_transition_to`).
    #[test]
    fn test_full_happy_path_sequence() {
        use WorkflowState::*;

        let sequence: &[WorkflowState] = &[
            Unstarted,
            AuditNeeded,
            AuditInProgress,
            AuditComplete,
            PlanNeeded,
            PlanInProgress,
            PlanReady,
            PlanApproved,
            DeleteNeeded,
            DeleteInProgress,
            DeleteComplete,
            VerificationInProgress,
            ReceiptReady,
            CleanupComplete,
            // Loop back for the next cleanup cycle.
            AuditNeeded,
        ];

        let mut ctx = WorkflowContext::new(PathBuf::from("/tmp"));
        assert_eq!(ctx.state, Unstarted);

        for &next in sequence.iter().skip(1) {
            let from = ctx.state;
            assert!(
                ctx.transition(next).is_ok(),
                "expected happy-path transition {} -> {} to succeed",
                from.as_str(),
                next.as_str()
            );
            assert_eq!(ctx.state, next);
        }
    }
}
