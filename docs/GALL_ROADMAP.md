# Gall Checkpoint Roadmap: Path to G9 Completion

This roadmap defines the remaining steps to transition `pentecost` from its current state (Phase 1 proven, checkpoints G0-G3 complete) to a fully realized G9 implementation.

## Current Status Summary
- **M1 (G0-G3):** Phase 1 proven! The domain layer is fully pure, destructive calls are isolated, and traversal barriers are verified.
- **M2 (G4-G6):** Logic largely present in `src/domain/`, but integration and CLI wiring are incomplete.
- **M3 (G7-G9):** OCEL v2 and Privacy frameworks exist but are not yet used as the primary execution gates.

---

## Phase 1: Architectural Alignment & Performance (COMPLETE)
**Target: G0-G3 Refinement & Foundation for G4**

1.  **Purge Domain Side-Effects**:
    - Refactor `src/domain/artifact.rs`, `src/domain/doctor.rs`, and `src/domain/tool_roots.rs` to remove direct filesystem/system calls.
    - Integration may read `DirEntry` and `Metadata`. Domain receives inert snapshots only.
2.  **Relocate Destructive Logic**:
    - Move `std::fs::remove_file` and `std::fs::remove_dir_all` from `src/nouns/delete.rs` to a new `src/integration/fs::delete` module.
3.  **Optimize Traversal Bottlenecks**:
    - Refactor `detect_project` to utilize metadata already collected by the `ignore` walker, eliminating redundant `stat` calls.
    - Implement a Prefix-Trie or Index for tool-root recording to replace the current $O(N \cdot M)$ linear search.
4.  **Time Machine Receipting**:
    - Add a `DeletionReceipt`-style mechanism for `exclusion apply` to ensure the "Never increase destructive power without simultaneously increasing receipts" law is upheld for exclusions.

## Phase 2: Visibility & Snapshot Integrity
**Target: G4 (Inventory) & G5 (Snapshots) Completion**

1.  **G4: Enhanced UX Visibility**:
    - Add real-time byte-rate and ETA calculation to `ProgressReporter`.
    - Implement `audit summarize` CLI to provide a high-level breakdown of disk usage by category.
2.  **G5: Snapshot Flow Integration**:
    - Implement `snapshot thin` CLI with safety guards.
    - Update the cleanup workflow to recommend snapshot thinning if disk space is not freed after deletion.
    - Ensure all `tmutil` interactions emit OCEL events.

## Phase 3: Evidence & Reporting
**Target: G6 (Tool Roots) & G7 (OCEL v2) Integration**

1.  **G7/G3: Unified OCEL Execution**:
    - Implement OCEL builders for `deletion_plan` and `delete_receipt` in `src/domain/ocel.rs`.
    - Update `plan build` and `delete execute` to emit OCEL v2 logs as the primary evidence artifact.
2.  **G6: Tool-Root Maturity**:
    - Wire `tool-roots audit` into the standard `audit` flow.
    - Implement `--stale` and `--min-size` filters for tool-root reporting.

## Phase 4: Privacy & Safety Gate
**Target: G8 (Privacy) & G9 (Doctor) Completion**

1.  **G8: Robust Privacy Gate**:
    - **Environment Neutrality**: Remove all hardcoded developer paths ('sac', 'john') from logic and tests.
    - **Gitignore Expansion**: Add `.agents/`, `.antigravitycli/`, and OCEL output patterns to `.gitignore`.
    - **CLI Implementation**: Build the `privacy` noun (`privacy scan`, `privacy redact`).
    - **Auto-Redaction**: Integrate `src/domain/redaction.rs` into all serialization paths so `--redact` applies globally.
2.  **G9: Final Promotion (The Doctor)**:
    - Finalize `doctor architecture` to verify domain purity and layer isolation.
    - Implement `doctor privacy` as a pre-commit/pre-publish check.
    - Do not implement direct mutating `doctor fix`. Instead implement `doctor diagnose`, `doctor plan-fix --output doctor-fix-plan.jsonocel`, and `doctor apply-fix --from doctor-fix-plan.jsonocel --receipt doctor-fix-receipt.jsonocel`.

---

## Promotion Rule for G9
The system is considered "Done" when:
1. `doctor architecture` reports 0 violations.
2. `doctor privacy` reports 0 unredacted local paths in documented examples.
3. Every destructive action (delete, thin, exclude) produces a verifiable OCEL v2 receipt.
4. Performance benchmarks show 0 redundant syscalls during a standard audit.
