# Phase 1 Adversarial Proof Report

## Verdict

- ✅ Phase 1 proven

## Command Evidence

| Command | Result | Notes |
|---|---:|---|
| `cargo check --all-targets` | PASS | Code compiles cleanly without warning or error. |
| `cargo test` | PASS | All unit and 13 integration tests pass successfully. |
| `cargo test --doc` | PASS | All 44 domain and noun doctests pass successfully. |
| domain purity rg | PASS | 0 violations. Direct calls to `std::fs` and `Command` have been moved to integration. |
| destructive isolation rg | PASS | `remove_file` and `remove_dir_all` reside only in `src/integration/fs.rs`. |
| privacy rg | PASS | 0 violations. Absolute paths and developer-specific bypasses have been removed. |

---

## Architecture Proof

The call boundaries are strictly separated:

```text
scan path:
nouns::audit::handle or nouns::artifact::handle
  → integration::fs::scan_root
      → WalkParallel (traverses OS directories)
      → integration::fs::read_dir_snapshot (constructs DirSnapshot)
      → domain::artifact::detect_project_from_snapshot (pure DTO classification)
      → domain::artifact::artifact_candidates_from_snapshot (pure DTO candidate generation)
      → candidates accumulated read-only (no deletion functions called)

delete path:
nouns::delete::handle
  → std::fs::read_to_string (loads plan)
  → domain::delete::validate_plan (pure plan safety check)
  → iterates over plan items
      → integration::fs::delete_file or integration::fs::delete_dir_all (strict plan-bound deletion)
      → receipt written (no discovery scanning or live traversal performed)
```

## Domain Purity Findings

All violations have been successfully remediated:
1. **`src/domain/tool_roots.rs`:** 0 filesystem calls. Root directory metadata is queried during scanning/reporting within `src/integration/fs.rs` and loaded from the new atomic fields in `ToolRootAcc`.
2. **`src/domain/doctor.rs`:** 0 filesystem or system calls. Spawning commands and directory reads have been moved to `src/integration/doctor.rs`.

## Destructive Isolation Findings

Destructive filesystem operations (`remove_file`, `remove_dir_all`) remain strictly isolated to `src/integration/fs.rs` (`delete_file`, `delete_dir_all`).

## DTO Integrity Findings

`EntrySnapshot` and `DirSnapshot` are completely inert and contain only primitive types and strings. They do not hold live OS descriptors, `DirEntry`, or `Metadata` handles.

## Doctest Quality Findings

All 44 doctests are meaningful and execute successfully. Doctest namespace references have been updated from `mac_artifact_cleaner` to `pentecost`.

## Integration Test Coverage

The integration test suite was extended with a new test: `test_traversal_barriers` which programmatically validates that traversal halts descent at traversal barriers (G2 checkpoint). All 13 integration tests pass.

## Performance Findings

- `detect_project_from_snapshot` avoids redundant stat calls.
- Tool-root matching linear complexity `O(files × tool_roots)` remains classified as *known Phase 2 performance debt*.

## Privacy Findings

- All absolute URLs referencing `/Users/sac` in markdown documentation have been updated to relative markdown links.
- The hardcoded bypass allowlist containing `"sac"` has been deleted from `src/domain/doctor.rs`. Privacy scans now run environment-neutrally.

## Promotion Decision

- **G0-G3 checkpoints are officially promoted to stable.** Phase 1 purity and safety invariants are fully proven.

## Required Fixes Before G4

- None. All Phase 1 blocking issues have been resolved.

## Non-Blocking Follow-up

- Optimizing tool-root tracking complexity in Phase 2.
