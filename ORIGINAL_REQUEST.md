# Original User Request

## Initial Request — 2026-05-26T13:56:32-07:00

Restructure and fully implement the Gall Checkpoints (G0 to G9) for the `osx-clnr` Rust project using module documentation and unit/doctests.

Working directory: /Users/sac/osx-clnr
Integrity mode: development

## Requirements

### R1. Modular Restructuring
The project must be structured into modular domain, noun, and integration layers following the shape specified in `AGENTS.md`.

### R2. Doctest and Moduledoc Discipline
Every public function under the domain layers must have module-level documentation and at least one passing doctest (covering positive and negative/refusal cases).

### R3. Safe Plan-Bound Deletion Verification
Every checkpoint transition must be verified using unit tests, doctests, and/or integration tests to prove that deletion can only occur from a validated dry-run plan.

## Acceptance Criteria

### Restructuring
- [ ] Code compiles cleanly without warnings or errors.
- [ ] Project is partitioned into library and binary targets.

### Verification
- [ ] `cargo test` and `cargo test --doc` execute and pass.

## Follow-up — 2026-05-26T21:27:44Z

# Antigravity Prompt — Phase 1 Adversarial Proof Pass

Working directory: `/Users/sac/osx-clnr`

Status: Phase 1 Architectural Purity is reported complete.

Your job is not to add features.

Your job is to prove whether the current implementation actually satisfies the Phase 1 claims.

Do not rely on summaries. Inspect the repository directly.

The claimed architecture is:

```text
integration reads the operating system
domain receives inert DTOs only
nouns orchestrate
delete execution delegates to integration/fs.rs
scan cannot delete
delete cannot scan
```

The reported evidence is:

```text
cargo check: ✅
unit tests: ✅ 1
integration tests: ✅ 12
doctests: ✅ 44
total: 57 passed, 0 failed
```

Your task is to produce a proof report with exact evidence.

---

# Mission

Perform an adversarial verification of Phase 1.

You must answer:

1. Is the domain layer actually pure?
2. Is all filesystem IO isolated to integration?
3. Is all destructive filesystem mutation isolated to integration/fs.rs?
4. Can scan accidentally delete?
5. Can delete accidentally scan?
6. Are the new DTOs sufficient and not leaking live OS handles?
7. Are doctests meaningful or merely superficial?
8. Are the integration tests proving the correct safety boundary?
9. Are there performance regressions or redundant filesystem reads?
10. What is the exact next Gall Checkpoint promotion status?

---

# Hard Rules

Do not change production behavior unless you find a real defect.

Do not add TODOs, stubs, placeholders, or speculative features.

Do not weaken safety boundaries to make tests pass.

Do not claim success without command output.

Do not summarize vaguely. Provide exact files, functions, and commands.

---

# Proof Checks Required

## P1 — Domain Purity Check

Inspect every file under:

```text
src/domain/
```

Verify there are no direct calls to:

```rust
std::fs::read_dir
std::fs::metadata
std::fs::symlink_metadata
std::fs::remove_file
std::fs::remove_dir_all
std::process::Command
fs::read_dir
fs::metadata
fs::symlink_metadata
fs::remove_file
fs::remove_dir_all
Command::new
```

Also check for imported filesystem APIs that are unused or suspicious.

You may use `rg`.

Expected result:

```text
0 violations
```

If violations exist, report them and classify:

```text
blocking
non-blocking
false positive
```

## P2 — Destructive Call Isolation Check

Search the full repo for destructive calls:

```rust
remove_file
remove_dir_all
trash
unlink
```

Expected result:

```text
std::fs::remove_file and std::fs::remove_dir_all appear only in src/integration/fs.rs
```

If any noun, domain, or scanner module directly deletes files, this is blocking.

## P3 — Scan Cannot Delete

Trace the scan path.

Prove that:

```text
scan_root
  → read_dir_snapshot
  → domain classification
  → plan/report candidates
```

does not call:

```text
delete_file
delete_dir_all
remove_file
remove_dir_all
```

Provide exact function chain.

## P4 — Delete Cannot Scan

Trace the delete path.

Prove that delete execution:

```text
delete-plan
  → load plan
  → validate plan
  → execute exact plan items
  → receipt
```

does not call:

```text
scan_root
read_dir_snapshot for discovery
detect_project_from_snapshot for live discovery
artifact_candidates_from_snapshot for live discovery
```

It may check path metadata for safety immediately before deletion, but it must not discover new deletion candidates.

## P5 — DTO Integrity Check

Inspect:

```text
EntrySnapshot
DirSnapshot
```

Verify:

* They contain inert values only.
* They do not contain `DirEntry`.
* They do not contain `Metadata`.
* They do not contain open file handles.
* They do not expose mutation APIs.
* Query methods are pure.

Report the exact struct fields.

## P6 — Doctest Quality Check

Inspect all doctests in `src/domain/**`.

Classify them:

```text
positive case
negative/refusal case
boundary case
serialization case
conservation case
```

Report gaps.

Minimum expectation:

* artifact classification has positive and negative examples.
* project detection has multi-marker examples.
* plan validation has refusal examples.
* receipt logic has conservation examples if already present.

If doctests only prove happy paths, mark this as incomplete.

## P7 — Integration Test Meaning Check

Inspect `tests/integration_tests.rs`.

Report what safety claims the 12 integration tests prove.

Map each test to a Gall Checkpoint:

```text
G0 architecture
G1 artifact classification
G2 traversal barrier
G3 plan-bound deletion
```

If tests do not prove scan/delete separation, propose exact tests to add.

## P8 — Performance Boundary Check

Inspect whether `detect_project_from_snapshot` avoids redundant stat calls.

Verify the path:

```text
walker/integration reads directory once
snapshot is passed to domain
domain does not re-stat children
```

If there is still repeated filesystem metadata lookup during project detection, report it.

Also inspect tool-root attribution.

If it still performs `O(files × tool_roots)` matching, classify as:

```text
known Phase 2 performance debt
```

unless already fixed.

## P9 — Privacy Safety Check

Run or simulate a privacy scan.

Search docs, examples, fixtures, and committed files for real local paths:

```text
/Users/sac
/Users/
```

Classify findings:

```text
real leak
synthetic example
acceptable documentation
blocking
```

Do not alter privacy policy unless needed.

## P10 — Full Command Evidence

Run:

```bash
cargo check --all-targets
cargo test
cargo test --doc
```

Also run:

```bash
rg "std::fs::read_dir|std::fs::metadata|std::fs::symlink_metadata|std::fs::remove_file|std::fs::remove_dir_all|std::process::Command|Command::new" src/domain || true

rg "remove_file|remove_dir_all|Command::new|read_dir|symlink_metadata|metadata" src || true

rg "/Users/sac|/Users/" docs examples tests src README.md AGENTS.md .gitignore || true
```

Capture and summarize outputs.

## Follow-up — 2026-06-14T20:22:14Z

Evolve the `osx-clnr` tool to add a command-line wrapper around the `gh` CLI to inspect, plan, and clean up GitHub repositories. The tool must follow the project's strict `clap-noun-verb` design and non-negotiable operating laws (dry-run, plan, receipt, verification).

Working directory: `/Users/sac/osx-clnr`
Integrity mode: benchmark

## Requirements

### R1. CLI Integration & Noun-Verb CLI Contract
Add a new noun `github` or `repo` to the existing clap-noun-verb structure in `osx-clnr`. The command surface must follow the existing pattern:
- `github scan` or `github audit`: Inspect repositories, branches, runs, and releases to identify stale candidates.
- `github plan`: Build a reviewable cleanup plan file.
- `github delete`: Execute deletion strictly from the saved plan file, executing the corresponding `gh` CLI commands.
- `github receipt`: Output and verify deletion receipts.

### R2. Strict `gh` CLI Wrapper Boundary
All interaction with GitHub must occur through executing the local `gh` CLI binary. The application must not perform direct HTTP network requests to the GitHub API, nor manage raw authentication tokens. It relies entirely on the system's authenticated `gh` environment.

### R3. Supported Cleanup Targets
The tool must support auditing and proposing plans to:
- Delete merged/stale branches (both local tracking and remote).
- Delete old GitHub Action workflow runs (e.g., filtering by status, age, or count).
- Archive or delete stale/empty repositories.
- Delete old/draft releases or untagged packages.

### R4. Plan-Bound Execution & Audit Trail
Adhere strictly to the `osx-clnr` safety laws in `AGENTS.md`:
- Never run destructive actions directly from a live scan.
- Write a structured JSON/OCEL plan file containing the targets for deletion.
- Execution requires passing the plan file path.
- Generate a receipt file mapping every successful, skipped, or failed deletion event.

## Acceptance Criteria

### CLI Interface Validation
- [ ] Running the binary with the new `github` noun and verbs successfully outputs help messages and accepts valid parameters.

### Plan Generation
- [ ] Running the scan/plan step generates a valid JSON plan containing structured cleanup targets (e.g., target branch names, run IDs, repo names, release tags).

### Mocked CLI Verification
- [ ] Rust integration/unit tests verify the command builder logic by mocking the output of `gh` CLI commands (e.g., mock stdout for `gh repo list`, `gh run list`, etc.) and checking that the target commands (e.g., `gh repo delete`, `gh run delete`) are invoked with correct arguments.

## Follow-up — 2026-06-14T23:37:44Z

Evolve the `osx-clnr` GitHub cleanup capabilities to support actions cache deletion, stale issues/PRs management, release assets cleanup, and pagination/interaction optimizations. The additions must follow the project's strict `clap-noun-verb` design and non-destructive plan-bound operating laws.

Working directory: `/Users/sac/osx-clnr`
Integrity mode: benchmark

## Requirements

### R1. Expand Cleanup Targets
The companion tool must support the following new targets:
1. **GitHub Actions Cache**: Retrieve and delete old/stale Actions Caches via the `gh cache` CLI (e.g. `gh cache list` and `gh cache delete`).
2. **Stale/Inactive Issues & PRs**: Identify and close or label issues and pull requests that have had no activity for $N$ days. Use the `gh issue` and `gh pr` CLIs.
3. **Release Assets**: List and delete large asset files/binaries attached to old releases, keeping the release itself if desired, or deleting specific target assets.

### R2. CLI wrapper & Pagination Optimizations
1. Wrap all interactions using the system's `gh` CLI executable. Optimize all list operations with proper pagination/limits to handle repositories with large numbers of issues, runs, or caches.
2. For interactive executions, support prompt choices or confirmation dialogs where appropriate during manual invocations (non-automated runs).

### R3. Safety & Plan-Bound Operations
1. All new targets must adhere to the `osx-clnr` safety laws:
   - Identify candidate resources during the scan/audit phase.
   - Write those resources as items to a reviewable JSON plan file (e.g. `github://cache/...`, `github://issue/...`, etc.).
   - Execute deletion strictly from the plan file.
   - Record outcomes in a detailed deletion receipt.

## Acceptance Criteria

### Integration Tests & Mocks
- [ ] Implement Rust unit/integration tests with mocked outputs for `gh cache list`, `gh issue list`, `gh pr list`, etc.
- [ ] Verify that correct cleanup commands (e.g. `gh cache delete`, `gh issue close`) are built and executed.

### CLI Execution
- [ ] Running the binary with the new targets added to the `github` subcommand runs successfully, outputs valid help text, builds plans, and executes deletions strictly from plans.```
