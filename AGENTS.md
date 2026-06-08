# AGENTS.md — Developer Agent Playbook

This document is the operating contract for AI developer agents and human pair-programmers working on or with `osx-clnr`.

`osx-clnr` is not a raw cleanup script. It is a **plan-bound macOS developer disk auditor and cleanup utility** that observes first, emits evidence, manufactures reviewable plans, executes only from those plans, and records receipts.

---

## 0. Non-Negotiable Operating Law

> **Never increase destructive power without simultaneously increasing receipts.**

Every destructive capability must be preceded by:

1. A read-only observation phase.
2. A typed domain decision.
3. A reviewable plan artifact.
4. A delete phase that cannot scan.
5. A receipt that records every attempted consequence.

The scanner may propose. The plan may authorize. The delete executor may only act on the plan.

```text
observation → classification → evidence → plan → plan-bound actuation → receipt → verification
```

No agent may add a path from a live scan directly into deletion behavior.

---

## 1. System Philosophy & Architecture

`osx-clnr` follows **Gall Checkpoint development**: complex behavior is admitted only after a simpler working layer has produced operational evidence.

The execution-trust pipeline is:

```text
filesystem observation
  → artifact classification
  → traversal barriers
  → root-tool inventory
  → age/size/update evidence
  → OCEL v2 report
  → reviewable cleanup plan
  → deletion from plan only
  → deletion receipt
  → Time Machine / APFS snapshot verification
```

### Core Architectural Rule

```text
CLI validates.
Domain computes.
Integration connects.
Receipts prove.
```

The CLI layer must be thin. The domain layer must be pure, typed, testable Rust. Boundary formats such as JSON and OCEL v2 JSON are emitted only at the external interface.

---

## 2. Prompt & Agent Instruction Innovations

This repository is designed for AI developer agents. Agents must reason through the following operating frame before making changes.

### 2.1 Yes-And Operational Alignment

When given a new requirement, first preserve the current lawful pipeline, then extend it.

Bad pattern:

```text
User asks for more cleanup → add more rm -rf paths.
```

Correct pattern:

```text
User asks for more cleanup
  → add observation
  → add classification
  → add report evidence
  → add plan support
  → add receipt support
  → only then admit deletion behavior
```

### 2.2 No Improvisational Destruction

Agents must never implement destructive behavior that depends on a fresh scan at delete time.

Allowed:

```text
osx-clnr --write-plan cleanup-plan.jsonocel
osx-clnr --delete-plan cleanup-plan.jsonocel --receipt deletion-receipt.jsonocel
```

Forbidden:

```text
osx-clnr --scan-and-delete
osx-clnr --delete --root ~
osx-clnr --delete-matching target
```

### 2.3 Receipt-Before-Confidence

A console message is not proof. A passing scan is not proof. A receipt is proof.

Every high-impact operation should emit or update an artifact that can be reviewed after the process completes.

### 2.4 Bounded Promotion

Agents must not jump directly from user intention to broad capability. Each new power needs a Gall Checkpoint.

Use this format when introducing major behavior:

```text
Checkpoint:
Capability:
Evidence:
Constraint:
Receipt:
Promotion Rule:
```

### 2.5 Boundary Evidence, Not Internal Drift

Internal state should remain typed Rust structs and enums. JSON/OCEL are boundary projections for review, interchange, and audit. Do not let loosely typed external documents become the internal control plane.

---

## 3. Gall Checkpoints

| ID | Name | Capability | Hard Constraint | Receipt |
|---|---|---|---|---|
| G0 | Simple Artifact Cleaner | Finds rebuildable artifacts | No deletion without dry run | Console dry-run summary |
| G1 | Language & Project Detection | Node, Python, Rust, Java, Go, Elixir, Erlang, Next, Nuxt, etc. | Delete candidates must be rooted in detected project context | Candidate summary |
| G2 | Traversal Barriers | Treats heavy artifact dirs as leaf buckets | Do not traverse `node_modules`, `target`, `.next`, `_build`, `.venv`, etc. during artifact scans | Barrier counters / audit events |
| G3 | Dry-Run Plan File | Writes exact deletion plan | Delete mode consumes only a saved plan | `cleanup-plan.json` / `cleanup-plan.jsonocel` |
| G4 | Disk Inventory & UX Visibility | Byte attribution, spinners, rates, phase markers | Inventory is read-only | `disk-audit.jsonocel` |
| G5 | Time Machine & APFS Snapshot Awareness | Detects/thins local snapshot pinning | Snapshot thinning is explicit and receipted | Snapshot before/after receipt |
| G6 | Root-Tool Aging Analysis | Sizes `.gemini`, `.cargo`, `.cache`, Docker, model stores, toolchains, etc. | Recommendations only; no direct deletion | Tool-root OCEL report |
| G7 | OCEL v2 Reporting | Object-centric event evidence | Major operations must relate events to objects | OCEL v2 JSON report |
| G8 | Privacy / Redaction Gate | Redacts local machine evidence for examples | Real local reports must not be committed | Privacy scan receipt |
| G9 | Noun-Verb CLI Contract | Durable command surface via `clap-noun-verb` | CLI remains thin; domain logic stays pure | CLI doctor / doctests |

Promotion requires:

```text
capability exists
AND domain doctests pass
AND destructive power is not increased without a new receipt
AND outputs are reviewable
AND privacy gate passes
```

---

## 4. CLI Architecture Using `clap-noun-verb`

This project should use `clap-noun-verb` because the domain naturally decomposes into governed nouns and allowed verbs.

### 4.1 Nouns

```text
audit
artifact
tool-roots
plan
delete
receipt
snapshot
exclusion
ocel
privacy
doctor
```

### 4.2 Verbs

```text
audit run
audit summarize

artifact scan
artifact summarize

tool-roots audit
tool-roots summarize

plan build
plan inspect
plan validate
plan redact

delete execute
delete dry-run

receipt verify
receipt summarize

snapshot audit
snapshot thin

exclusion plan
exclusion apply

ocel validate
ocel summarize

privacy scan
privacy redact

doctor architecture
doctor substrate
doctor doctests
doctor privacy
```

### 4.3 CLI Layer Rule

A CLI command may parse, validate, route, and format. It must not own policy.

Thin wrapper pattern:

```rust
use clap_noun_verb_macros::verb;
use clap_noun_verb::Result;

/// # Arguments
/// * `from` - Input audit report [value_hint: FilePath]
/// * `output` - Output cleanup plan [value_hint: FilePath]
#[verb("build")]
fn cmd_build_plan(from: String, output: String) -> Result<PlanBuildOutput> {
    let request = BuildPlanRequest::new(from, output)?;
    let result = mac_artifact_cleaner::domain::plan::build_plan(request)?;
    Ok(result)
}
```

The command delegates immediately to a domain function.

---

## 5. Doctest Discipline

Doctests are mandatory for public domain functions. They are Gall locks: tiny executable claims that prevent agent drift.

### 5.1 Doctest Rule

Every public function in `src/domain/**` must have:

1. Module-level documentation.
2. At least one doctest.
3. A positive case.
4. A negative or refusal case when relevant.

### 5.2 Example: Artifact Leaf Classification

```rust
//! Artifact classification for rebuildable developer outputs.
//!
//! This module performs no filesystem mutation.
//!
//! # Examples
//!
//! ```
//! use mac_artifact_cleaner::domain::artifact::is_artifact_leaf_name;
//!
//! assert!(is_artifact_leaf_name("node_modules"));
//! assert!(is_artifact_leaf_name("target"));
//! assert!(is_artifact_leaf_name(".next"));
//! assert!(!is_artifact_leaf_name("src"));
//! ```

/// Returns true when a directory name should be treated as a rebuildable
/// artifact/dependency leaf during scanning.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::artifact::is_artifact_leaf_name;
///
/// assert!(is_artifact_leaf_name("node_modules"));
/// assert!(is_artifact_leaf_name("_build"));
/// assert!(is_artifact_leaf_name(".venv"));
/// assert!(!is_artifact_leaf_name("Documents"));
/// ```
pub fn is_artifact_leaf_name(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | ".next"
            | ".nuxt"
            | ".output"
            | ".vercel"
            | ".turbo"
            | ".vite"
            | ".parcel-cache"
            | ".venv"
            | "venv"
            | "env"
            | "_build"
            | "deps"
            | ".elixir_ls"
            | ".gradle"
            | "vendor"
            | ".bundle"
            | "build"
            | "dist"
            | "coverage"
            | "htmlcov"
            | "CMakeFiles"
            | "bin"
            | "obj"
    )
}
```

### 5.3 Required Test Commands

Agents must run:

```bash
cargo check --all-targets
cargo test
cargo test --doc
```

If behavior affects privacy or emitted reports, also run:

```bash
cargo test privacy
cargo test ocel
```

---

## 6. OCEL v2 Reporting Model

OCEL v2 JSON is the external evidence format for audit, plan, delete, receipt, and review operations.

### 6.1 Object Types

| Object Type | Meaning |
|---|---|
| `disk_audit` | One audit run |
| `scan_root` | A requested root path |
| `filesystem_object` | File or directory observed during audit |
| `artifact_candidate` | Rebuildable cleanup candidate |
| `tool_root` | Root-level tool/cache/model/worktree storage location |
| `deletion_plan` | Reviewable plan artifact |
| `delete_attempt` | One attempted deletion consequence |
| `delete_receipt` | Terminal receipt for delete execution |
| `snapshot_state` | Time Machine/APFS snapshot state |
| `tm_exclusion_plan` | Time Machine exclusion script or plan |

### 6.2 Event Types

| Event Type | Meaning |
|---|---|
| `disk_audit_started` | Audit began |
| `scan_root_started` | Root traversal began |
| `filesystem_object_observed` | File/dir observed |
| `traversal_barrier_applied` | Heavy subtree treated as leaf bucket |
| `bytes_attributed` | Bytes attributed to root/tool/artifact bucket |
| `tool_root_observed` | Root tool/cache store measured |
| `tool_root_review_proposed` | Human review recommended |
| `artifact_candidate_proposed` | Cleanup candidate emitted |
| `deletion_plan_written` | Plan artifact emitted |
| `deletion_from_plan_started` | Plan-bound deletion began |
| `artifact_deleted` | Delete succeeded |
| `artifact_delete_skipped` | Path missing or already gone |
| `artifact_delete_refused` | Guard blocked deletion |
| `artifact_delete_failed` | Delete attempted but failed |
| `deletion_completed` | Delete run ended |
| `snapshot_state_observed` | APFS/Time Machine state observed |
| `snapshot_thin_requested` | Explicit thinning requested |
| `tm_exclusion_plan_written` | Time Machine exclusions emitted |

### 6.3 Required OCEL Relationships

Every delete event must relate to:

```text
delete_receipt
  deletion_plan
  artifact_candidate
  filesystem_object
```

Every candidate event must relate to:

```text
disk_audit
  scan_root
  filesystem_object
```

Every tool-root review event must relate to:

```text
disk_audit
  tool_root
```

---

## 7. Technical Reference & Command Line Guide

### 7.1 Safe / Read-Only Commands

```text
--root <PATH>                         Directory to scan; repeatable; defaults to $HOME
--write-plan <PATH>                   Write dry-run deletion plan
--ocel-output <PATH>                  Emit OCEL v2 JSON evidence
--tool-roots                          Include major tool-root analysis
--min-tool-root-mb <MB>               Minimum size for tool-root reporting
--write-tm-exclusions <PATH>          Write Time Machine exclusion plan/script
--progress-every <N>                  Print heartbeat every N files/dirs
--verbose                             Print project roots and candidates
```

### 7.2 Scope Configuration

```text
--deps                                Include dependency dirs: node_modules, .venv, deps, vendor, target
--aggressive                          Include broader build outputs: dist, build, out
--include-hidden                      Include hidden home subtrees when supported
--tool-caches                         Include package/tool cache analysis when supported
```

### 7.3 Modifying Commands

```text
--delete-plan <PLAN_PATH>             Execute deletion strictly from a saved plan
--receipt <RECEIPT_PATH>              Write deletion receipt
```

Deletion mode must not accept `--root` as an authority source.

---

## 8. Root Tool Classifications

The tool identifies and catalogs root-level infrastructure folders.

| Path Relative to `$HOME` | Category | Default Disposition |
|---|---|---|
| `.gemini`, `.claude`, `.codex`, `.cursor`, `.continue` | `ai_tool_state` | `review` |
| `.vscode`, `.idea` | `editor_state` | `review` |
| `.cargo` | `rust_package_cache` | `review` |
| `.rustup` | `rust_toolchains` | `review_with_tool` |
| `.npm`, `.pnpm-store`, `.yarn` | `node_package_cache` | `cleanup_candidate` |
| `.bun`, `.deno` | `js_runtime_cache` | `cleanup_candidate` |
| `.cache/uv`, `.cache/pip` | `python_package_cache` | `cleanup_candidate` |
| `.pyenv` | `python_toolchains` | `review_with_tool` |
| `.gradle` | `jvm_package_cache` | `cleanup_candidate` |
| `.m2` | `maven_package_cache` | `review` |
| `.mix`, `.hex` | `elixir_package_cache` | `cleanup_candidate` |
| `.docker` | `container_state` | `review_with_tool` |
| `.minikube` | `kubernetes_local_state` | `review_with_tool` |
| `.kube` | `kubernetes_config` | `keep` |
| `.ollama` | `local_model_store` | `review_with_tool` |
| `.cache/huggingface` | `model_cache` | `review_with_tool` |
| `.local` | `local_app_state` | `review` |
| `.config` | `config_state` | `keep` |
| `Library/Developer` | `apple_developer_state` | `review` |
| `Library/Caches` | `macos_user_caches` | `cleanup_candidate` |
| `Library/Application Support/MobileSync/Backup` | `ios_backup` | `review` |
| `Library/Messages/Attachments` | `messages_attachments` | `review` |

### Last-Used / Updated Signals

Root-tool reports should include:

```text
bytes
files
dirs
created_unix
last_accessed_unix
last_modified_unix
metadata_changed_unix
newest_descendant_modified_unix
newest_descendant_path
days_since_modified
days_since_accessed
days_since_newest_descendant_modified
recommendation
rationale
```

`last_accessed` is a weak signal on macOS. Prefer `newest_descendant_modified_unix` when determining whether a tool root appears stale.

---

## 9. Time Machine & APFS Snapshot Awareness

Deleting files may not immediately free disk blocks if APFS local snapshots retain references.

Correct lifecycle:

```text
exclude rebuildable junk
  → delete rebuildable junk from reviewed plan
  → observe local snapshots
  → thin snapshots explicitly if requested
  → verify free space
  → write receipt
```

Agents may add Time Machine support only through explicit commands or plan artifacts. Do not silently thin snapshots.

Suggested commands:

```text
snapshot audit
snapshot thin --bytes 300GB
exclusion plan --from cleanup-plan.jsonocel --output tm-exclusions.sh
exclusion apply --from tm-exclusions.sh
```

---

## 10. Progress & Liveness UX

Long scans must prove they are alive.

Use:

```text
spinner       → liveness
counters      → proof of movement
byte rate     → proof of useful work
phase marker  → where time is being spent
progress bar  → delete-plan execution
```

The scanner should use a dedicated reporter thread reading atomics. Worker threads should not spam per-path logs.

Example console shape:

```text
⠸ auditing disk | phase=walking filesystem | files=812440 dirs=59211 seen=241.83 GB rate=184.92 MB/s skipped=93 errors=0 elapsed=82s
⠧ auditing disk | phase=building report | files=1429011 dirs=88412 seen=512.09 GB rate=0 B/s skipped=104 errors=2 elapsed=181s
✅ audit complete
```

Delete mode should use item-count progress because the plan size is known.

---

## 11. Privacy & Repository Safety

Source code is publishable. Real local output is not.

Do not commit:

```text
cleanup-plan*.json
cleanup-plan*.jsonocel
deletion-plan*.json
deletion-plan*.jsonocel
delete-receipt*.json
delete-receipt*.jsonocel
disk-audit*.json
disk-audit*.jsonocel
tool-root-audit*.json
tool-root-audit*.jsonocel
*.log
*.trace
*.receipt
```

Real reports may contain:

```text
absolute paths
usernames
project names
hidden tool directories
agent worktree names
file sizes
timestamps
local development patterns
```

Examples must be redacted.

Use replacement patterns such as:

```text
/Users/<user>/dev/project-a/target
$HOME/workspace/project-a/target
```

CI must reject accidental local path leaks in docs/examples.

---

## 12. Development Workflow & Agent Rules

Agents must strictly follow these rules.

### 12.1 Exhaustive Completeness

Do not leave placeholders, stubs, mocks, or deferred implementation notes.

Forbidden:

```rust
// TODO: implement this
unimplemented!()
todo!()
panic!("not implemented")
```

Allowed only in tests when explicitly testing panic behavior.

### 12.2 No In-Place Stream Editing

Do not use `sed` or `awk` to modify repository files. Use direct file replacement or structured patch tooling.

### 12.3 Compilation & Tests

Before submitting changes, run:

```bash
cargo fmt
cargo check --all-targets
cargo test
cargo test --doc
```

### 12.4 Destructive Code Review Rule

Any change touching deletion, snapshot thinning, exclusion application, or plan execution must update at least one of:

```text
domain doctest
integration test
OCEL receipt test
privacy/refusal test
```

### 12.5 No Self-Marked Completion

Agents may not claim completion merely because code was edited. Completion requires observed evidence:

```text
build passed
tests passed
doctests passed
privacy scan passed
relevant receipts emitted or validated
```

---

## 13. Definition of Done

A change is done only when all applicable checks are satisfied.

| Area | Required Evidence |
|---|---|
| Rust compile | `cargo check --all-targets` passes |
| Formatting | `cargo fmt` applied |
| Unit tests | `cargo test` passes |
| Doctests | `cargo test --doc` passes |
| CLI behavior | noun-verb command remains thin |
| Privacy | no real local paths in examples/docs |
| OCEL | emitted report validates structurally |
| Deletion | delete only from plan; receipt emitted |
| Snapshot | explicit user action; before/after state recorded |

---

## 14. Recommended Repository Shape

```text
osx-clnr/
  Cargo.toml
  README.md
  AGENTS.md
  LICENSE
  .gitignore
  src/
    main.rs
    nouns/
      audit.rs
      artifact.rs
      tool_roots.rs
      plan.rs
      delete.rs
      receipt.rs
      snapshot.rs
      exclusion.rs
      ocel.rs
      privacy.rs
      doctor.rs
    domain/
      artifact.rs
      audit.rs
      tool_roots.rs
      policy.rs
      plan.rs
      delete.rs
      receipt.rs
      ocel.rs
      redaction.rs
      time.rs
    integration/
      fs.rs
      tmutil.rs
      docker.rs
      progress.rs
  docs/
    GALL_CHECKPOINTS.md
    PRIVACY_MODEL.md
    OCEL_MODEL.md
    TIME_MACHINE_MODEL.md
  examples/
    redacted-disk-audit.jsonocel
    redacted-cleanup-plan.jsonocel
    redacted-deletion-receipt.jsonocel
  tests/
    fixtures/
      fake-home/
```

---

## 15. Project Positioning

This project is not merely a disk cleaner.

It is:

> **A plan-bound macOS developer disk auditor that emits OCEL evidence, manufactures reviewable cleanup plans, executes only from approved plans, and records deletion receipts.**

The command surface should make that obvious:

```text
noun = governed object
verb = allowed transition
plan = reviewable authority
receipt = proof of consequence
doctest = local law
Gall checkpoint = promotion gate
```
