//! Bridge between Pentecost's DeletionReceipt and affidavit's Receipt types.
//!
//! This adapter translates filesystem deletion operations into the universal
//! affidavit Receipt format: BLAKE3 rolling chains of operation events with
//! privacy-preserving object identities (hashed paths, not raw paths).

use crate::domain::receipt::DeletionReceipt;
use affidavit::{Blake3Hash, ChainAssembler, ObjectRef, OperationEvent, Receipt};

/// Build an affidavit Receipt from a DeletionReceipt.
///
/// Maps the deletion result sequence into a sealed BLAKE3 chain:
/// - Header event: records the plan that triggered the deletion
/// - Per-result events: one event per deleted artifact, with status and hash
/// - Object identities are BLAKE3(path) for privacy (no path disclosure)
pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> Receipt {
    let mut assembler = ChainAssembler::new();

    // Header event: the deletion operation itself
    let header_event = OperationEvent {
        id: "deletion-header".to_string(),
        seq: 0,
        event_type: "deletion_executed".to_string(),
        objects: vec![ObjectRef {
            id: format!("plan-{}", receipt.execution_record.version),
            obj_type: "deletion_plan".to_string(),
            qualifier: None,
        }],
        payload_commitment: Blake3Hash::from_bytes(
            serde_json::to_string(&receipt.execution_record)
                .unwrap_or_default()
                .as_bytes(),
        ),
    };
    assembler.append(header_event).unwrap();

    // Per-result events: one event per deleted item
    for (i, result) in receipt.execution_record.results.iter().enumerate() {
        let event = OperationEvent {
            id: format!("deletion-{}", i),
            seq: (i + 1) as u64,
            event_type: format!("deletion_{:?}", result.status).to_lowercase(),
            objects: vec![ObjectRef {
                id: Blake3Hash::from_bytes(result.path.to_string_lossy().as_bytes())
                    .as_hex()
                    .to_string(),
                obj_type: "filesystem_object".to_string(),
                qualifier: Some("deleted".to_string()),
            }],
            payload_commitment: result
                .blake3_hash
                .as_ref()
                .cloned()
                .unwrap_or_else(|| Blake3Hash::from_bytes(b"")),
        };
        assembler.append(event).unwrap();
    }

    assembler.finalize()
}

/// Verify an affidavit Receipt using the 7-stage certification pipeline.
pub fn certify(receipt: &Receipt) -> affidavit::Verdict {
    affidavit::verifier::verify(receipt)
}

/// Admit a receipt through the dual-court Layer 2 gate.
///
/// Runs BOTH:
/// 1. OCEL structural law (event→object links, referential integrity)
/// 2. Affidavit chain law (BLAKE3 continuity, commitments)
///
/// Only mints `Admitted` if both courts pass.
pub fn admit(
    receipt: Receipt,
) -> Result<affidavit::AdmittedReceipt, affidavit::admission::AffidavitRefusal> {
    affidavit::admission::admit(receipt)
}
