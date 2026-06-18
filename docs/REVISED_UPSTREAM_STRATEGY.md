# REVISED: Upstream Contribution Strategy

## The Discovery

Upon examining the actual affidavit repository, we discovered that **affidavit already implements most of what we re-implemented in Pentecost**:

### What Affidavit Already Has

✅ **chain.rs** (10.1 KB)
- `Blake3Hash` with `from_bytes()`, `from_hex()`, `as_hex()`
- `ChainAssembler` with append/events/len/is_empty/finalize
- `recompute_chain()` for verification
- `genesis_hash()` and `fold_event()`
- `FORMAT_VERSION` and `GENESIS_SEED` constants
- Custom `Deserialize` impl that re-verifies chain hashes

✅ **verifier.rs** (7-stage pipeline)
- `stage_decode` - Receipt structurally present
- `stage_check_format` - Version matches standard
- `stage_chain_integrity` - Chain hash recomputes correctly
- `stage_continuity` - Seq numbers strictly increasing, no gaps
- `stage_verify_commitments` - All commitments well-formed
- `stage_evaluate_profile` - CoreV1 profile requirements
- `emit_verdict` - Final ACCEPTED/REJECTED decision
- Returns `Verdict { accepted, profile, outcomes, reason }`

✅ **admission.rs**
- `Evidence<Receipt, Admitted, AffidavitReceiptChain>` typestate pattern
- `AffidavitRefusal` enum with named refusals
- `admit()` function: Receipt → `Evidence<Admitted>` or `Refusal`
- OCEL law projection: `project_to_ocel()`
- Integration with `wasm4pm-compat` for OCEL validation

✅ **types.rs** (629 lines)
- `Blake3Hash` struct
- `ObjectRef` with id/obj_type/qualifier
- `OperationEvent` with id/seq/event_type/objects/payload_commitment
- `Receipt` sealed struct
- `Verdict` struct
- `CheckOutcome` struct
- `ProfileId` enum
- `AdmittedReceipt` type alias
- Custom `Deserialize` impl

✅ **ocel.rs**
- OCEL v2 event/object structure
- Integration with wasm4pm-compat
- Event→Object linking

✅ **Additional capabilities already in affidavit:**
- GPU verification (`1000x_gpu_verifier.rs`)
- SBOM compliance tracking
- Process mining integration (`mining.rs`, `discovery.rs`)
- LSP support for receipt editing
- Quality metrics and monitoring
- Mutation testing
- Post-quantum cryptography support
- Distributed receipt sharding
- Auto-remediation

### What Pentecost Duplicated

❌ **src/domain/affidavit.rs** (750 lines)
- We re-implemented Blake3Hash, ObjectRef, OperationEvent, Receipt, ChainAssembler, 7-stage verifier, Verdict, CheckOutcome, custom Deserialize
- This is **unnecessary code duplication**
- We should delete this file and use `affidavit` as a dependency

---

## REVISED Strategy: Three Phases (Not Four)

### Phase 1: Replace Pentecost's affidavit.rs with dependency (Week 1)

**Current state:**
```rust
// Pentecost src/domain/affidavit.rs (750 lines - DUPLICATES AFFIDAVIT)
use crate::domain::affidavit::{Blake3Hash, Receipt, ChainAssembler, verify};
```

**Target state:**
```rust
// Pentecost Cargo.toml
[dependencies]
affidavit = { git = "https://github.com/seanchatmangpt/affidavit", features = ["core", "discovery"] }

// Pentecost src/domain/affidavit_integration.rs (adapter layer, ~150 lines)
use affidavit::{Blake3Hash, Receipt, ChainAssembler, verifier};

pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> affidavit::Receipt {
    // Adapt Pentecost DeletionReceipt to affidavit Receipt
    // Build OperationEvents from deletion results
    // Assemble chain
}

pub fn certify(receipt: &affidavit::Receipt) -> affidavit::Verdict {
    // Delegate to affidavit::verifier::verify
}
```

**Actions:**
1. Delete `src/domain/affidavit.rs` (750 lines removed)
2. Add affidavit to Cargo.toml dependencies
3. Create small adapter module `src/domain/affidavit_integration.rs`
4. Update imports in nouns/delete.rs and nouns/receipt.rs
5. Run tests — should all pass (we're just swapping implementations)

**Result:** 
- Pentecost shrinks by 750 lines
- Gains affidavit's features (GPU verification, SBOM, etc.)
- Upstream (affidavit) gets validation from Pentecost as a production user
- Reduces maintenance burden (affidavit team owns the core engine)

### Phase 2: Extract Deletion-Specific Patterns as Macros/Traits (Week 2)

**Current state:**
```rust
// Pentecost domain/delete.rs, domain/receipt.rs, domain/plan.rs
// Filesystem-specific witnesses, adjudicators, OCEL builders
pub struct DeletionPlanAdjudicator { ... }
pub struct PlanSafetyWitness { ... }
// ~200 lines of filesystem policy
```

**Target state:**
```rust
// Pentecost remains the same — no changes needed
// This is already in the right shape (macOS-specific)
```

**Why:** Deletion is filesystem-specific. The Witness/Adjudicator pattern in Pentecost is already properly encapsulated. There's no need to upstream it; it stays in Pentecost as a worked example.

**Benefit:** Pentecost becomes a reference implementation showing how to use affidavit for filesystem cleanup.

### Phase 3: Create process-governance as a thin adapter (Week 3 - OPTIONAL)

**IF we want to share the pattern with other domains:**

Create a new public crate `process-governance` that provides:
- Generic `Operation` trait
- Generic `ExecutionPlan<T>` / `ExecutionReceipt<T>`
- Generic `ExecutionAdmittor<T, W>` pattern

This crate **uses affidavit** as its cryptographic backend:

```rust
// process-governance/src/lib.rs
use affidavit::{Receipt, ChainAssembler, OperationEvent};

pub trait Operation {
    fn to_operation_event(&self) -> OperationEvent;
}

pub struct ExecutionReceipt<T: Operation> {
    pub affidavit_receipt: affidavit::Receipt,
    pub operations: Vec<T>,
}
```

Then external projects (database, cache, etc.) can:
```rust
use process_governance::*;
use affidavit::verifier;

// Implement Operation for their domain
impl Operation for DatabaseRecordDeletion { ... }

// Use the framework
let plan = ExecutionPlan::new(vec![...]);
let receipt = execute(plan)?;
verifier::verify(&receipt.affidavit_receipt)?;
```

**Decision:** This is nice-to-have but not critical. Pentecost works perfectly without it.

---

## What We Actually Should Do

### The Simple Path (Recommended)

**Week 1 ONLY:**

1. Delete `src/domain/affidavit.rs` (750 lines)
2. Add `affidavit` to Cargo.toml
3. Create 150-line adapter in `src/domain/affidavit_integration.rs`
4. Update nouns/delete.rs and nouns/receipt.rs imports
5. Run tests
6. Push to branch

**Result:**
- Pentecost is leaner (1600 → 850 lines of domain logic)
- Uses upstream affidavit directly
- Gains affidavit's ecosystem (GPU verification, SBOM, LSP, etc.)
- Validates affidavit in production
- Reduces maintenance (affidavit team owns core)
- Takes **1 day**

### No Upstream Contribution Needed

The original strategy of "extracting affidavit-core as a PR" is **unnecessary**. Affidavit is already public and already has a clean core. We just need to **use it** instead of re-implementing it.

---

## The Adapter Layer (affidavit_integration.rs)

```rust
//! Bridge between Pentecost's filesystem operations and affidavit's generic receipt model.

use crate::domain::receipt::DeletionReceipt;
use affidavit::{Blake3Hash, ChainAssembler, ObjectRef, OperationEvent, Receipt};
use std::collections::HashMap;

/// Build an affidavit Receipt from a DeletionReceipt.
///
/// Maps:
/// - DeletionReceipt.execution_record → OperationEvents
/// - DeletionResult.path → ObjectRef (using BLAKE3 hash)
/// - DeletionResult.status → event_type + payload_commitment
pub fn build_deletion_affidavit(receipt: &DeletionReceipt) -> Receipt {
    let mut assembler = ChainAssembler::new();
    
    // Header event: the deletion operation itself
    let header_event = OperationEvent {
        id: "deletion-header".to_string(),
        seq: 0,
        event_type: "deletion_plan_executed".to_string(),
        objects: vec![
            ObjectRef {
                id: Blake3Hash::from_bytes(
                    receipt.execution_record.version.as_bytes()
                ).as_hex().to_string(),
                obj_type: "deletion_plan".to_string(),
                qualifier: None,
            },
        ],
        payload_commitment: Blake3Hash::from_bytes(
            format!("{:?}", receipt.execution_record).as_bytes()
        ),
    };
    assembler.append(header_event).unwrap();
    
    // Per-result events
    for (i, result) in receipt.execution_record.results.iter().enumerate() {
        let event = OperationEvent {
            id: format!("deletion-{}", i),
            seq: (i + 1) as u64,
            event_type: format!("{:?}", result.status),
            objects: vec![ObjectRef {
                id: Blake3Hash::from_bytes(result.path.to_string_lossy().as_bytes())
                    .as_hex()
                    .to_string(),
                obj_type: "filesystem_object".to_string(),
                qualifier: Some("deleted".to_string()),
            }],
            payload_commitment: result.blake3_hash
                .as_ref()
                .cloned()
                .unwrap_or_else(|| Blake3Hash::from_bytes(b"")),
        };
        assembler.append(event).unwrap();
    }
    
    assembler.finalize()
}

/// Verify an affidavit Receipt using the 7-stage pipeline.
pub fn certify(receipt: &Receipt) -> affidavit::Verdict {
    affidavit::verifier::verify(receipt)
}

/// Verify and admit a receipt, returning an Admitted receipt or refusal.
pub fn admit(receipt: Receipt) -> Result<affidavit::AdmittedReceipt, affidavit::admission::AffidavitRefusal> {
    affidavit::admission::admit(receipt)
}
```

This is the **only new code** needed.

---

## Updated Cargo.toml

```toml
[dependencies]
# ... existing deps ...
affidavit = { git = "https://github.com/seanchatmangpt/affidavit", features = ["core", "discovery", "conformance"] }
```

**Features:**
- `core`: Basic receipt assembly and verification (always needed)
- `discovery`: Process mining integration (optional, but nice for future work)
- `conformance`: Conformance checking (optional)

---

## What Changes in Pentecost Code

### src/domain/mod.rs
```rust
// BEFORE
pub mod affidavit;

// AFTER
pub mod affidavit_integration;
```

### src/nouns/delete.rs
```rust
// BEFORE
use crate::domain::affidavit;

// AFTER
use crate::domain::affidavit_integration;

// ... later in code ...
let affidavit_receipt = affidavit_integration::build_deletion_affidavit(&receipt);
let verdict = affidavit_integration::certify(&affidavit_receipt);
```

### src/nouns/receipt.rs
```rust
// BEFORE
use crate::domain::affidavit;

// AFTER
use crate::domain::affidavit_integration;

// ... in Verify handler ...
let affidavit_receipt = affidavit_integration::build_deletion_affidavit(&receipt_data);
let verdict = affidavit_integration::certify(&affidavit_receipt);

// ... in Certify handler ...
let affidavit_receipt = affidavit_integration::build_deletion_affidavit(&receipt_data);
let verdict = affidavit_integration::certify(&affidavit_receipt);
```

---

## Testing Plan

1. Build: `cargo build` — should succeed with affidavit dependency
2. Tests: `cargo test` — all tests should pass (we're swapping implementations, not changing behavior)
3. Doctests: `cargo test --doc` — should pass (doctests in affidavit are the reference)
4. Clippy: `cargo clippy` — should pass
5. Doctor gates: All gates should pass (same domain purity, same privacy, same OCEL output)

---

## Benefits of This Approach

### For Pentecost:
- ✅ Delete 750 lines of duplicated code
- ✅ Reduce maintenance (affidavit team owns core crypto)
- ✅ Gain ecosystem features (GPU, SBOM, LSP, etc.) automatically
- ✅ More focused codebase (macOS-specific only)
- ✅ Cleaner dependency model

### For Affidavit:
- ✅ Gain Pentecost as a production user
- ✅ Get validation of core/v1 in the wild
- ✅ Potential to add Pentecost as a showcase example

### For the Community:
- ✅ One canonical implementation of core/v1 (not two)
- ✅ Clear separation: affidavit (universal) vs. Pentecost (macOS-specific)
- ✅ Easier for other projects to adopt affidavit for their domains

---

## Timeline and Effort

| Activity | Time | Complexity |
|----------|------|-----------|
| Review affidavit API | 1 hour | Low |
| Create adapter layer | 2 hours | Low |
| Update imports | 1 hour | Low |
| Test and validate | 2 hours | Low |
| **Total** | **6 hours** | **Low** |

**Estimated completion:** This week.

---

## No Need for process-governance

The original strategy proposed extracting a `process-governance` crate. **This is premature abstraction.** Pentecost's deletion-specific code is already well-encapsulated:

- `DeletionPlanAdjudicator` is macOS-specific (snapshot exclusions, system paths)
- `PlanSafetyWitness` is filesystem-specific
- The OCEL builders are deletion-specific

These are not generic enough to warrant a shared crate. If/when a second domain (database, cache) emerges, we can extract patterns then. For now, Pentecost is a reference implementation.

---

## Conclusion

**The simplest solution is the correct one:** Pentecost should use affidavit as a dependency, not re-implement it. This reduces code duplication, improves maintainability, and provides better validation of affidavit's production readiness.

The 750-line affidavit.rs should be deleted and replaced with a thin 150-line adapter. Everything else remains the same.
