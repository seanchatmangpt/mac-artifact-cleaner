//! Regression tests for plan approval binding.
//!
//! Bug (pre-fix): `plan_approve`'s HMAC signature was purely a return value
//! to the caller -- nothing on disk recorded that a plan had been reviewed.
//! `oclnr delete execute --yes` read whatever was currently on disk at
//! `--plan` and deleted it unconditionally, including:
//!   - a hand-written plan that was never produced by `plan_build` /
//!     `plan_approve` at all, and
//!   - a plan that was approved, then hand-edited afterward (items appended,
//!     or an approved item's path substituted for a different, unreviewed
//!     directory).
//!
//! These tests use a state-parameterized fixture (`plan_in_state`) that can
//! construct any of the relevant plan states on demand -- unapproved,
//! approved-and-untampered, or approved-then-tampered -- rather than only
//! extending a single shared happy-path builder.

use std::path::PathBuf;

use osx_clnr::domain::{
    delete::require_plan_approved,
    plan::{DeletionPlan, PlanApproval, PlanItem, PlanItemKind},
};

/// The states a plan can be in with respect to approval, for parameterizing
/// the fixture below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanApprovalState {
    /// Never run through `plan_approve` (e.g. hand-written, or freshly built
    /// by `plan_build` but not yet approved).
    Unapproved,
    /// Approved, and unmodified since.
    ApprovedUntampered,
    /// Approved, then an item's path was substituted afterward -- the exact
    /// attack from the bug report (a directory injected/swapped in after
    /// signing, with the `approval` field left in place).
    ApprovedThenTampered,
    /// Approved, then a brand-new item was appended afterward -- the other
    /// attack from the bug report (an injected directory that was never
    /// scanned or approved, added to an otherwise-legitimate approved plan).
    ApprovedThenItemInjected,
}

/// Fixture: build a `DeletionPlan` already parked in an arbitrary approval
/// state. Parameterized so callers can construct the exact buggy state on
/// demand rather than only exercising a single shared happy-path builder.
fn plan_in_state(state: PlanApprovalState) -> DeletionPlan {
    let mut plan = DeletionPlan::new(
        vec![PathBuf::from("/tmp/plan-approval-test")],
        false,
        false,
        vec![PlanItem {
            path: PathBuf::from("/tmp/plan-approval-test/canary3"),
            kind: PlanItemKind::Dir,
            reason: "test fixture".to_string(),
            bytes: 0,
        }],
        vec![],
    );

    match state {
        PlanApprovalState::Unapproved => plan,
        PlanApprovalState::ApprovedUntampered => {
            let plan_hash = plan.content_hash();
            plan.approval = Some(PlanApproval {
                approver: "alice".to_string(),
                approval_reason: "scheduled cleanup".to_string(),
                approved_at_unix: 0,
                plan_hash,
            });
            plan
        }
        PlanApprovalState::ApprovedThenTampered => {
            let plan_hash = plan.content_hash();
            plan.approval = Some(PlanApproval {
                approver: "alice".to_string(),
                approval_reason: "scheduled cleanup".to_string(),
                approved_at_unix: 0,
                plan_hash,
            });
            // Substitute the approved item's path for a completely
            // different, never-reviewed directory -- mirrors the bug
            // report's "canary3 -> canary4" swap performed after approval.
            plan.items[0].path = PathBuf::from("/tmp/plan-approval-test/canary4-substituted");
            plan
        }
        PlanApprovalState::ApprovedThenItemInjected => {
            let plan_hash = plan.content_hash();
            plan.approval = Some(PlanApproval {
                approver: "alice".to_string(),
                approval_reason: "scheduled cleanup".to_string(),
                approved_at_unix: 0,
                plan_hash,
            });
            // Append a brand-new item that was never scanned or approved --
            // mirrors the bug report's hand-edited "inject canary3 as a new
            // item" attack.
            plan.items.push(PlanItem {
                path: PathBuf::from("/tmp/plan-approval-test/injected"),
                kind: PlanItemKind::Dir,
                reason: "injected after approval".to_string(),
                bytes: 0,
            });
            plan
        }
    }
}

#[test]
fn unapproved_plan_is_refused() {
    let plan = plan_in_state(PlanApprovalState::Unapproved);
    let result = require_plan_approved(&plan);
    assert!(result.is_err(), "a plan that was never approved must be refused");
}

#[test]
fn approved_untampered_plan_is_accepted() {
    let plan = plan_in_state(PlanApprovalState::ApprovedUntampered);
    let result = require_plan_approved(&plan);
    assert!(result.is_ok(), "a freshly approved, untampered plan must be accepted: {:?}", result);
}

/// The core regression: an item's path substituted after approval (the
/// "canary3 -> canary4" swap from the bug report) must be caught even
/// though the `approval` field is still present and internally
/// well-formed -- the recomputed content hash no longer matches it.
#[test]
fn plan_tampered_via_path_substitution_after_approval_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ApprovedThenTampered);
    let result = require_plan_approved(&plan);
    assert!(
        result.is_err(),
        "a plan whose approved item's path was substituted after signing must be refused"
    );
}

/// The other regression: a new item appended to an already-approved plan
/// (the "inject a directory that was never scanned or approved" attack)
/// must also be caught.
#[test]
fn plan_tampered_via_item_injection_after_approval_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ApprovedThenItemInjected);
    let result = require_plan_approved(&plan);
    assert!(result.is_err(), "a plan with an item injected after signing must be refused");
}

/// Sanity check that the fixture states are actually distinguishable from
/// each other by content hash -- otherwise the tampering tests above would
/// pass vacuously.
#[test]
fn fixture_states_have_distinct_content_hashes() {
    let untampered = plan_in_state(PlanApprovalState::ApprovedUntampered);
    let path_tampered = plan_in_state(PlanApprovalState::ApprovedThenTampered);
    let item_injected = plan_in_state(PlanApprovalState::ApprovedThenItemInjected);

    assert_ne!(untampered.content_hash(), path_tampered.content_hash());
    assert_ne!(untampered.content_hash(), item_injected.content_hash());
    assert_ne!(path_tampered.content_hash(), item_injected.content_hash());
}
