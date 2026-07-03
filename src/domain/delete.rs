//! Plan-bound deletion rules and validation.

use std::path::Path;

use crate::domain::{artifact::is_macos_os_dir, plan::DeletionPlan};

/// Validates whether a single plan item path is present in the plan and passes safety checks.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::delete::validate_plan_item;
/// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
/// use std::path::{Path, PathBuf};
///
/// let plan = DeletionPlan::new(
///     vec![PathBuf::from("/Users/user")],
///     false,
///     true,
///     vec![PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/target"),
///         kind: PlanItemKind::Dir,
///         reason: "rust target".to_string(),
///         bytes: 0,
///     }],
///     vec![],
/// );
///
/// // Positive case: path is present in plan and safe.
/// assert!(validate_plan_item(Path::new("/Users/user/dev/project/target"), &plan));
///
/// // Negative case: path is safe but not present in plan.
/// assert!(!validate_plan_item(Path::new("/Users/user/dev/project/src"), &plan));
///
/// // Refusal case: system paths are always rejected even if present in the plan.
/// let bad_plan = DeletionPlan::new(
///     vec![PathBuf::from("/")],
///     false,
///     true,
///     vec![PlanItem {
///         path: PathBuf::from("/System"),
///         kind: PlanItemKind::Dir,
///         reason: "system directory".to_string(),
///         bytes: 0,
///     }],
///     vec![],
/// );
/// assert!(!validate_plan_item(Path::new("/System"), &bad_plan));
/// ```
pub fn validate_plan_item(item_path: &Path, plan: &DeletionPlan) -> bool {
    if is_macos_os_dir(item_path) {
        return false;
    }
    plan.items.iter().any(|item| item.path == item_path)
}

/// Validates the structure and safety of the entire deletion plan.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::delete::{DeletionPlanAdjudicator, PlanSafetyWitness};
/// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
/// use std::path::PathBuf;
/// use wasm4pm_compat::admission::Admit;
/// use wasm4pm_compat::evidence::Evidence;
/// use wasm4pm_compat::state::Raw;
///
/// let plan = DeletionPlan::new(
///     vec![PathBuf::from("/Users/user")],
///     false,
///     true,
///     vec![PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/target"),
///         kind: PlanItemKind::Dir,
///         reason: "rust target".to_string(),
///         bytes: 0,
///     }],
///     vec![],
/// );
///
/// // Positive case: plan is version 1 and has no system directory violations.
/// assert!(DeletionPlanAdjudicator::admit(Evidence::<_, Raw, PlanSafetyWitness>::raw(plan.clone())).is_ok());
///
/// // Refusal case 1: system directory violation.
/// let bad_item_plan = DeletionPlan::new(
///     vec![PathBuf::from("/")],
///     false,
///     true,
///     vec![PlanItem {
///         path: PathBuf::from("/System"),
///         kind: PlanItemKind::Dir,
///         reason: "system directory".to_string(),
///         bytes: 0,
///     }],
///     vec![],
/// );
/// assert!(DeletionPlanAdjudicator::admit(Evidence::<_, Raw, PlanSafetyWitness>::raw(bad_item_plan)).is_err());
///
/// // Refusal case 2: unsupported plan version.
/// let mut bad_version_plan = plan.clone();
/// bad_version_plan.version = 2;
/// assert!(DeletionPlanAdjudicator::admit(Evidence::<_, Raw, PlanSafetyWitness>::raw(bad_version_plan)).is_err());
/// ```
use wasm4pm_compat::admission::{Admission, Admit, Refusal};
use wasm4pm_compat::{evidence::Evidence, state::Raw};

/// The witness for verifying a deletion plan against macOS safety rules and scope constraints.
pub struct PlanSafetyWitness;

pub struct DeletionPlanAdjudicator;

impl Admit for DeletionPlanAdjudicator {
    type Raw = DeletionPlan;
    type Admitted = DeletionPlan;
    type Reason = String;
    type Witness = PlanSafetyWitness;

    fn admit(
        raw: Evidence<Self::Raw, Raw, Self::Witness>,
    ) -> Result<Admission<Self::Admitted, Self::Witness>, Refusal<Self::Reason, Self::Witness>>
    {
        let plan = &raw.value;
        let mut errors = Vec::new();

        if plan.version != 1 {
            errors.push(format!("Unsupported plan version: {}", plan.version));
        }

        for item in &plan.items {
            if is_macos_os_dir(&item.path) {
                errors.push(format!(
                    "Safety violation: system path in deletion plan: {}",
                    item.path.display()
                ));
            }
        }

        if errors.is_empty() {
            Ok(Admission::new(raw.value.clone()))
        } else {
            Err(Refusal::new(errors.join("; ")))
        }
    }
}

/// Requires that `plan` carries a valid, untampered [`PlanApproval`] before
/// destructive execution proceeds.
///
/// This is deliberately **not** folded into [`DeletionPlanAdjudicator`]
/// (which is shared with the GitHub-resource deletion path, an unrelated
/// workflow that has no approval step of its own). `oclnr delete execute`
/// calls this in addition to the adjudicator so that:
///
/// - a plan file that was never run through `plan_approve` is refused, and
/// - a plan that was hand-edited (items appended, paths substituted, etc.)
///   *after* `plan_approve` signed it is refused, even though the file is
///   otherwise well-formed JSON with a valid `version` field.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::delete::require_plan_approved;
/// use osx_clnr::domain::plan::{DeletionPlan, PlanApproval, PlanItem, PlanItemKind};
/// use std::path::PathBuf;
///
/// let mut plan = DeletionPlan::new(
///     vec![PathBuf::from("/Users/user")],
///     false,
///     true,
///     vec![PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/target"),
///         kind: PlanItemKind::Dir,
///         reason: "rust target".to_string(),
///         bytes: 0,
///     }],
///     vec![],
/// );
///
/// // Refusal: a hand-written or never-approved plan is rejected outright.
/// assert!(require_plan_approved(&plan).is_err());
///
/// let plan_hash = plan.content_hash();
/// plan.approval = Some(PlanApproval {
///     approver: "alice".to_string(),
///     approval_reason: "cleanup".to_string(),
///     approved_at_unix: 0,
///     plan_hash,
/// });
///
/// // Positive: freshly approved, untampered plan is accepted.
/// assert!(require_plan_approved(&plan).is_ok());
///
/// // Refusal: an item substituted into the plan after signing (the exact
/// // attack from the bug report — a directory injected post-approval) is
/// // caught even though `approval` is still present and well-formed.
/// let mut tampered = plan.clone();
/// tampered.items[0].path = PathBuf::from("/Users/user/injected-after-approval");
/// assert!(require_plan_approved(&tampered).is_err());
/// ```
pub fn require_plan_approved(plan: &DeletionPlan) -> Result<(), String> {
    plan.verify_approval()
}
