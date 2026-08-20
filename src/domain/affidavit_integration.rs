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

use crate::domain::{receipt::DeletionReceipt, time::SnapshotThinReceipt};

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
/// let sealed = build_deletion_affidavit(&receipt).unwrap();
/// assert_eq!(sealed.events.len(), 2); // header + 1 result
///
/// // Negative — an empty execution still seals into a lone header event.
/// let empty = DeletionReceipt::new(0, 1, 2, vec![], None, None);
/// assert_eq!(build_deletion_affidavit(&empty).unwrap().events.len(), 1);
/// ```
///
/// # Errors
///
/// Returns `Err` if `receipt.execution_record` fails to serialize to JSON.
/// This is possible in practice, not just theoretical: `serde`'s `PathBuf`
/// serialization fails on a non-UTF-8 path (permitted on real Unix
/// filesystems), and `DeletionResult.path` is a `PathBuf`. A failure here
/// used to be silently laundered into an empty-payload commitment via
/// `unwrap_or_default()` — sealing a cryptographic chain over nothing while
/// still reporting success. See `no-overclaiming-rust.md`.
pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> anyhow::Result<Receipt> {
    let mut assembler = ChainAssembler::new();

    // Header event: the deletion operation, bound to the plan it discharged.
    let execution_record_json = serde_json::to_string(&receipt.execution_record)
        .map_err(|e| anyhow::anyhow!("could not serialize execution_record for sealing: {e}"))?;
    let header_event = OperationEvent {
        id: "deletion-header".to_string(),
        seq: 0,
        event_type: "deletion_executed".to_string(),
        objects: vec![ObjectRef {
            id: format!("plan-v{}", receipt.execution_record.version),
            obj_type: "deletion_plan".to_string(),
            qualifier: None,
        }],
        payload_commitment: Blake3Hash::from_bytes(execution_record_json.as_bytes()),
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

    Ok(assembler.finalize())
}

/// Shared projection for [`SnapshotThinReceipt`], parameterized by the
/// truthful event/object-type vocabulary of the operation that produced it.
/// Thinning and selective deletion are distinct operations (byte-driven vs
/// name/date-driven — see [`crate::domain::ocel::build_snapshot_thin_ocel`]
/// and [`crate::domain::ocel::build_snapshot_delete_ocel`]) and must not be
/// conflated in the sealed provenance any more than in the OCEL log.
fn build_snapshot_affidavit(
    receipt: &SnapshotThinReceipt,
    event_type: &str,
    plan_obj_type: &str,
) -> anyhow::Result<Receipt> {
    let mut assembler = ChainAssembler::new();

    let receipt_json = serde_json::to_string(receipt)
        .map_err(|e| anyhow::anyhow!("could not serialize snapshot receipt for sealing: {e}"))?;
    let header_event = OperationEvent {
        id: format!("{}-header", event_type),
        seq: 0,
        event_type: event_type.to_string(),
        objects: vec![ObjectRef {
            id: format!("{}-{}", receipt.volume, receipt.timestamp_unix),
            obj_type: plan_obj_type.to_string(),
            qualifier: None,
        }],
        payload_commitment: Blake3Hash::from_bytes(receipt_json.as_bytes()),
    };
    assembler.append(header_event).expect("header event canonicalizes");

    for (i, name) in receipt.snapshots_thinned.iter().enumerate() {
        let event = OperationEvent {
            id: format!("{}-{}", event_type, i),
            seq: (i + 1) as u64,
            event_type: event_type.to_string(),
            objects: vec![ObjectRef {
                id: Blake3Hash::from_bytes(name.as_bytes()).as_hex().to_string(),
                obj_type: "snapshot_state".to_string(),
                qualifier: Some("removed".to_string()),
            }],
            payload_commitment: Blake3Hash::from_bytes(name.as_bytes()),
        };
        assembler.append(event).expect("result event canonicalizes");
    }

    Ok(assembler.finalize())
}

/// Project a [`SnapshotThinReceipt`] produced by a byte-driven thin operation
/// into a sealed affidavit [`Receipt`].
///
/// ```
/// use osx_clnr::domain::time::SnapshotThinReceipt;
/// use osx_clnr::domain::affidavit_integration::build_snapshot_thin_affidavit;
///
/// let receipt = SnapshotThinReceipt::new(
///     "/".to_string(), 1_000_000, 1_716_768_000,
///     vec!["snap1".to_string(), "snap2".to_string()],
///     vec!["snap2".to_string()],
/// );
/// let sealed = build_snapshot_thin_affidavit(&receipt).unwrap();
/// assert_eq!(sealed.events.len(), 2); // header + 1 thinned snapshot
///
/// // Negative — nothing thinned still seals into a lone header event.
/// let empty = SnapshotThinReceipt::new(
///     "/".to_string(), 1_000_000, 1_716_768_000,
///     vec!["snap1".to_string()], vec!["snap1".to_string()],
/// );
/// assert_eq!(build_snapshot_thin_affidavit(&empty).unwrap().events.len(), 1);
/// ```
///
/// # Errors
///
/// Returns `Err` if `receipt` fails to serialize — see
/// [`build_deletion_affidavit`]'s `# Errors` section for why this is a real,
/// not theoretical, failure mode.
pub fn build_snapshot_thin_affidavit(receipt: &SnapshotThinReceipt) -> anyhow::Result<Receipt> {
    build_snapshot_affidavit(receipt, "snapshot_thin_requested", "snapshot_thin_plan")
}

/// Project a [`SnapshotThinReceipt`] produced by a name/date-driven selective
/// delete into a sealed affidavit [`Receipt`].
///
/// Uses the distinct `snapshot_delete_requested` event type so a delete is
/// never mistakable for a thin in the sealed provenance chain.
///
/// ```
/// use osx_clnr::domain::time::SnapshotThinReceipt;
/// use osx_clnr::domain::affidavit_integration::build_snapshot_delete_affidavit;
///
/// let receipt = SnapshotThinReceipt::new(
///     "/".to_string(), 0, 1_716_768_000,
///     vec!["snap1".to_string(), "snap2".to_string()],
///     vec!["snap2".to_string()],
/// );
/// let sealed = build_snapshot_delete_affidavit(&receipt).unwrap();
/// assert_eq!(sealed.events.len(), 2); // header + 1 deleted snapshot
/// assert_eq!(sealed.events[0].event_type, "snapshot_delete_requested");
/// ```
///
/// # Errors
///
/// Returns `Err` if `receipt` fails to serialize — see
/// [`build_deletion_affidavit`]'s `# Errors` section for why this is a real,
/// not theoretical, failure mode.
pub fn build_snapshot_delete_affidavit(receipt: &SnapshotThinReceipt) -> anyhow::Result<Receipt> {
    build_snapshot_affidavit(receipt, "snapshot_delete_requested", "snapshot_delete_plan")
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
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None)).unwrap();
/// let verdict = certify(&sealed);
/// assert!(verdict.accepted);              // freshly sealed → accepted
/// assert!(!verdict.outcomes.is_empty());  // per-stage outcomes recorded
/// ```
pub fn certify(receipt: &Receipt) -> Verdict {
    affidavit::verifier::verify(receipt)
}

/// Fold a `chain_integrity` stage into a [`Verdict`], comparing a freshly
/// recomputed `chain_hash` against a `stored_chain_hash` read back from a
/// previously persisted affidavit file.
///
/// [`certify`] only checks that a *freshly built* [`Receipt`] is internally
/// consistent — it has no notion of "the file on disk was hand-edited after
/// the fact". This function is the seam that closes that gap: callers in the
/// integration layer read the on-disk `chain_hash` (I/O) and pass it here so
/// the comparison itself stays pure and testable.
///
/// If `stored_chain_hash` is `None` (no prior affidavit file to compare
/// against), the verdict passes through unchanged.
///
/// ```
/// use osx_clnr::domain::receipt::DeletionReceipt;
/// use osx_clnr::domain::affidavit_integration::{build_deletion_affidavit, certify, verify_chain_hash};
///
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None)).unwrap();
/// let verdict = certify(&sealed);
///
/// // Positive — stored hash matches the recomputed one.
/// let matching = verify_chain_hash(verdict.clone(), sealed.chain_hash.as_hex(), Some(sealed.chain_hash.as_hex()));
/// assert!(matching.accepted);
///
/// // Negative — a tampered stored hash must reject, even though the
/// // freshly-recomputed receipt is internally consistent.
/// let tampered = verify_chain_hash(verdict, sealed.chain_hash.as_hex(), Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"));
/// assert!(!tampered.accepted);
/// assert!(tampered.outcomes.iter().any(|o| o.stage == "chain_integrity" && !o.passed));
/// ```
pub fn verify_chain_hash(
    mut verdict: Verdict,
    recomputed_chain_hash: &str,
    stored_chain_hash: Option<&str>,
) -> Verdict {
    let Some(stored) = stored_chain_hash else {
        return verdict;
    };

    if stored == recomputed_chain_hash {
        verdict.outcomes.push(affidavit::CheckOutcome {
            stage: "chain_integrity".to_string(),
            passed: true,
            detail: "recomputed chain hash matches stored chain_hash".to_string(),
        });
    } else {
        verdict.accepted = false;
        verdict.reason = format!(
            "chain_integrity: stored chain_hash '{stored}' does not match recomputed chain_hash '{recomputed_chain_hash}' — receipt or affidavit file was modified after sealing"
        );
        verdict.outcomes.push(affidavit::CheckOutcome {
            stage: "chain_integrity".to_string(),
            passed: false,
            detail: verdict.reason.clone(),
        });
    }

    verdict
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
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None)).unwrap();
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
/// let sealed = build_deletion_affidavit(&DeletionReceipt::new(0, 1, 2, vec![], None, None)).unwrap();
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
/// let sealed = build_deletion_affidavit(&receipt).unwrap();
/// assert!(admit(sealed).is_ok());  // both courts accept
/// ```
pub fn admit(receipt: Receipt) -> Result<AdmittedReceipt, affidavit::admission::AffidavitRefusal> {
    affidavit::admission::admit(receipt)
}
