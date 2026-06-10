//! Plan-bound deletion rules and validation.

use crate::domain::artifact::is_macos_os_dir;
use crate::domain::plan::DeletionPlan;
use std::path::Path;

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
use wasm4pm_compat::evidence::Evidence;
use wasm4pm_compat::state::Raw;

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
