# DOC_COVERAGE_LOG

Bijective coverage map: every documented capability must have a running example,
and every example must be referenced from the docs. A "covered ✅" entry cites the
example that ran and what its run demonstrated.

---

## Iteration 1 — 2026-06-14

**Commit at start:** `4be87d5`  
**Working tree:** clean  
**Toolchain:** cargo 1.97.0-nightly, rustc 1.97.0-nightly  
**Examples dir before:** did not exist (zero exercised surface)

---

### Coverage Map (pre-iteration)

#### Documented-but-unexercised (gap — prose with no running example)

All 35 public domain functions and ~25 public types. High-priority clusters by
README/guide prominence:

| Capability | Described in | Example |
|---|---|---|
| `human_bytes` / `parse_size_in_bytes` | README, TIME_MACHINE_MODEL | ❌ none |
| `check_reclaim` / `ReclaimCheck` | TIME_MACHINE_MODEL §3.4, receipt.rs doc | ❌ none |
| `select_oldest_snapshots` / `identify_thinned_snapshots` / `SnapshotThinReceipt` | TIME_MACHINE_MODEL §3.2 | ❌ none |
| `artifact_candidates_from_snapshot` / `detect_project_from_snapshot` | CLAUDE.md, OCEL_MODEL | ❌ none |
| `build_disk_audit_ocel` / `summarize_ocel_log` | OCEL_MODEL, CLAUDE.md | ❌ none |
| `redact_path` / `redact_content` | PRIVACY_MODEL | ❌ none |
| `recommend_tool_root` / `build_tool_root_report` | GALL_CHECKPOINTS G6 | ❌ none |
| `global_cache_candidates` | README, TIME_MACHINE_MODEL | ❌ none |
| `diagnose_architecture` / `diagnose_substrate` | GALL_CHECKPOINTS G9 | ❌ none |
| `DeletionReceipt::verify` / `VerificationReport` | CLAUDE.md | ❌ none |

#### Exercised-but-undocumented (gap — examples touching undiscoverable APIs)

None (no examples existed).

---

### Triples Closed This Iteration

#### Triple 1: Size units round-trip ✅

- **Doc:** `src/domain/time.rs` `human_bytes` + `parse_size_in_bytes` (existing doc comments + added link to example)
- **Example:** `examples/size_units.rs`
- **Run output + exit code:**
  ```
  parse "1GB"   -> 1000000000 bytes
  parse "500mb" -> 500000000 bytes
  format 1 GB   -> "1.00 GB"
  format 500 MB -> "500.00 MB"
  round-trip    -> "1.00 GB"
  size_units: all assertions passed
  EXIT:0
  ```
- **What the run demonstrated:** SI/1000 base enforced, round-trip lossless at unit boundaries, refusal cases (invalid/empty) rejected. Would fail if base were changed to 1024.

#### Triple 2: Reclaim check reality law ✅

- **Doc:** `src/domain/receipt.rs` `check_reclaim` (existing doc comment + added link to example)
- **Example:** `examples/reclaim_check.rs`
- **Run output + exit code:**
  ```
  not-applicable (500 MB claim):  true
  witnessed (3 GB measured, 4 GB claimed):  true
  shortfall (0 measured, 3 GB claimed):  true
    claimed=3000000000 measured=0
  back-compat (None/None):  true
  reclaim_check: all assertions passed
  EXIT:0
  ```
- **What the run demonstrated:** all four behavioral cases exercised — floor guard (NotApplicable), within-tolerance recovery (Witnessed), snapshot-pinning detection (Shortfall with correct fields), and back-compat for old receipts. `BytesFreedMismatch` ghost-variant path is now a running witness.

#### Triple 3: Snapshot pipeline cross-product ✅

- **Doc:** `src/domain/time.rs` `select_oldest_snapshots` (existing doc comment + added link to example)
- **Example:** `examples/snapshot_pipeline.rs`
- **Run output + exit code:**
  ```
  select 2 oldest  -> ["2026-05-25-080000", "2026-05-26-135630"]
  thinned list     -> ["com.apple.TimeMachine.2026-05-26-135630.local", "com.apple.TimeMachine.2026-05-25-080000.local"]
  receipt          -> snapshots_thinned=[...]
  edge: n=0        -> []
  edge: unparsable -> []
  snapshot_pipeline: all assertions passed
  EXIT:0
  ```
- **What the run demonstrated:** three APIs composing — selection, identification, receipt. Chronological sort order enforced (oldest-first). Edge cases (n=0, unparsable names) silently ignored as specified. Would fail if sort order inverted or `SnapshotThinReceipt::new` diffing logic broke.

---

### Queued (not closed this iteration — 3-triple hard cap reached)

| Capability cluster | Files | Priority |
|---|---|---|
| artifact detection pipeline | `artifact.rs` | High — README workflow step 1 |
| OCEL builders + `summarize_ocel_log` | `ocel.rs` | High — OCEL_MODEL guide |
| `redact_path` / `redact_content` | `redaction.rs` | Medium — G8 privacy gate |
| `recommend_tool_root` + `build_tool_root_report` | `tool_roots.rs` | Medium — G6 |
| `DeletionReceipt::verify` full pipeline | `receipt.rs` | High — core proof gate |
| `global_cache_candidates` | `artifact.rs` | Medium — emergency reclaim |
| `diagnose_architecture` | `doctor.rs` | Medium — G9 self-verification |

---

### Hard Stops

None.

---

### Post-iteration Gap Map

#### Still documented-but-unexercised

| Capability | Described in | Status |
|---|---|---|
| `artifact_candidates_from_snapshot` / `detect_project_from_snapshot` | CLAUDE.md, README | OPEN — queued |
| `build_disk_audit_ocel` / `summarize_ocel_log` | OCEL_MODEL | OPEN — queued |
| `redact_path` / `redact_content` | PRIVACY_MODEL | OPEN — queued |
| `recommend_tool_root` / `build_tool_root_report` | GALL_CHECKPOINTS G6 | OPEN — queued |
| `global_cache_candidates` | README, TIME_MACHINE_MODEL | OPEN — queued |
| `diagnose_architecture` | GALL_CHECKPOINTS G9 | OPEN — queued |
| `DeletionReceipt::verify` + `VerificationReport` | CLAUDE.md | OPEN — queued |

#### Exercised-but-undocumented

None — all three new examples are referenced from their primary doc comments and
from this log.

#### Covered ✅ (this iteration)

| Capability | Example |
|---|---|
| `human_bytes` + `parse_size_in_bytes` | `examples/size_units.rs` |
| `check_reclaim` + `ReclaimCheck` | `examples/reclaim_check.rs` |
| `select_oldest_snapshots` + `identify_thinned_snapshots` + `SnapshotThinReceipt` | `examples/snapshot_pipeline.rs` |
