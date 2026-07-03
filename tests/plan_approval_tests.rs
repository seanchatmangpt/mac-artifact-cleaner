//! Regression tests for plan approval binding.
//!
//! Bug (pre-fix, round 1): `plan_approve`'s HMAC signature was purely a
//! return value to the caller -- nothing on disk recorded that a plan had
//! been reviewed. `oclnr delete execute --yes` read whatever was currently
//! on disk at `--plan` and deleted it unconditionally, including:
//!   - a hand-written plan that was never produced by `plan_build` /
//!     `plan_approve` at all, and
//!   - a plan that was approved, then hand-edited afterward (items appended,
//!     or an approved item's path substituted for a different, unreviewed
//!     directory).
//!
//! Bug (pre-fix, round 2 -- this file's main addition): the on-disk
//! `PlanApproval.plan_hash` gate added to close round 1 was an **unkeyed**
//! BLAKE3 content hash with no secret. A verifier proved this forgeable:
//! without ever calling `plan_approve`, they hand-computed the same BLAKE3
//! hash offline (e.g. in Python) and wrote a self-forged approval block into
//! the plan file, and `oclnr delete execute --yes` accepted it. Separately,
//! `plan_approve` computed a real HMAC-SHA256 (`ApprovalMetadata::sign`) but
//! signed it with a hardcoded literal key (`b"secret"`, publicly visible in
//! source) and never actually checked that signature at delete time.
//!
//! The fix: `require_plan_approved` / `DeletionPlan::verify_approval` now
//! take a `secret: &[u8]` and verify a real HMAC-SHA256 signature
//! (`PlanApproval::hmac_signature`) over the plan's content hash, keyed with
//! a secret the caller sources from an environment variable or a
//! machine-local key file (`integration::config::approval_secret`) -- never
//! from the plan file itself. The plain `plan_hash` field is retained only
//! as a diagnostic; it is no longer, by itself, a security boundary.
//!
//! These tests use a state-parameterized fixture (`plan_in_state`) that can
//! construct any of the relevant plan states on demand -- unapproved,
//! properly signed, forged, or approved-then-tampered -- rather than only
//! extending a single shared happy-path builder.

use std::path::PathBuf;

use osx_clnr::domain::{
    delete::require_plan_approved,
    plan::{DeletionPlan, PlanApproval, PlanItem, PlanItemKind},
};

/// The real secret, standing in for whatever `integration::config::approval_secret`
/// would source from `OCLNR_APPROVAL_SECRET` or `~/.oclnr/approval.key` on a
/// legitimate machine.
const REAL_SECRET: &[u8] = b"the-genuine-approval-secret-nobody-else-has";

/// A secret an attacker might guess, or the empty/placeholder key a weaker
/// implementation might fall back to. Must never verify against a signature
/// produced with `REAL_SECRET`.
const WRONG_SECRET: &[u8] = b"secret";

/// The states a plan can be in with respect to approval, for parameterizing
/// the fixture below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanApprovalState {
    /// Never run through `plan_approve` (e.g. hand-written, or freshly built
    /// by `plan_build` but not yet approved).
    Unapproved,
    /// Legitimately signed via `plan_approve`'s real path: `DeletionPlan::sign_approval`
    /// with the real secret, and unmodified since.
    ApprovedUntampered,
    /// Approved, then an item's path was substituted afterward -- the exact
    /// attack from the round-1 bug report (a directory injected/swapped in
    /// after signing, with the `approval` field left in place).
    ApprovedThenTampered,
    /// Approved, then a brand-new item was appended afterward -- the other
    /// round-1 attack (an injected directory that was never scanned or
    /// approved, added to an otherwise-legitimate approved plan).
    ApprovedThenItemInjected,
    /// The round-2 attack: never signed by `plan_approve` at all. An
    /// attacker who can only write the plan file hand-computes the plain,
    /// unkeyed content hash offline (exactly what the verifier demonstrated)
    /// and writes it in as `plan_hash`, alongside an empty or guessed
    /// `hmac_signature`.
    ForgedPlainHashNoHmac,
    /// A variant of the forgery where the attacker also guesses a
    /// plausible-looking (but wrong) HMAC key -- e.g. the literal `b"secret"`
    /// that was hardcoded in the pre-fix source -- and signs with that
    /// instead of leaving `hmac_signature` empty.
    ForgedWithGuessedKey,
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
            plan.approval = Some(plan.sign_approval(REAL_SECRET, "alice", "scheduled cleanup"));
            plan
        }
        PlanApprovalState::ApprovedThenTampered => {
            plan.approval = Some(plan.sign_approval(REAL_SECRET, "alice", "scheduled cleanup"));
            // Substitute the approved item's path for a completely
            // different, never-reviewed directory -- mirrors the bug
            // report's "canary3 -> canary4" swap performed after approval.
            plan.items[0].path = PathBuf::from("/tmp/plan-approval-test/canary4-substituted");
            plan
        }
        PlanApprovalState::ApprovedThenItemInjected => {
            plan.approval = Some(plan.sign_approval(REAL_SECRET, "alice", "scheduled cleanup"));
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
        PlanApprovalState::ForgedPlainHashNoHmac => {
            // Exactly the verifier's demonstrated attack: never call
            // `plan_approve` / `sign_approval` at all. Hand-compute the
            // plain content hash (this is public information -- BLAKE3, no
            // secret, reproducible by anyone who can read the plan file) and
            // write it in as `plan_hash`, with no real HMAC.
            let forged_plain_hash = plan.content_hash();
            plan.approval = Some(PlanApproval {
                approver: "attacker".to_string(),
                approval_reason: "self-approved, never reviewed".to_string(),
                approved_at_unix: 0,
                plan_hash: forged_plain_hash,
                hmac_signature: String::new(),
            });
            plan
        }
        PlanApprovalState::ForgedWithGuessedKey => {
            // Same forgery, but the attacker also signs with a guessed key
            // (the pre-fix hardcoded literal `b"secret"`) rather than
            // leaving the HMAC field empty.
            plan.approval = Some(plan.sign_approval(WRONG_SECRET, "attacker", "self-approved"));
            plan
        }
    }
}

#[test]
fn unapproved_plan_is_refused() {
    let plan = plan_in_state(PlanApprovalState::Unapproved);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(result.is_err(), "a plan that was never approved must be refused");
}

#[test]
fn approved_untampered_plan_is_accepted() {
    let plan = plan_in_state(PlanApprovalState::ApprovedUntampered);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(result.is_ok(), "a freshly approved, untampered plan must be accepted: {:?}", result);
}

/// The core round-1 regression: an item's path substituted after approval
/// (the "canary3 -> canary4" swap from the bug report) must be caught even
/// though the `approval` field is still present and internally
/// well-formed -- the recomputed content hash no longer matches it.
#[test]
fn plan_tampered_via_path_substitution_after_approval_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ApprovedThenTampered);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(
        result.is_err(),
        "a plan whose approved item's path was substituted after signing must be refused"
    );
}

/// The other round-1 regression: a new item appended to an already-approved
/// plan (the "inject a directory that was never scanned or approved" attack)
/// must also be caught.
#[test]
fn plan_tampered_via_item_injection_after_approval_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ApprovedThenItemInjected);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(result.is_err(), "a plan with an item injected after signing must be refused");
}

/// The round-2 regression, and the exact attack the verifier demonstrated:
/// hand-forging an approval block by computing the plain (unkeyed) content
/// hash offline, without ever calling `plan_approve`, must now be rejected.
#[test]
fn plan_forged_with_plain_hash_and_no_hmac_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ForgedPlainHashNoHmac);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(
        result.is_err(),
        "a plan with a hand-forged plain-hash approval and no valid HMAC must be refused \
         (this is the attack the verifier proved -- it must now fail)"
    );
}

/// A forgery that also guesses a plausible key (the old hardcoded literal
/// `b"secret"`) and signs with that must still be rejected, because the real
/// secret used to verify differs from the guessed key.
#[test]
fn plan_forged_with_guessed_key_is_refused() {
    let plan = plan_in_state(PlanApprovalState::ForgedWithGuessedKey);
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(
        result.is_err(),
        "a plan signed with a guessed/wrong secret must be refused when verified against the \
         real secret"
    );
}

/// Even a legitimately-produced signature must be refused if `delete
/// execute` is (mis)configured to verify against the wrong secret -- e.g. a
/// different machine's key file, or a typo'd environment variable. This
/// guards against a fix that checks *a* signature without checking it's
/// signed with *the* secret this machine trusts.
#[test]
fn legitimate_signature_is_refused_when_verified_with_wrong_secret() {
    let plan = plan_in_state(PlanApprovalState::ApprovedUntampered);
    let result = require_plan_approved(&plan, WRONG_SECRET);
    assert!(
        result.is_err(),
        "a genuinely-signed plan must still be refused if verified against a different secret \
         than the one it was signed with"
    );
}

/// End-to-end: the legitimate `plan_approve` -> `delete_execute` flow (sign
/// with the real secret, verify with the real secret) must keep working.
#[test]
fn legitimate_approve_then_delete_flow_still_works() {
    let mut plan = plan_in_state(PlanApprovalState::Unapproved);
    assert!(require_plan_approved(&plan, REAL_SECRET).is_err(), "sanity: starts unapproved");

    // Simulate `plan_approve`: sign with the real secret.
    plan.approval = Some(plan.sign_approval(REAL_SECRET, "alice", "scheduled cleanup"));

    // Simulate `delete execute --yes`: verify with the same real secret.
    let result = require_plan_approved(&plan, REAL_SECRET);
    assert!(result.is_ok(), "legitimate approve-then-delete flow must succeed: {:?}", result);
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
