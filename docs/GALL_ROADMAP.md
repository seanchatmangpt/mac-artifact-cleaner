# Gall Checkpoint Roadmap: Path to G9 Completion

This roadmap defines the remaining steps to transition `pentecost` from its current state to a fully realized G9 implementation.

## Current Status Summary (June 2026)
- **M1 (G0-G3):** ✅ Complete — domain purity, traversal barriers, plan-bound deletion, integration separation verified.
- **M2 (G4-G6):** ✅ Complete — free-space reporting (`VolumeSpace`/`statvfs`), bytes accounting, snapshot delete, emergency reclaim, tool-root aging, global cache nomination.
- **M3 (G7):** ✅ Substantially complete — all operations emit OCEL v2; `snapshot_delete_requested` event type distinct from `snapshot_thin_requested`.
- **M4 (G8-G9):** 🔄 In progress — `doctor privacy` exists; auto-redaction and full G9 promotion rule pending.

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

## Phase 2: Visibility & Snapshot Integrity (COMPLETE)
**Target: G4 (Inventory) & G5 (Snapshots) Completion**

1.  **G4: Enhanced UX Visibility**: ✅
    - `VolumeSpace`/`statvfs` free-space sampling; disk header printed on `oclnr audit`.
    - Bytes accounting: `PlanItem.bytes`, `DeletionResult.bytes_freed`, `bytes_freed_total` reclaim total.
2.  **G5: Snapshot Flow Integration**: ✅
    - `oclnr snapshot thin --bytes`, `oclnr snapshot delete --which` (oldest/all/explicit).
    - `oclnr emergency [--yes]` — ENOSPC escalation path.
    - `check_reclaim` reality law in receipt verification (claimed vs. measured delta, 50% tolerance).
    - `write_or_dump_on_full` — ENOSPC-safe writer for plan and receipt output.

## Phase 3: Evidence & Reporting (COMPLETE)
**Target: G6 (Tool Roots) & G7 (OCEL v2) Integration**

1.  **G7: Unified OCEL Execution**: ✅
    - OCEL builders for all operations including `build_snapshot_delete_ocel` (distinct event type).
    - `validate_ocel_log` referential integrity check.
2.  **G6: Tool-Root Maturity**: ✅
    - `tool-roots audit` wired; `recommend_tool_root` classification logic.
    - `--include-global-caches` flag on `oclnr plan` nominates regenerable caches.

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
