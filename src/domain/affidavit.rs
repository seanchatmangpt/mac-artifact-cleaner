//! Affidavit support — a faithful, in-tree port of the affidavit `core/v1`
//! cryptographic provenance kernel (upstream `affidavit` v26.6.14,
//! <https://github.com/seanchatmangpt/affidavit>).
//!
//! Affidavit's doctrine is *certify, don't decide*: a receipt is an append-only,
//! content-addressed chain of operation-events, and the verifier checks a witness
//! against a fixed format standard rather than deciding whether the underlying
//! process was honest. This module reproduces the upstream kernel byte-for-byte —
//! the same [`GENESIS_SEED`], the same rolling-hash recurrence, and the same
//! canonical (sorted-key) JSON encoding — so receipts emitted here verify under
//! the upstream `affi verify` command and vice-versa.
//!
//! It is vendored rather than taken as a crate dependency for two reasons that
//! matter to this project specifically:
//!
//! 1. **Domain purity.** This module performs *zero* `std::fs`, `std::process`,
//!    or OS calls — exactly like the rest of `src/domain/**`. The upstream crate
//!    carries a full CLI (`affi`) with filesystem persistence; only its pure
//!    kernel belongs in the domain layer. Persistence lives in the nouns layer.
//! 2. **Receipts, not capability.** The project invariant is *never increase
//!    destructive power without simultaneously increasing receipts.* Affidavit is
//!    pure receipt machinery; vendoring the kernel keeps the deletion pipeline
//!    self-contained while still emitting cross-verifiable provenance.
//!
//! The chain rule (deterministic, append-only):
//!
//! ```text
//! chain_hash_0 = blake3(GENESIS_SEED)
//! chain_hash_n = blake3(chain_hash_{n-1}.as_hex().as_bytes() || canonical_bytes(event_n))
//! ```
//!
//! Any change to an event's bytes propagates through every subsequent link, so a
//! valid receipt cannot be forged or hand-edited without breaking the chain.

use serde::{Deserialize, Deserializer, Serialize};

use crate::domain::receipt::{DeletionReceipt, DeletionStatus};

/// Format version stamped into assembled receipts (upstream `chain::FORMAT_VERSION`).
pub const FORMAT_VERSION: &str = "core/v1";

/// Genesis seed for the rolling chain hash. Binds chains to this release and is
/// identical to upstream so cross-tool verification holds.
pub const GENESIS_SEED: &[u8] = b"affidavit-v26.6.14-genesis";

/// Expected hex length of a BLAKE3-256 digest (32 bytes → 64 hex chars).
const BLAKE3_HEX_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Core types (faithful port of `affidavit::types`)
// ---------------------------------------------------------------------------

/// A BLAKE3 digest rendered as a lowercase hex string.
///
/// Stored as hex so receipts serialize to canonical, human-diffable JSON.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Blake3Hash(pub String);

impl Blake3Hash {
    /// Construct a hash from raw bytes by computing their BLAKE3 digest.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::Blake3Hash;
    /// // A BLAKE3-256 digest is always 64 lowercase hex chars.
    /// assert_eq!(Blake3Hash::from_bytes(b"hello world").as_hex().len(), 64);
    /// // Distinct inputs yield distinct digests; identical inputs are stable.
    /// assert_ne!(Blake3Hash::from_bytes(b"a"), Blake3Hash::from_bytes(b"b"));
    /// assert_eq!(Blake3Hash::from_bytes(b"a"), Blake3Hash::from_bytes(b"a"));
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Blake3Hash(blake3::hash(bytes).to_hex().to_string())
    }

    /// Construct a hash from an already-computed lowercase hex string.
    ///
    /// Used on deserialization round-trips where the digest is already known.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::Blake3Hash;
    /// // `from_hex` stores the string verbatim — it does not re-hash.
    /// let h = Blake3Hash::from_hex("abc123");
    /// assert_eq!(h.as_hex(), "abc123");
    /// // It is the inverse of `as_hex` for any computed digest.
    /// let computed = Blake3Hash::from_bytes(b"x");
    /// assert_eq!(Blake3Hash::from_hex(computed.as_hex().to_string()), computed);
    /// ```
    pub fn from_hex(hex: impl Into<String>) -> Self {
        Blake3Hash(hex.into())
    }

    /// Borrow the hex representation of this hash.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::Blake3Hash;
    /// assert_eq!(Blake3Hash::from_hex("deadbeef").as_hex(), "deadbeef");
    /// ```
    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A qualified reference from an operation-event to an OCEL object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    /// Stable identifier of the referenced object.
    pub id: String,
    /// OCEL object type (the class of the object).
    pub obj_type: String,
    /// Optional qualifier describing the role of the object in the event.
    pub qualifier: Option<String>,
}

/// A single append-only operation-event in a receipt chain.
///
/// Carries a logical sequence number (never wall-clock) and a commitment to its
/// payload bytes — the verifier checks commitments without seeing payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEvent {
    /// Identifier of this event, unique within the receipt.
    pub id: String,
    /// Monotonic logical sequence number (deterministic ordering, not time).
    pub seq: u64,
    /// The kind of operation this event records.
    pub event_type: String,
    /// Qualified object references this event relates to.
    pub objects: Vec<ObjectRef>,
    /// BLAKE3 commitment to the event's payload bytes.
    pub payload_commitment: Blake3Hash,
}

/// An immutable, content-addressed chain of operation-events.
///
/// The private `_seal` field prevents struct-literal construction from outside
/// this module, enforcing that receipts are built only through the canonically
/// sealed seam [`ChainAssembler::finalize`]. Deserialization re-verifies the
/// chain hash to block forged receipts from JSON.
//
// `#[non_exhaustive]` is deliberately *not* used: the private unit `_seal`
// field is the upstream sealing mechanism (it makes external struct-literal
// construction a hard E0451 error and is `#[serde(skip)]`-ed out of the wire
// format). Keeping it byte-for-byte matches affidavit `core/v1`, so clippy's
// `manual_non_exhaustive` suggestion is intentionally suppressed.
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Receipt {
    /// Format version string used by the verifier's format check.
    pub format_version: String,
    /// Ordered, append-only operation-events.
    pub events: Vec<OperationEvent>,
    /// Rolling BLAKE3 hash computed over the events in order.
    pub chain_hash: Blake3Hash,
    /// Private seal — struct-literal construction is unconstructable.
    #[serde(skip)]
    _seal: (),
}

impl Receipt {
    /// Construct a `Receipt` with the canonical sealing. Used only by
    /// [`ChainAssembler::finalize`]; external code cannot call this because
    /// `_seal` is private.
    fn sealed(format_version: String, events: Vec<OperationEvent>, chain_hash: Blake3Hash) -> Self {
        Receipt {
            format_version,
            events,
            chain_hash,
            _seal: (),
        }
    }
}

/// Custom deserialization that re-verifies the chain hash: a `Receipt` read from
/// JSON is valid only if its `chain_hash` recomputes correctly from its events.
/// This closes the deserialization-forgery door.
impl<'de> Deserialize<'de> for Receipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        #[derive(Deserialize)]
        struct RawReceipt {
            format_version: String,
            events: Vec<OperationEvent>,
            chain_hash: Blake3Hash,
        }

        let raw = RawReceipt::deserialize(deserializer)?;
        let recomputed = recompute_chain(&raw.events);
        if recomputed != raw.chain_hash {
            return Err(D::Error::custom(format!(
                "chain hash mismatch: receipt claims {}, recomputed {}",
                raw.chain_hash, recomputed
            )));
        }
        Ok(Receipt::sealed(
            raw.format_version,
            raw.events,
            raw.chain_hash,
        ))
    }
}

/// The conformance profile a verdict was evaluated under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileId {
    /// Core v1: every event has a commitment and a non-empty event_type.
    CoreV1,
}

impl ProfileId {
    /// Stable string identifier for this profile.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::ProfileId;
    /// // CoreV1 stringifies to the shared format-standard tag.
    /// assert_eq!(ProfileId::CoreV1.as_str(), "core/v1");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            ProfileId::CoreV1 => "core/v1",
        }
    }
}

/// The result of a single decidable pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutcome {
    /// Name of the pipeline stage that produced this outcome.
    pub stage: String,
    /// Whether the stage's decidable check passed.
    pub passed: bool,
    /// Human-readable explanation of the outcome.
    pub detail: String,
}

/// The final verdict of the certify pipeline over a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// True when every required stage passed (ACCEPT), false otherwise (REJECT).
    pub accepted: bool,
    /// The conformance profile under which the receipt was evaluated.
    pub profile: ProfileId,
    /// Per-stage outcomes, in pipeline order.
    pub outcomes: Vec<CheckOutcome>,
    /// Summary reason for the final verdict.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Canonical encoding (faithful port of `affidavit::types::canonical_bytes`)
// ---------------------------------------------------------------------------

/// Produce deterministic, sorted-key JSON bytes for any serializable value.
///
/// This is the canonical byte form used for content addressing and hashing:
/// object keys are recursively sorted so the same logical value always yields
/// identical bytes regardless of in-memory field order.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::canonical_bytes;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Data { b: i32, a: i32 }
/// // Keys are sorted: `a` before `b`, regardless of declaration order.
/// assert_eq!(canonical_bytes(&Data { b: 2, a: 1 }), br#"{"a":1,"b":2}"#);
/// ```
pub fn canonical_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let v = serde_json::to_value(value).expect("serializable value");
    let sorted = sort_value(v);
    serde_json::to_vec(&sorted).expect("re-serializable value")
}

/// Recursively sort the keys of all JSON objects within a value.
fn sort_value(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<String, Value> =
                map.into_iter().map(|(k, v)| (k, sort_value(v))).collect();
            let mut out = serde_json::Map::new();
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_value).collect()),
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Chain assembly (faithful port of `affidavit::chain`, pure subset only)
// ---------------------------------------------------------------------------

/// Compute the genesis link `chain_hash_0 = blake3(GENESIS_SEED)`.
fn genesis_hash() -> Blake3Hash {
    Blake3Hash::from_bytes(GENESIS_SEED)
}

/// Fold one event into a running chain hash:
/// `blake3(prev.as_hex().as_bytes() || canonical_bytes(event))`.
fn fold_event(prev: &Blake3Hash, event: &OperationEvent) -> Blake3Hash {
    let event_bytes = canonical_bytes(event);
    let mut buf = Vec::with_capacity(prev.as_hex().len() + event_bytes.len());
    buf.extend_from_slice(prev.as_hex().as_bytes());
    buf.extend_from_slice(&event_bytes);
    Blake3Hash::from_bytes(&buf)
}

/// Purely recompute the rolling chain hash over an ordered slice of events.
///
/// The verifier re-derives the chain hash from event bytes alone and compares it
/// against the receipt's stored `chain_hash`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::{recompute_chain, ChainAssembler, OperationEvent, Blake3Hash};
///
/// // The empty chain equals the genesis hash.
/// let empty = recompute_chain(&[]);
/// let asm = ChainAssembler::new();
/// assert_eq!(empty, asm.finalize().chain_hash);
///
/// // Recomputation is deterministic and order-sensitive.
/// let ev = |seq: u64, p: &[u8]| OperationEvent {
///     id: format!("e{seq}"),
///     seq,
///     event_type: "test.op".to_string(),
///     objects: vec![],
///     payload_commitment: Blake3Hash::from_bytes(p),
/// };
/// let a = vec![ev(0, b"a"), ev(1, b"b")];
/// let b = vec![ev(1, b"b"), ev(0, b"a")];
/// assert_eq!(recompute_chain(&a), recompute_chain(&a));
/// assert_ne!(recompute_chain(&a), recompute_chain(&b));
/// ```
pub fn recompute_chain(events: &[OperationEvent]) -> Blake3Hash {
    let mut acc = genesis_hash();
    for event in events {
        acc = fold_event(&acc, event);
    }
    acc
}

/// An append-only assembler that maintains a rolling chain hash as events arrive.
#[derive(Debug, Clone, Default)]
pub struct ChainAssembler {
    events: Vec<OperationEvent>,
    running: Option<Blake3Hash>,
}

impl ChainAssembler {
    /// Create an empty assembler seeded with the genesis hash.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::ChainAssembler;
    /// let asm = ChainAssembler::new();
    /// assert!(asm.is_empty());
    /// assert_eq!(asm.len(), 0);
    /// ```
    pub fn new() -> Self {
        ChainAssembler {
            events: Vec::new(),
            running: None,
        }
    }

    /// Append one operation-event, folding it into the running chain hash.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::{ChainAssembler, OperationEvent, Blake3Hash, recompute_chain};
    /// let ev = OperationEvent {
    ///     id: "e0".into(), seq: 0, event_type: "emit".into(),
    ///     objects: vec![], payload_commitment: Blake3Hash::from_bytes(b"p"),
    /// };
    /// let mut asm = ChainAssembler::new();
    /// asm.append(ev.clone());
    /// // The incremental running hash matches a fresh recomputation.
    /// assert_eq!(asm.finalize().chain_hash, recompute_chain(&[ev]));
    /// ```
    pub fn append(&mut self, event: OperationEvent) {
        let prev = self.running.take().unwrap_or_else(genesis_hash);
        self.running = Some(fold_event(&prev, &event));
        self.events.push(event);
    }

    /// Borrow the events appended so far, in order.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::{ChainAssembler, OperationEvent, Blake3Hash};
    /// let mut asm = ChainAssembler::new();
    /// assert!(asm.events().is_empty());
    /// asm.append(OperationEvent {
    ///     id: "e0".into(), seq: 0, event_type: "emit".into(),
    ///     objects: vec![], payload_commitment: Blake3Hash::from_bytes(b"p"),
    /// });
    /// assert_eq!(asm.events()[0].id, "e0");
    /// ```
    pub fn events(&self) -> &[OperationEvent] {
        &self.events
    }

    /// Number of events appended so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::{ChainAssembler, OperationEvent, Blake3Hash};
    /// let mut asm = ChainAssembler::new();
    /// assert_eq!(asm.len(), 0);
    /// asm.append(OperationEvent {
    ///     id: "e0".into(), seq: 0, event_type: "emit".into(),
    ///     objects: vec![], payload_commitment: Blake3Hash::from_bytes(b"p"),
    /// });
    /// assert_eq!(asm.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether no events have been appended yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::{ChainAssembler, OperationEvent, Blake3Hash};
    /// let mut asm = ChainAssembler::new();
    /// assert!(asm.is_empty());
    /// asm.append(OperationEvent {
    ///     id: "e0".into(), seq: 0, event_type: "emit".into(),
    ///     objects: vec![], payload_commitment: Blake3Hash::from_bytes(b"p"),
    /// });
    /// assert!(!asm.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Finalize into an immutable [`Receipt`] carrying the final chain hash.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::affidavit::{ChainAssembler, FORMAT_VERSION, certify};
    /// // An empty assembler still seals into a valid (header-less) receipt.
    /// let receipt = ChainAssembler::new().finalize();
    /// assert_eq!(receipt.format_version, FORMAT_VERSION);
    /// assert!(certify(&receipt).accepted);
    /// ```
    pub fn finalize(self) -> Receipt {
        let chain_hash = self.running.unwrap_or_else(genesis_hash);
        Receipt::sealed(FORMAT_VERSION.to_string(), self.events, chain_hash)
    }
}

/// Content address of a receipt: `blake3(canonical_bytes(receipt))`. Used as the
/// receipt's immutable filename upstream.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::{ChainAssembler, content_address};
/// let receipt = ChainAssembler::new().finalize();
/// // Content addressing is a 64-hex BLAKE3 digest and is stable per value.
/// assert_eq!(content_address(&receipt).as_hex().len(), 64);
/// assert_eq!(content_address(&receipt), content_address(&receipt));
/// ```
pub fn content_address(receipt: &Receipt) -> Blake3Hash {
    Blake3Hash::from_bytes(&canonical_bytes(receipt))
}

/// Serialize a receipt to canonical (sorted-key) JSON bytes.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::{ChainAssembler, serialize_receipt};
/// let receipt = ChainAssembler::new().finalize();
/// let json = String::from_utf8(serialize_receipt(&receipt)).unwrap();
/// // Canonical JSON sorts keys: `chain_hash` precedes `format_version`.
/// assert!(json.starts_with("{\"chain_hash\":"));
/// assert!(json.contains("\"format_version\":\"core/v1\""));
/// ```
pub fn serialize_receipt(receipt: &Receipt) -> Vec<u8> {
    canonical_bytes(receipt)
}

// ---------------------------------------------------------------------------
// Verifier (faithful port of `affidavit::verifier`, the 7-stage pipeline)
// ---------------------------------------------------------------------------

/// Whether a hex string is a well-formed lowercase BLAKE3-256 digest.
fn is_well_formed_hash(hex: &str) -> bool {
    hex.len() == BLAKE3_HEX_LEN
        && hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
}

/// Certify a receipt by running the seven-stage decidable pipeline:
/// decode → check_format → chain_integrity → continuity → verify_commitments →
/// evaluate_profile → emit_verdict.
///
/// Produces one [`CheckOutcome`] per stage in order plus a final [`Verdict`].
/// The verdict is `accepted` only when every prior stage passed. The function is
/// pure: the same receipt always yields the same verdict, and it reads only
/// payload commitments, never raw payloads.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::{certify, ChainAssembler, OperationEvent, ObjectRef, Blake3Hash};
///
/// fn ev(id: &str, seq: u64, ty: &str, payload: &[u8]) -> OperationEvent {
///     OperationEvent {
///         id: id.to_string(),
///         seq,
///         event_type: ty.to_string(),
///         objects: vec![ObjectRef { id: format!("obj-{id}"), obj_type: "artifact".into(), qualifier: None }],
///         payload_commitment: Blake3Hash::from_bytes(payload),
///     }
/// }
///
/// // Positive case: a sealed, well-formed chain is ACCEPTED.
/// let mut asm = ChainAssembler::new();
/// asm.append(ev("e0", 0, "emit", b"zero"));
/// asm.append(ev("e1", 1, "emit", b"one"));
/// let receipt = asm.finalize();
/// let verdict = certify(&receipt);
/// assert!(verdict.accepted);
/// assert_eq!(verdict.reason, "all stages passed");
///
/// // Refusal case: tampering a commitment without re-sealing breaks chain integrity.
/// let mut forged = receipt.clone();
/// forged.events[1].payload_commitment = Blake3Hash::from_bytes(b"tampered");
/// let bad = certify(&forged);
/// assert!(!bad.accepted);
/// assert!(bad.outcomes.iter().any(|o| o.stage == "chain_integrity" && !o.passed));
///
/// // Negative case: a wrong format version is REJECTED at the format stage.
/// let mut wrong = receipt.clone();
/// wrong.format_version = "1.0.0".to_string();
/// let bad = certify(&wrong);
/// assert!(!bad.accepted);
/// assert!(bad.outcomes.iter().any(|o| o.stage == "check_format" && !o.passed));
/// ```
pub fn certify(receipt: &Receipt) -> Verdict {
    let outcomes: Vec<CheckOutcome> = vec![
        stage_decode(receipt),
        stage_check_format(receipt),
        stage_chain_integrity(receipt),
        stage_continuity(receipt),
        stage_verify_commitments(receipt),
        stage_evaluate_profile(receipt),
    ];

    let first_failure = outcomes.iter().find(|o| !o.passed);
    let accepted = first_failure.is_none();
    let reason = match first_failure {
        Some(o) => format!("{}: {}", o.stage, o.detail),
        None => "all stages passed".to_string(),
    };

    Verdict {
        accepted,
        profile: ProfileId::CoreV1,
        outcomes,
        reason,
    }
}

/// Stage 1: the receipt is structurally present and its version is parseable.
fn stage_decode(receipt: &Receipt) -> CheckOutcome {
    let passed = !receipt.format_version.trim().is_empty();
    CheckOutcome {
        stage: "decode".to_string(),
        passed,
        detail: if passed {
            format!("{} event(s), format_version present", receipt.events.len())
        } else {
            "format_version is empty or unparseable".to_string()
        },
    }
}

/// Stage 2: the receipt's format version matches the verifier's standard.
fn stage_check_format(receipt: &Receipt) -> CheckOutcome {
    let passed = receipt.format_version == FORMAT_VERSION;
    CheckOutcome {
        stage: "check_format".to_string(),
        passed,
        detail: if passed {
            format!("format_version == {FORMAT_VERSION}")
        } else {
            format!(
                "expected format_version {FORMAT_VERSION}, found {}",
                receipt.format_version
            )
        },
    }
}

/// Stage 3: the recomputed rolling chain hash equals the stored chain hash.
fn stage_chain_integrity(receipt: &Receipt) -> CheckOutcome {
    let computed = recompute_chain(&receipt.events);
    let passed = computed == receipt.chain_hash;
    CheckOutcome {
        stage: "chain_integrity".to_string(),
        passed,
        detail: if passed {
            "recomputed chain hash matches stored chain_hash".to_string()
        } else {
            format!(
                "chain hash mismatch: stored {}, recomputed {}",
                receipt.chain_hash, computed
            )
        },
    }
}

/// Stage 4: seq numbers are strictly increasing from 0 with no gaps; ids unique.
fn stage_continuity(receipt: &Receipt) -> CheckOutcome {
    let mut seen_ids: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for (index, event) in receipt.events.iter().enumerate() {
        let expected_seq = index as u64;
        if event.seq != expected_seq {
            return CheckOutcome {
                stage: "continuity".to_string(),
                passed: false,
                detail: format!(
                    "seq gap at position {index}: expected {expected_seq}, found {}",
                    event.seq
                ),
            };
        }
        if !seen_ids.insert(event.id.as_str()) {
            return CheckOutcome {
                stage: "continuity".to_string(),
                passed: false,
                detail: format!("duplicate event id: {}", event.id),
            };
        }
    }
    CheckOutcome {
        stage: "continuity".to_string(),
        passed: true,
        detail: format!(
            "{} event(s) with contiguous seq and unique ids",
            receipt.events.len()
        ),
    }
}

/// Stage 5: every event carries a well-formed (correct-length lowercase hex) commitment.
fn stage_verify_commitments(receipt: &Receipt) -> CheckOutcome {
    for event in &receipt.events {
        if !is_well_formed_hash(event.payload_commitment.as_hex()) {
            return CheckOutcome {
                stage: "verify_commitments".to_string(),
                passed: false,
                detail: format!(
                    "event {} has a malformed commitment (expected {BLAKE3_HEX_LEN} lowercase hex chars)",
                    event.id
                ),
            };
        }
    }
    CheckOutcome {
        stage: "verify_commitments".to_string(),
        passed: true,
        detail: "all commitments are well-formed BLAKE3 digests".to_string(),
    }
}

/// Stage 6 (CoreV1 profile): each event has a non-empty event_type and commitment.
fn stage_evaluate_profile(receipt: &Receipt) -> CheckOutcome {
    for event in &receipt.events {
        if event.event_type.trim().is_empty() {
            return CheckOutcome {
                stage: "evaluate_profile".to_string(),
                passed: false,
                detail: format!("event {} has an empty event_type", event.id),
            };
        }
        if event.payload_commitment.as_hex().is_empty() {
            return CheckOutcome {
                stage: "evaluate_profile".to_string(),
                passed: false,
                detail: format!("event {} is missing a commitment", event.id),
            };
        }
    }
    CheckOutcome {
        stage: "evaluate_profile".to_string(),
        passed: true,
        detail: format!("profile {} satisfied", ProfileId::CoreV1.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Bridge: DeletionReceipt → affidavit Receipt
// ---------------------------------------------------------------------------

/// Object type used for the per-deletion filesystem target.
const OBJ_FILESYSTEM: &str = "filesystem_object";
/// Object type used for the deletion receipt the chain attests to.
const OBJ_RECEIPT: &str = "delete_receipt";

/// Map a [`DeletionStatus`] to its affidavit event type. The event type must
/// truthfully reflect the operation — a skip is not a delete.
fn status_event_type(status: DeletionStatus) -> &'static str {
    match status {
        DeletionStatus::Deleted => "artifact.deleted",
        DeletionStatus::SkippedMissing => "artifact.skipped_missing",
        DeletionStatus::Refused => "artifact.refused",
        DeletionStatus::Failed => "artifact.deletion_failed",
    }
}

/// Build a sealed affidavit `core/v1` [`Receipt`] from a [`DeletionReceipt`].
///
/// The chain is a complete, append-only provenance of one deletion execution:
///
/// * Event `0` (`deletion.execution.recorded`) commits to the execution summary
///   (version, timestamps, volume free-space samples) and anchors the
///   `delete_receipt` object the rest of the chain refers to.
/// * Events `1..=n` (one per [`crate::domain::receipt::DeletionResult`]) commit
///   to each result. Their `filesystem_object` is identified by the BLAKE3 hash
///   of the path, never the raw path — so the affidavit receipt is privacy-clean
///   (no absolute paths) yet still verifiable against the original receipt, which
///   anyone holding it can recompute. This is affidavit's zero-knowledge stance:
///   commit, don't expose.
///
/// The resulting receipt is sealed via [`ChainAssembler`] and always passes
/// [`certify`] — emitting it is how `delete` discharges the project invariant
/// *never increase destructive power without simultaneously increasing receipts.*
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::affidavit::{build_deletion_affidavit, certify};
/// use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
///
/// // Positive case: a real execution record yields an ACCEPTED affidavit receipt.
/// let dr = DeletionReceipt::new(
///     "deletion-chain-001".to_string(), 0, 1, 2,
///     vec![DeletionResult {
///         path: "/Users/u/dev/proj/target".into(),
///         status: DeletionStatus::Deleted,
///         error: None,
///         blake3_hash: None,
///         bytes_freed: 1024,
///     }],
///     None, None,
/// );
/// let receipt = build_deletion_affidavit(&dr);
/// // header event + one per result
/// assert_eq!(receipt.events.len(), 2);
/// assert!(certify(&receipt).accepted);
///
/// // Privacy case: no raw absolute path leaks into the affidavit receipt.
/// let json = String::from_utf8(osx_clnr::domain::affidavit::serialize_receipt(&receipt)).unwrap();
/// assert!(!json.contains("/Users/u/dev/proj/target"));
///
/// // Negative case: an empty execution still seals to a valid (header-only) chain.
/// let empty = DeletionReceipt::new("c".to_string(), 0, 1, 2, vec![], None, None);
/// let r = build_deletion_affidavit(&empty);
/// assert_eq!(r.events.len(), 1);
/// assert!(certify(&r).accepted);
/// ```
pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> Receipt {
    let rec = &receipt.execution_record;
    let mut asm = ChainAssembler::new();

    // Content address of the whole execution record anchors every event's
    // `delete_receipt` reference.
    let receipt_addr = Blake3Hash::from_bytes(&canonical_bytes(rec)).0;

    // Header event: commit to the execution summary (everything but the
    // per-item results, which get their own events).
    #[derive(Serialize)]
    struct ExecutionSummary<'a> {
        version: u32,
        plan_created_unix: u64,
        execution_started_unix: u64,
        execution_completed_unix: u64,
        result_count: usize,
        available_before: &'a Option<u64>,
        available_after: &'a Option<u64>,
    }
    let summary = ExecutionSummary {
        version: rec.version,
        plan_created_unix: rec.plan_created_unix,
        execution_started_unix: rec.execution_started_unix,
        execution_completed_unix: rec.execution_completed_unix,
        result_count: rec.results.len(),
        available_before: &rec.available_before,
        available_after: &rec.available_after,
    };
    asm.append(OperationEvent {
        id: "deletion-execution".to_string(),
        seq: 0,
        event_type: "deletion.execution.recorded".to_string(),
        objects: vec![ObjectRef {
            id: receipt_addr.clone(),
            obj_type: OBJ_RECEIPT.to_string(),
            qualifier: Some("self".to_string()),
        }],
        payload_commitment: Blake3Hash::from_bytes(&canonical_bytes(&summary)),
    });

    // One event per deletion result.
    for (i, result) in rec.results.iter().enumerate() {
        let path_id = Blake3Hash::from_bytes(result.path.to_string_lossy().as_bytes()).0;
        asm.append(OperationEvent {
            id: format!("del-{i}"),
            seq: (i + 1) as u64,
            event_type: status_event_type(result.status).to_string(),
            objects: vec![
                ObjectRef {
                    id: path_id,
                    obj_type: OBJ_FILESYSTEM.to_string(),
                    qualifier: Some("target".to_string()),
                },
                ObjectRef {
                    id: receipt_addr.clone(),
                    obj_type: OBJ_RECEIPT.to_string(),
                    qualifier: Some("receipt".to_string()),
                },
            ],
            payload_commitment: Blake3Hash::from_bytes(&canonical_bytes(result)),
        });
    }

    asm.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};

    fn sample_receipt() -> DeletionReceipt {
        DeletionReceipt::new(
            "deletion-chain-001".to_string(),
            100,
            101,
            102,
            vec![
                DeletionResult {
                    path: "/Users/dev/proj/target".into(),
                    status: DeletionStatus::Deleted,
                    error: None,
                    blake3_hash: None,
                    bytes_freed: 1024,
                },
                DeletionResult {
                    path: "/Users/dev/proj/node_modules".into(),
                    status: DeletionStatus::SkippedMissing,
                    error: None,
                    blake3_hash: None,
                    bytes_freed: 0,
                },
            ],
            None,
            None,
        )
    }

    #[test]
    fn empty_chain_equals_genesis() {
        assert_eq!(recompute_chain(&[]), genesis_hash());
        assert_eq!(ChainAssembler::new().finalize().chain_hash, genesis_hash());
    }

    #[test]
    fn deletion_receipt_certifies() {
        let receipt = build_deletion_affidavit(&sample_receipt());
        // header + one event per result
        assert_eq!(receipt.events.len(), 3);
        assert_eq!(receipt.format_version, FORMAT_VERSION);
        assert!(certify(&receipt).accepted);
    }

    #[test]
    fn canonical_json_round_trips_through_forgery_check() {
        let receipt = build_deletion_affidavit(&sample_receipt());
        let bytes = serialize_receipt(&receipt);
        // A faithfully serialized receipt re-verifies on the way back in.
        let back: Receipt = serde_json::from_slice(&bytes).expect("honest receipt deserializes");
        assert_eq!(back, receipt);
    }

    #[test]
    fn tampered_json_is_rejected_at_deserialize() {
        let receipt = build_deletion_affidavit(&sample_receipt());
        let json = String::from_utf8(serialize_receipt(&receipt)).unwrap();
        // Forge an event_type without re-sealing the chain.
        let forged = json.replace("artifact.deleted", "artifact.kept");
        let result: Result<Receipt, _> = serde_json::from_str(&forged);
        assert!(result.is_err(), "forged receipt must fail deserialization");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("chain hash mismatch"));
    }

    #[test]
    fn event_types_track_status_truthfully() {
        let receipt = build_deletion_affidavit(&sample_receipt());
        assert_eq!(receipt.events[0].event_type, "deletion.execution.recorded");
        assert_eq!(receipt.events[1].event_type, "artifact.deleted");
        assert_eq!(receipt.events[2].event_type, "artifact.skipped_missing");
    }

    #[test]
    fn no_absolute_paths_leak_into_receipt() {
        let receipt = build_deletion_affidavit(&sample_receipt());
        let json = String::from_utf8(serialize_receipt(&receipt)).unwrap();
        assert!(!json.contains("/Users/dev/proj"));
    }
}
