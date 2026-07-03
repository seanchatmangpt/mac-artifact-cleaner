# Phase 1 Remediation Plan (✅ COMPLETED)

This document outlines the actions that were taken and completed to fix all architectural and privacy violations. All items have been verified green.

---

## 1. Domain Purity: Tool Roots Metadata Fetching

### Problem
`src/domain/tool_roots.rs` contains direct calls to `std::fs::symlink_metadata` and Unix `ctime` metadata extraction inside `build_tool_root_report`.

### Remediation
1. Extend `ToolRootAcc` or pass a map of pre-computed path metadata from the integration layer into the domain's report builder.
2. In `src/integration/fs.rs`, record the `ctime`, `mtime`, and metadata properties during the directory traversal or in the tool-root collection phase.
3. Keep `src/domain/tool_roots.rs` strictly pure, executing matching and report sorting/building without making any `std::fs` calls.

---

## 2. Domain Purity: Doctor Diagnostics

### Problem
`src/domain/doctor.rs` contains direct calls to `fs::read_to_string`, `fs::read_dir`, path existence checks (`.exists()`, `.is_dir()`), and spawns system commands (`Command::new("which")` and `Command::new("cargo")`).

### Remediation
1. Relocate all external interactions to the integration layer (e.g. `src/integration/doctor.rs` or directly within the CLI handling layers under `src/nouns/doctor.rs`).
2. The domain module `src/domain/doctor.rs` must only expose pure types and pure evaluation functions:
   - For example, `diagnose_architecture` should accept a structured inventory of directory/file presence (e.g., `struct WorkspaceInventory`) rather than a live `&Path` reference to walk and check files directly.
   - Doctest analysis should receive file contents as `String` arrays or structs, running purely in-memory.
   - Substrate diagnostics should receive the outputs of command executions pre-run by the integration layer.

---

## 3. Privacy Leaks: Absolute URL References

### Problem
Absolute markdown links referencing local filesystem paths (containing the absolute home path `file:///Users/user/`) are hardcoded in documentation:
- `docs/PRIVACY_MODEL.md`
- `docs/TIME_MACHINE_MODEL.md`
- `docs/OCEL_MODEL.md`

### Remediation
1. Rewrite all absolute links to relative markdown links:
   - For example, `[tmutil.rs](file:///Users/user/osx-clnr/src/integration/tmutil.rs)` becomes `[tmutil.rs](../src/integration/tmutil.rs)`.
   - Replace absolute links to the `.gitignore` or other sources with relative workspace paths.

---

## 4. Privacy Bypasses: Hardcoded Allowlist in Validator

### Problem
`src/domain/doctor.rs` has `"sac"` explicitly listed in the allowlist of usernames for privacy checks to bypass the scanner on the developer's local machine.

### Remediation
1. Remove `|| clean_username == "sac";` from `scan_unredacted_paths` in `src/domain/doctor.rs`.
2. Ensure that any personal username pattern is redacted/sanitized in documentation, examples, and tests so that the validator checks can be executed cleanly on all environments without special developer-specific bypasses.

---

## 5. Test Coverage: Traversal Barrier Integration Test

### Problem
No integration test explicitly validates that the scanner halts descent at traversal barriers and increments the pruning statistics counter.

### Remediation
1. Add `test_traversal_barriers` to `tests/integration_tests.rs`.
2. Construct a mock directory containing `node_modules` and subdirectories inside it (e.g., `node_modules/nested-pkg/dist/`).
3. Run `scan_root` and assert that:
   - The outer `node_modules` is detected.
   - The traversal stops at the barrier (i.e. children of `node_modules` are not traversed or added).
   - Stats reflect the barrier application (e.g. `pruned_dirs` matches the expected counts).
