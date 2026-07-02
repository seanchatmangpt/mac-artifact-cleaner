//! Bridge between Pentecost's filesystem deletion record and affidavit's
//! provenance layer.
//!
//! Architecture: affidavit owns the provenance *seal* — the BLAKE3 rolling
//! chain, the 7-stage structural verification, the content address. Pentecost
//! owns the *operational facts* (paths, statuses, bytes freed, volume deltas)
//! and the *physical* reality law ([`crate::domain::receipt::DeletionReceipt::verify`]).
//!
//! This module is the single seam where a `DeletionReceipt` is projected into a
//! sealed `affidavit::Receipt`. Object identities are BLAKE3(path), never raw
//! paths, so the sealed provenance leaks no filesystem structure.

// Re-export the affidavit types Pentecost names at this seam, so callers depend
// on the integration module rather than reaching into the upstream crate.
use affidavit::{chain::ChainAssembler, Blake3Hash, ObjectRef, OperationEvent};
pub use affidavit::{types::AdmittedReceipt, Receipt, Verdict};

use crate::domain::receipt::DeletionReceipt;

/// Project a [`DeletionReceipt`] into a sealed affidavit [`Receipt`].
///
/// The chain is one `deletion_executed` header event (bound to the
/// `deletion_plan` object) followed by one event per result, each bound to a
/// `filesystem_object` identified by BLAKE3(path). The sealed receipt therefore
/// carries `results.len() + 1` events.
///
/// Positive — a one-item receipt seals into header + one result event:
///
/// ```
/// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
/// use osx_clnr::domain::affidavit_integration::build_deletion_affidavit;
///
/// let receipt = DeletionReceipt::new(
///     0, 1, 2,
///     vec![DeletionResult {
///         path: "/tmp/build-artifact".into(),
///         status: DeletionStatus::Deleted,
///         error: None,
///         blake3_hash: None,
///         bytes_freed: 4096,
///     }],
///     None, None,
/// );
/// let sealed = build_deletion_affidavit(&receipt);
/// assert_eq!(sealed.events.len(), 2); // header + 1 result
///
/// // Negative — an empty execution still seals into a lone header event.
/// let empty = DeletionReceipt::new(0, 1, 2, vec![], None, None);
/// assert_eq!(build_deletion_affidavit(&empty).events.len(), 1);
/// ```
pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> Receipt {
    let mut assembler = ChainAssembler::new();

    // Header event: the deletion operation, bound to the plan it discharged.
    let header_event = OperationEvent {
        id: "deletion-header".to_string(),
        seq: 0,
        event_type: "deletion_executed".to_string(),
        objects: vec![ObjectRef {
            id: format!("plan-v{}", receipt.execution_record.version),
            obj_type: "deletion_plan".to_string(),
            qualifier: None,
        }],
        payload_commitment: Blake3Hash::from_bytes(
            serde_json::to_string(&receipt.execution_record).unwrap_or_default().as_bytes(),
        ),
    };
    // Append is infallible here: the event canonicalizes (no non-serializable
    // payloads), so a failure would be a library-level invariant break.
    assembler.append(header_event).expect("header event canonicalizes");

    // Per-result events: one per deleted/skipped/failed/refused item.
    for (i, result) in receipt.execution_record.results.iter().enumerate() {
        let payload_commitment = match &result.blake3_hash {
            // Pre-deletion content hash captured for files — commit to it.
            Some(hex) => Blake3Hash::from_hex(hex.clone()),
            // Directories / missing paths carry no content hash; commit to the
            // empty payload so the event is still well-formed.
            None => Blake3Hash::from_bytes(b""),
        };

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
            payload_commitment,
        };
        assembler.append(event).expect("result event canonicalizes");
    }

    assembler.finalize()
}

/// Run affidavit's 7-stage structural certification over a sealed receipt.
///
/// A receipt produced by [`build_deletion_affidavit`] is internally consistent,
/// so it certifies as accepted with one outcome per pipeline stage.
///
/// ```
/// use osx_clnr::domain::receipt::DeletionReceipt;
/// use osx_clnr::domain::affidavit_integration::{build_deletion_affidavit, certify};
///
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None));
/// let verdict = certify(&sealed);
/// assert!(verdict.accepted);              // freshly sealed → accepted
/// assert!(!verdict.outcomes.is_empty());  // per-stage outcomes recorded
/// ```
pub fn certify(receipt: &Receipt) -> Verdict {
    affidavit::verifier::verify(receipt)
}

/// Content address (BLAKE3 over the canonical receipt bytes) of a sealed receipt.
///
/// The address is a 64-character lowercase hex digest and is stable across
/// repeated calls on the same receipt.
///
/// ```
/// use osx_clnr::domain::receipt::DeletionReceipt;
/// use osx_clnr::domain::affidavit_integration::{build_deletion_affidavit, content_address};
///
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None));
/// let addr = content_address(&sealed);
/// assert_eq!(addr.len(), 64);                       // BLAKE3-256 hex
/// assert_eq!(addr, content_address(&sealed));       // stable
/// ```
pub fn content_address(receipt: &Receipt) -> String {
    affidavit::chain::content_address(receipt).map(|h| h.as_hex().to_string()).unwrap_or_default()
}

/// Canonical (sorted-key) JSON bytes for a sealed receipt — byte-stable and
/// re-verifiable by upstream `affi verify`.
///
/// ```
/// use osx_clnr::domain::receipt::DeletionReceipt;
/// use osx_clnr::domain::affidavit_integration::{build_deletion_affidavit, serialize_receipt};
///
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None));
/// let bytes = serialize_receipt(&sealed);
/// assert!(!bytes.is_empty());
/// let json = String::from_utf8(bytes).unwrap();
/// assert!(json.contains("core/v1"));  // stamped format version
/// ```
pub fn serialize_receipt(receipt: &Receipt) -> Vec<u8> {
    affidavit::chain::serialize_receipt(receipt).unwrap_or_default()
}

/// Admit a receipt through affidavit's dual-court Layer 2 gate (OCEL structural
/// law + BLAKE3 chain law). Mints `Admitted` only if both courts accept.
///
/// A receipt from [`build_deletion_affidavit`] carries non-empty event→object
/// links and a sound chain, so it passes both courts.
///
/// ```
/// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
/// use osx_clnr::domain::affidavit_integration::{build_deletion_affidavit, admit};
///
/// let receipt = DeletionReceipt::new(
///     0, 1, 2,
///     vec![DeletionResult {
///         path: "/tmp/x".into(),
///         status: DeletionStatus::Deleted,
///         error: None,
///         blake3_hash: None,
///         bytes_freed: 1,
///     }],
///     None, None,
/// );
/// let sealed = build_deletion_affidavit(&receipt);
/// assert!(admit(sealed).is_ok());  // both courts accept
/// ```
pub fn admit(receipt: Receipt) -> Result<AdmittedReceipt, affidavit::admission::AffidavitRefusal> {
    affidavit::admission::admit(receipt)
}
