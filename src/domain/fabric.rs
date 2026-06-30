//! Fabric integration mapping for `osx-clnr` plans and receipts.
//!
//! This module converts deletion plans and execution receipts into the mathematical
//! `Fabric` category graph representation from `cfab-surface`.
//!
//! # Examples
//!
//! ```
//! use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
//! use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
//! use osx_clnr::domain::fabric::build_fabric;
//! use std::path::PathBuf;
//!
//! let plan = DeletionPlan::new(
//!     vec![PathBuf::from("/Users/sac/osx-clnr")],
//!     false,
//!     true,
//!     vec![PlanItem {
//!         path: PathBuf::from("/Users/sac/osx-clnr/target"),
//!         kind: PlanItemKind::Dir,
//!         reason: "rebuildable cargo target".to_string(),
//!         bytes: 200,
//!     }],
//!     vec![],
//! );
//!
//! // Build a fabric with plan only
//! let fabric = build_fabric(&plan, None, "/Users/sac/osx-clnr/plan.json", "/Users/sac/osx-clnr/receipt.json").unwrap();
//! assert_eq!(fabric.len(), 3); // 1 plan node, 1 root node, 1 item node
//!
//! // Build a fabric with plan and receipt
//! let receipt = DeletionReceipt::new(
//!     "chain-id-123".to_string(),
//!     plan.created_unix,
//!     plan.created_unix + 10,
//!     plan.created_unix + 20,
//!     vec![DeletionResult {
//!         path: PathBuf::from("/Users/sac/osx-clnr/target"),
//!         status: DeletionStatus::Deleted,
//!         error: None,
//!         blake3_hash: None,
//!         bytes_freed: 200,
//!     }],
//!     Some(1_000_000_000),
//!     Some(1_000_000_000),
//! );
//!
//! let fabric_with_receipt = build_fabric(
//!     &plan,
//!     Some(&receipt),
//!     "/Users/sac/osx-clnr/plan.json",
//!     "/Users/sac/osx-clnr/receipt.json",
//! ).unwrap();
//! assert_eq!(fabric_with_receipt.len(), 4); // 1 plan node, 1 root node, 1 item node, 1 receipt node
//! ```

use crate::domain::plan::DeletionPlan;
use crate::domain::receipt::DeletionReceipt;
use cfab_surface::{Fabric, FabricError, Relation, RelationKind, Surface};
use url::Url;

/// Maps a `DeletionPlan` and optional `DeletionReceipt` into a `cfab_surface::Fabric` graph.
///
/// # Errors
///
/// Returns a `FabricError` if any URL parsing, connection rule, or acyclicity check fails.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::plan::DeletionPlan;
/// use osx_clnr::domain::fabric::build_fabric;
///
/// let plan = DeletionPlan::new(vec![], false, false, vec![], vec![]);
/// let fabric = build_fabric(&plan, None, "/tmp/plan.json", "/tmp/receipt.json").unwrap();
/// assert_eq!(fabric.len(), 1); // Only the plan node
/// ```
pub fn build_fabric(
    plan: &DeletionPlan,
    receipt: Option<&DeletionReceipt>,
    plan_path: &str,
    receipt_path: &str,
) -> Result<Fabric, FabricError> {
    let mut fabric = Fabric::new();

    // 1. Parse plan URL and create plan surface
    let plan_url = Url::parse(&format!("plan:///{}", plan_path.trim_start_matches('/')))
        .map_err(|e| FabricError::InvalidUrl(e.to_string()))?;
    let plan_id = plan_url.to_string();
    let plan_surface = Surface::from_url(plan_id.clone(), plan_url, "Deletion Plan".to_string())?;
    fabric.add_surface(plan_surface)?;

    // 2. Add roots as LocalDirectory surfaces
    for root in &plan.roots {
        let root_url = Url::from_file_path(root)
            .map_err(|_| FabricError::InvalidUrl(format!("Invalid root path: {:?}", root)))?;
        let root_id = root_url.to_string();
        let root_surface = Surface::from_url(
            root_id.clone(),
            root_url,
            root.to_string_lossy().into_owned(),
        )?;
        fabric.add_surface(root_surface)?;

        // Connect root_surface -> plan_surface via Transformation
        let relation = Relation::new(
            RelationKind::Transformation {
                mapping_id: "audit-scan-root".to_string(),
            },
            1.0,
        );
        fabric.connect(&root_id, &plan_id, relation)?;
    }

    // 3. Process plan items (candidates)
    for item in &plan.items {
        let item_url = path_to_url(&item.path)?;
        let item_id = item_url.to_string();
        let item_name = item.path.to_string_lossy().into_owned();
        let item_surface = Surface::from_url(item_id.clone(), item_url, item_name)?;
        fabric.add_surface(item_surface)?;

        // Connect item_surface -> plan_surface via Transformation
        let relation = Relation::new(
            RelationKind::Transformation {
                mapping_id: "audit-scan-item".to_string(),
            },
            1.0,
        );
        fabric.connect(&item_id, &plan_id, relation)?;
    }

    // 4. Process receipt if provided
    if let Some(_r) = receipt {
        let receipt_url = Url::parse(&format!("receipt:///{}", receipt_path.trim_start_matches('/')))
            .map_err(|e| FabricError::InvalidUrl(e.to_string()))?;
        let receipt_id = receipt_url.to_string();
        let receipt_surface = Surface::from_url(
            receipt_id.clone(),
            receipt_url,
            "Deletion Receipt".to_string(),
        )?;
        fabric.add_surface(receipt_surface)?;

        // Connect plan_surface -> receipt_surface via Evidence
        let relation = Relation::new(
            RelationKind::Evidence {
                evaluator_id: "receipt-verifier".to_string(),
            },
            1.0,
        );
        fabric.connect(&plan_id, &receipt_id, relation)?;
    }

    Ok(fabric)
}

fn path_to_url(path: &std::path::Path) -> Result<Url, FabricError> {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("github://") {
        Url::parse(&path_str).map_err(|e| FabricError::InvalidUrl(e.to_string()))
    } else {
        Url::from_file_path(path).map_err(|_| {
            FabricError::InvalidUrl(format!("Invalid file path: {:?}", path))
        })
    }
}
