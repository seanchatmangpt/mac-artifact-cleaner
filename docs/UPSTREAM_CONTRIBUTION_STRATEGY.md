# Upstream Contribution Strategy: Decoupling Pentecost from macOS

## The Abstraction Problem

Pentecost currently couples three layers:

```
Layer 1: Universal Governance Framework (Process Mining + Cryptography)
  ├─ OCEL event emission
  ├─ BLAKE3 provenance chains
  ├─ 7-stage verification pipeline
  ├─ Typestate admission pattern
  └─ RL-ready MDP formulation
  
Layer 2: Destructive Operation Abstraction (Generic)
  ├─ ExecutionPlan (was DeletionPlan)
  ├─ ExecutionReceipt (was DeletionReceipt)
  ├─ Admission control pattern
  └─ Event emission for any operation
  
Layer 3: macOS Filesystem Specifics (NOT upstream)
  ├─ APFS snapshot management
  ├─ Time Machine exclusion via tmutil
  ├─ POSIX filesystem operations
  ├─ macOS system path heuristics
  └─ oclnr CLI
```

Only **Layer 1 and 2** are universal. **Layer 3** stays in Pentecost.

## Proposed Upstream Crates

### Crate 1: `affidavit-core` (to affidavit project)

**Current location:** `src/domain/affidavit.rs` (750 lines)

**Proposed:** Contribute as `affidavit/crates/core-v1/src/lib.rs`

**Content:**
```rust
pub mod chain;      // BLAKE3Hash, rolling hash computation
pub mod events;     // OperationEvent, ObjectRef (generic)
pub mod receipt;    // Receipt, Verdict, ChainAssembler (sealed, unforgeable)
pub mod verify;     // 7-stage pipeline: decode, format, chain_integrity, etc.
pub mod canonical;  // Canonical JSON encoding (deterministic key ordering)

// No macOS, no filesystem, no OS calls
```

**API Surface:**
```rust
pub struct Blake3Hash(String);
pub struct ObjectRef { object_type: String, object_id: Blake3Hash }
pub struct OperationEvent { /* generic operation data */ }
pub struct Receipt { /* sealed receipt */ }
pub struct ChainAssembler { /* builder */ }
pub fn verify_receipt(receipt: &Receipt) -> Verdict;
```

**Independence:** Zero dependencies on Pentecost code. Pure Rust.

**Contribution:** Submit as PR to https://github.com/seanchatmangpt/affidavit

**Benefits for affidavit:**
- Upstream gets the tested, 7-stage pipeline
- Other projects (database, cache, distributed systems) can use it

---

### Crate 2: `process-governance` (new standalone crate)

**Purpose:** Generic framework for receipted execution of any destructive operation.

**Structure:**
```
process-governance/
  src/
    admission/     # Typestate pattern (Evidence<T, State, Witness>)
    execution/     # ExecutionResult, ExecutionReceipt<T>
    conformance/   # LTL constraint checking
    ocel/          # OCEL v2 emission and validation
    mrl/           # MDP formulation for RL
```

**Key Types:**

```rust
// Generic execution plan (replaces DeletionPlan)
pub struct ExecutionPlan<T: Operation> {
    pub items: Vec<T>,
    pub created_unix: u64,
    pub justification: String,
}

// Generic operation trait
pub trait Operation: Serialize + Deserialize {
    fn target_id(&self) -> ObjectRef;
    fn operation_type(&self) -> String;
    fn size_bytes(&self) -> u64;
}

// Generic execution result (replaces DeletionReceipt)
pub struct ExecutionReceipt<T: Operation> {
    pub execution_record: ExecutionRecord<T>,
    pub affidavit: affidavit_core::Receipt,
}

// Generic admission gate
pub struct ExecutionAdmittor<T: Operation, W: Witness> {
    // Validates execution plan before execution
}

// Generic conformance checker (replaces Gall Pipeline)
pub trait ConformanceRule<T: Operation> {
    fn check(&self, trace: &ExecutionTrace<T>) -> ConformanceResult;
}
```

**No macOS code.** No filesystem operations. Just the **pattern**.

**Usage Example:**
```rust
// In some other project (database cleanup, cache eviction, etc.)
use process_governance::{ExecutionPlan, ExecutionAdmittor, Operation};

#[derive(Serialize, Deserialize)]
struct DatabaseRecordDeletion {
    record_id: String,
    reason: String,
}

impl Operation for DatabaseRecordDeletion {
    fn target_id(&self) -> ObjectRef { ... }
    fn operation_type(&self) -> String { "db_record_deletion".to_string() }
    fn size_bytes(&self) -> u64 { 4096 } // varies by DB
}

let plan = ExecutionPlan::new(vec![...]);
let admitted = DatabaseAdmittor::admit(plan)?; // Custom witness
let receipt = execute(admitted)?;  // Generic execution
verify_receipt(&receipt.affidavit)?;  // Use affidavit-core
```

---

### Crate 3: `wasm4pm-compat` (extend existing)

**Current status:** Likely already in affidavit project as the WASM4PM integration point.

**Enhancement:** Add trait bounds for `process-governance` types.

```rust
// In wasm4pm-compat
pub trait ProcessMiningCertifiable: Serialize {
    fn to_ocel_events(&self) -> Vec<OcelEvent>;
}

impl<T: Operation> ProcessMiningCertifiable for ExecutionReceipt<T> { ... }
```

**Benefit:** Any `process-governance` user can emit OCEL and use process mining.

---

## Refactored Pentecost Architecture

After upstream contributions, Pentecost becomes:

```
pentecost/
  Cargo.toml
    [dependencies]
    affidavit-core = { git = "https://github.com/seanchatmangpt/affidavit" }
    process-governance = { git = "https://github.com/yourusername/process-governance" }
    wasm4pm-compat = { git = "https://github.com/seanchatmangpt/affidavit" }

  src/
    domain/
      ├─ artifact.rs      # macOS-specific artifact detection
      ├─ audit.rs         # macOS filesystem scanning
      ├─ tool_roots.rs    # macOS tool root definitions
      ├─ time.rs          # Time Machine snapshot logic
      ├─ ocel.rs          # Pentecost-specific OCEL builders
      │                   #   (wraps process-governance events)
      ├─ delete.rs        # Filesystem-specific deletion logic
      │                   #   (implements Operation trait)
      ├─ plan.rs          # Pentecost DeletionPlan
      │                   #   (type alias: type DeletionPlan = ExecutionPlan<FilesystemOp>)
      ├─ receipt.rs       # Pentecost DeletionReceipt
      │                   #   (type alias: type DeletionReceipt = ExecutionReceipt<FilesystemOp>)
      └─ policy.rs        # macOS-specific deletion policies
    
    integration/
      ├─ fs.rs            # POSIX filesystem operations
      ├─ tmutil.rs        # macOS tmutil subprocess calls
      ├─ doctor.rs        # Diagnostic checks
      └─ progress.rs      # Progress bars
    
    nouns/
      ├─ delete.rs        # Uses ExecutionAdmittor<DeletionPlan>
      ├─ receipt.rs       # Uses process-governance receipt verification
      ├─ audit.rs
      ├─ plan.rs
      └─ ...
```

**Key insight:** Pentecost is now just **glue and macOS-specific business logic**. The universal framework is upstream.

---

## Migration Path (Phased)

### Phase 1: Extract affidavit-core (Weeks 1–2)

**Goal:** Move `src/domain/affidavit.rs` to `affidavit-core` as a standalone crate.

**Steps:**
1. Clone affidavit project locally
2. Create `affidavit/crates/core-v1/`
3. Copy `affidavit.rs`, rename to `lib.rs`
4. Remove any Pentecost-specific imports
5. Add comprehensive README and examples
6. Write unit tests (already in doctests)
7. Open PR to affidavit: "Add core/v1 cryptographic kernel"

**Result:** `affidavit-core = { git = "...", version = "0.1" }`

**Impact on Pentecost:** Change imports:
```rust
// Before
use crate::domain::affidavit::*;

// After
use affidavit_core::*;
```

### Phase 2: Extract process-governance (Weeks 3–4)

**Goal:** Create a new crate with generic operation/execution/admission pattern.

**Steps:**
1. Create new repo: `process-governance`
2. Define `Operation` trait
3. Extract `Evidence<T, State, Witness>` pattern (move from `wasm4pm-compat`)
4. Create `ExecutionPlan<T>`, `ExecutionReceipt<T>`
5. Add `ExecutionAdmittor<T, W>` (generic over witness type)
6. Add conformance checking framework
7. Integrate `affidavit-core` for cryptographic binding
8. Write docs and examples (database, cache, distributed systems)

**Result:** `process-governance = { git = "...", version = "0.1" }`

**Impact on Pentecost:** Redefine types:
```rust
// Before
pub struct DeletionPlan { ... }
pub struct DeletionReceipt { ... }

// After
pub type DeletionPlan = process_governance::ExecutionPlan<FilesystemOperation>;
pub type DeletionReceipt = process_governance::ExecutionReceipt<FilesystemOperation>;
```

### Phase 3: Refactor Pentecost to use upstream crates (Weeks 5–6)

**Goal:** Delete duplicated code, use upstream types.

**Steps:**
1. Remove `src/domain/affidavit.rs` (moved to affidavit-core)
2. Simplify `src/domain/receipt.rs` (now wraps `ExecutionReceipt<T>`)
3. Simplify `src/domain/plan.rs` (now wraps `ExecutionPlan<T>`)
4. Update `src/domain/delete.rs` to implement `Operation` trait
5. Update `src/nouns/*.rs` to use upstream admission/verification APIs
6. Add Pentecost-specific OCEL builders (still in domain, macOS-specific)
7. Add macOS-specific witnesses and adjudicators

**Result:** Pentecost shrinks from ~2500 lines of domain logic to ~800 lines (only macOS specifics).

### Phase 4: Contribute examples back to upstream (Week 7+)

**Goal:** Upstream crates are validated by a production user (Pentecost).

**Steps:**
1. Add Pentecost as an example in `process-governance` README
2. Contribute integration tests (real-world deletion traces)
3. Document lessons learned: "How to extend process-governance for a new domain"
4. Open issues on affidavit/process-governance for improvements discovered in production

---

## Example: Database Record Deletion Framework

To validate the abstraction, we should create a toy example: **Receipted database cleanup.**

```rust
// In process-governance examples/

use process_governance::*;
use affidavit_core::*;

#[derive(Serialize, Deserialize)]
pub struct DatabaseRecordDeletion {
    pub record_id: String,
    pub table_name: String,
    pub reason: String,  // GDPR request, retention policy, etc.
    pub size_bytes: u64,
}

impl Operation for DatabaseRecordDeletion {
    fn target_id(&self) -> ObjectRef {
        ObjectRef {
            object_type: "db_record".to_string(),
            object_id: Blake3Hash::from_bytes(self.record_id.as_bytes()),
        }
    }
    
    fn operation_type(&self) -> String {
        "db_record_deletion".to_string()
    }
    
    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

// Define compliance witness for databases
pub struct GDPRWitness {
    pub request_id: String,
    pub requestor: String,
    pub authorization_timestamp: u64,
}

impl Witness for GDPRWitness {
    fn validate(&self, operation: &DatabaseRecordDeletion) -> Result<()> {
        // Check that GDPR request exists
        // Check that retention period expired
        // etc.
        Ok(())
    }
}

// Usage:
fn main() -> anyhow::Result<()> {
    let plan = ExecutionPlan::new(vec![
        DatabaseRecordDeletion { ... },
        DatabaseRecordDeletion { ... },
    ]);
    
    let witness = GDPRWitness { ... };
    let admitted = DatabaseAdmittor::new(witness).admit(plan)?;
    
    // Execute deletions from DB
    let receipt = execute_from_plan(&admitted)?;
    
    // Emit cryptographic proof
    verify_receipt(&receipt.affidavit)?;
    
    // GDPR auditor can later verify:
    // receipt verify --receipt gdpr-deletions.affidavit.json
    // Result: ✅ ACCEPTED — 47 records deleted per GDPR request xyz-123
    
    Ok(())
}
```

This example validates that the abstraction works for a completely different domain.

---

## Benefits of This Abstraction

### For affidavit project:
- Gains `affidavit-core` as a standalone, production-tested crate
- Gains `process-governance` as a generic framework for other domains
- Gains real-world validation (Pentecost proves it works)
- Gains examples beyond filesystem (database, cache, distributed systems)

### For Pentecost:
- Shrinks from ~2500 domain lines to ~800 (removes boilerplate)
- Upgrades to upstream maintenance for core logic
- Can focus on macOS UX/features instead of infrastructure
- Cleanly separated: macOS-specific vs. universal

### For other projects:
- Process cleanup tools (Docker, K8s, CI/CD caches)
- Database deletion compliance (GDPR, data retention policies)
- Cache eviction frameworks (Redis, memcached)
- Log rotation and archival
- Distributed system membership management

**All can use the same framework without reimplementing receipts, conformance checking, or RL infrastructure.**

---

## Concrete PR Outline

### PR 1: affidavit-core (to affidavit repo)

```
Title: Add affidavit core/v1 cryptographic kernel as standalone crate

Summary:
- Extracts BLAKE3 chain hashing and 7-stage verification from Pentecost
- Adds as affidavit/crates/core-v1
- 100% domain-agnostic; zero macOS/filesystem code
- Includes 16+ doctests, production validation from Pentecost

API:
- pub struct Blake3Hash
- pub struct ObjectRef
- pub struct OperationEvent
- pub struct Receipt (sealed, unforgeable)
- pub fn verify_receipt(receipt: &Receipt) -> Verdict

Dependencies: None (just serde, blake3, serde_json)
```

### PR 2: process-governance (new repo)

```
Title: Process Governance Framework — Receipted Execution for Any Operation

Summary:
- Generic typestate-enforced admission framework
- Works with any destructive operation (deletion, eviction, etc.)
- Integrates affidavit-core for cryptographic binding
- Includes database deletion example; ready for cache/distributed-system examples

API:
- pub trait Operation
- pub struct ExecutionPlan<T>
- pub struct ExecutionReceipt<T>
- pub trait Witness
- pub struct ExecutionAdmittor<T, W>
- pub fn verify_receipt(receipt: &Receipt) -> Verdict

Dependencies: affidavit-core, serde, blake3
```

### PR 3: Pentecost Integration (to pentecost repo)

```
Title: Refactor to use upstream affidavit-core and process-governance

Summary:
- Removes 1600 lines of duplicated code
- Adds dependencies on affidavit-core and process-governance
- Defines FilesystemOperation implementing Operation trait
- Defines FilesystemWitness implementing Witness trait
- Remaining 800 lines of domain code are 100% macOS-specific

Result: Pentecost is now a thin wrapper around universal framework
```

---

## Timeline and Effort

| Phase | Effort | Timeline | Result |
|-------|--------|----------|--------|
| 1: affidavit-core | 1 week | Week 1–2 | Upstream gains tested crate |
| 2: process-governance | 2 weeks | Week 3–4 | New framework available |
| 3: Pentecost refactor | 1 week | Week 5–6 | Pentecost uses upstream |
| 4: Examples & validation | 1 week | Week 7–8 | Community examples ready |

**Total:** ~4 weeks, full decoupling achieved.

---

## Success Criteria

After completion:

1. **Pentecost has zero dependencies on affidavit source code** (uses only published crates)
2. **Pentecost domain logic is 100% macOS-specific** (filesystem, tmutil, snapshots)
3. **affidavit-core is used by 2+ external projects** (validates abstraction)
4. **process-governance has examples for 3+ domains** (filesystem, database, cache)
5. **All tests pass, all doctests pass, all gates pass**

This is the clean architecture Pentecost deserves. 🚀
