//! Design for Combinatorial Maximum (DCM) — reversibility classification.
//!
//! This module adopts one bounded, testable slice of the DCM architectural
//! doctrine (`~/gymact/docs/combinatorial-maximum.md`, written for a
//! different project) into osx-clnr's existing plan/delete/receipt
//! pipeline, rather than a literal transplant of DCM's full machinery
//! (public RDF/SHACL ontology, a BRCE broker, compiled cognition routes —
//! none of which have an osx-clnr analog and are explicitly out of scope
//! here).
//!
//! The correspondence adopted:
//!
//! - **SELECT / CONSTRUCT are powerless; DO is consequential** (DCM §8) —
//!   already osx-clnr's core invariant ("the scanner cannot delete; the
//!   deleter cannot scan"). `plan(build)`/`plan(inspect)`/`plan(validate)`
//!   are SELECT/CONSTRUCT; `delete(execute)` is the sole DO edge.
//! - **Reversibility is admission, not optimism** (DCM §3) — every
//!   [`PlanItem`](crate::domain::plan::PlanItem) now carries an explicit
//!   [`Reversibility`] classification. `UNKNOWN != REVERSIBLE`: an
//!   unclassifiable item is a fence, never silently treated as safe.
//! - **Irreversible frontier / explicit cut** (DCM §9) — the plan-approval
//!   HMAC (`DeletionPlan::sign_approval`) already binds an exact content
//!   hash of the plan; because `reversibility` is a field on `PlanItem`,
//!   it is now part of that bound content, so an approval cannot be
//!   silently re-used against a plan whose reversibility classification
//!   changed underneath it.
//! - **Silent pruning is forbidden** (DCM §5) — [`classify_reversibility`]
//!   never drops an item from consideration; callers are expected to
//!   surface `Irreversible`/`Unknown` items as visible plan entries (see
//!   `nouns::plan`), not exclude them without evidence.
//! - **Failed edge is topology** (DCM §4) — a `Reversibility::Irreversible`
//!   or `Unknown` item does not falsify the plan; it is retained with its
//!   typed disposition for the operator to review at `plan(inspect)`.
//!
//! What is deliberately *not* adopted: the public RDF/PROV-O/SHACL
//! ontology layer, the `CombinatorialBrokerRequest`/BRCE execution broker,
//! and "compiled cognition routes" — osx-clnr has no possibility-graph
//! runtime, no RDF admission layer, and no cached-route replay system, and
//! grafting those in would not be a real correspondence, just vocabulary.

use serde::{Deserialize, Serialize};

use crate::domain::plan::PlanItemKind;

/// A plan item's classified reversibility, per DCM §3 ("Reversibility is
/// admission, not optimism").
///
/// `COMPENSATABLE != REVERSIBLE`, `UNKNOWN != REVERSIBLE`,
/// `IRREVERSIBLE != REVERSIBLE` — each variant is distinct and the default
/// (`Unknown`) is a fence, never treated as safe-to-delete.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    /// Deleting this item is mechanically recoverable at negligible cost:
    /// the artifact is regenerable in place (a build/cache directory that
    /// the owning tool will recreate on next invocation).
    Reversible,
    /// Deleting this item is recoverable but not free: the data can be
    /// restored from an external, durable source (e.g. re-cloning a GitHub
    /// repository, re-fetching a release asset) at the cost of a network
    /// round trip and possibly losing local-only state (uncommitted work,
    /// draft comments).
    Compensatable,
    /// Not mechanically classifiable from the evidence available at
    /// plan-build time (kind + artifact-detection reason string alone).
    /// This is the fence: an unclassifiable item must never be treated as
    /// equivalent to `Reversible`.
    #[default]
    Unknown,
    /// Known to be non-recoverable by any means encoded in this
    /// classifier (no regeneration path, no external durable copy).
    Irreversible,
}

impl Reversibility {
    /// Human-readable label, used in `plan(inspect)` output and the
    /// deletion receipt so operators see the classification, not just the
    /// serialized enum tag.
    pub fn label(self) -> &'static str {
        match self {
            Reversibility::Reversible => "reversible",
            Reversibility::Compensatable => "compensatable",
            Reversibility::Unknown => "unknown",
            Reversibility::Irreversible => "irreversible",
        }
    }
}

/// Classifies a plan item's reversibility from its kind and the
/// artifact-detection reason string already computed at plan-build time.
///
/// This is a coarse, evidence-labeled heuristic over data already present
/// on the candidate — not a formal proof of recoverability, and it does
/// not perform any filesystem or process I/O (domain purity: zero
/// `std::fs`/`std::process`). It is deliberately conservative: any
/// `reason` string it doesn't recognize as a known-regenerable pattern
/// classifies as `Unknown`, never `Reversible` — per DCM §3 the fence
/// direction is mandatory ("unknown reversibility is a fence").
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::dcm::{classify_reversibility, Reversibility};
/// use osx_clnr::domain::plan::PlanItemKind;
///
/// // Positive: recognized regenerable-cache reasons classify Reversible.
/// assert_eq!(
///     classify_reversibility(PlanItemKind::Dir, "rust target"),
///     Reversibility::Reversible
/// );
/// assert_eq!(
///     classify_reversibility(PlanItemKind::Dir, "node_modules"),
///     Reversibility::Reversible
/// );
///
/// // Positive: GitHub-kind items (recoverable via network re-fetch, but
/// // not free/instant) classify Compensatable, not Reversible.
/// assert_eq!(
///     classify_reversibility(PlanItemKind::GithubRepo, "stale fork"),
///     Reversibility::Compensatable
/// );
///
/// // Negative (the fence): an unrecognized reason must NOT default to
/// // Reversible just because it's a Dir — that would falsify DCM §3.
/// assert_eq!(
///     classify_reversibility(PlanItemKind::Dir, "unrecognized artifact type"),
///     Reversibility::Unknown
/// );
///
/// // Negative: a bare file with no regenerable-cache signal in its reason
/// // is Unknown, not Reversible — deleting a file is not obviously
/// // cost-free just because the plan builder proposed it.
/// assert_eq!(
///     classify_reversibility(PlanItemKind::File, "large file"),
///     Reversibility::Unknown
/// );
/// ```
pub fn classify_reversibility(kind: PlanItemKind, reason: &str) -> Reversibility {
    match kind {
        PlanItemKind::GithubRepo
        | PlanItemKind::GithubBranch
        | PlanItemKind::GithubRun
        | PlanItemKind::GithubRelease
        | PlanItemKind::GithubCache
        | PlanItemKind::GithubIssue
        | PlanItemKind::GithubPr
        | PlanItemKind::GithubReleaseAsset => Reversibility::Compensatable,
        PlanItemKind::File | PlanItemKind::Dir => {
            let r = reason.to_ascii_lowercase();
            const REGENERABLE_SIGNALS: &[&str] = &[
                "target",
                "node_modules",
                "cache",
                "build",
                "venv",
                "__pycache__",
                "dist",
                ".gradle",
                "pod",
                "vendor/bundle",
                "site-packages",
            ];
            if REGENERABLE_SIGNALS.iter().any(|sig| r.contains(sig)) {
                Reversibility::Reversible
            } else {
                Reversibility::Unknown
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Falsifier (DCM §15.2): an `Unknown`/`Compensatable`/`Irreversible`
    /// item must never compare equal to `Reversible` — if this test ever
    /// fails to compile or fails at runtime because someone collapsed the
    /// enum to a boolean "safe/unsafe", the DCM correspondence is broken.
    #[test]
    fn reversibility_variants_are_pairwise_distinct() {
        let variants = [
            Reversibility::Reversible,
            Reversibility::Compensatable,
            Reversibility::Unknown,
            Reversibility::Irreversible,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                assert_eq!(i == j, a == b, "variant {a:?} vs {b:?} equality mismatch");
            }
        }
    }

    /// Falsifier: the default (used when deserializing plans written
    /// before this field existed) must be `Unknown`, never `Reversible` —
    /// an old plan file re-loaded must not silently gain a "safe to
    /// delete" classification it was never actually given.
    #[test]
    fn default_reversibility_is_unknown_not_reversible() {
        assert_eq!(Reversibility::default(), Reversibility::Unknown);
    }

    #[test]
    fn dotfile_style_reason_is_unknown_not_reversible() {
        assert_eq!(
            classify_reversibility(PlanItemKind::File, ".env backup"),
            Reversibility::Unknown
        );
    }
}
