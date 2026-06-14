# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Identity

**Binary:** `oclnr` — package name is `pentecost` (`Cargo.toml` → `[[bin]] name = "oclnr"`)

**Purpose:** macOS developer disk auditor and cleanup utility. Enforces the invariant: *never increase destructive power without simultaneously increasing receipts.* Deletion is always plan-bound — the scanner cannot delete; the deleter cannot scan.

## Build Commands

Use direct `cargo` commands (no `Makefile.toml`, no `cargo-make` here):

```bash
cargo build                          # build
cargo test                           # all unit + integration tests
cargo test <test_name>               # single test
cargo test -- --nocapture            # tests with stdout
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run -- <subcommand>            # run CLI

# .cargo/config.toml aliases
cargo dx                             # alias for `cargo test`
cargo sanity                         # full DX suite (fmt + clippy + test + 4 doctor checks)
cargo doctor-arch                    # alias for `cargo run -- doctor architecture`
cargo doctor-sub                     # alias for `cargo run -- doctor substrate`
cargo doctor-doc                     # alias for `cargo run -- doctor doctests`
cargo doctor-priv                    # alias for `cargo run -- doctor privacy`
```

The `scripts/sanity.sh` script runs the full 7-step pipeline: fmt check → clippy → test → `doctor architecture` → `doctor substrate` → `doctor doctests` → `doctor privacy`.

## Architecture

Three strictly separated layers:

```
src/
  domain/      — Pure Rust: zero std::fs calls. Receives inert DTOs only.
  integration/ — All std::fs, OS, tmutil, Docker calls live here.
  nouns/       — CLI subcommand handlers; bridges integration ↔ domain.
```

### Domain purity rule (hard constraint)

`src/domain/**` must contain **zero** `std::fs`, `std::process`, or OS calls. Domain functions receive inert snapshot DTOs (`EntrySnapshot`, `DirSnapshot`) constructed by the integration layer. Violations are architectural defects.

### CLI structure (`src/nouns/mod.rs`)

Each `Command` variant maps to a nouns submodule with its own `Action` enum and `handle()` function:

| CLI noun | Key domain module |
|---|---|
| `audit` | `domain::audit`, `domain::artifact` |
| `plan` | `domain::plan` |
| `delete` | `domain::delete` |
| `receipt` | `domain::receipt` |
| `tool-roots` | `domain::tool_roots` |
| `ocel` | `domain::ocel` |
| `snapshot` | `integration::tmutil` |
| `emergency` | `domain::artifact`, `integration::fs` |
| `doctor` | `domain::doctor`, `integration::doctor` |
| `privacy` | `domain::redaction` |
| `exclusion` | — |

### Execution pipeline (plan-bound by design)

```
oclnr audit scan     →  disk-audit.json + disk-audit.jsonocel
oclnr plan create    →  cleanup-plan.json  (human reviews this)
oclnr delete run     →  reads plan only; scanner is disabled
oclnr receipt verify →  deletion-receipt.jsonocel
```

### OCEL v2 (`src/domain/ocel.rs`)

Every operation emits an Object-Centric Event Log v2. `build_disk_audit_ocel`, `build_tool_roots_ocel`, `build_snapshot_audit_ocel`, `build_snapshot_thin_ocel`, `build_snapshot_delete_ocel`, etc. are the builders. All builders are pure functions — they take data, return `OcelLog`. `validate_ocel_log` checks referential integrity and type conformance. Delete events must carry relationships to `delete_receipt`, `deletion_plan`, `artifact_candidate`, and `filesystem_object` objects or validation fails.

`build_snapshot_delete_ocel` (distinct from `build_snapshot_thin_ocel`) emits `snapshot_delete_requested` events — the event type must truthfully reflect the operation (a delete is not a thin).

### Domain DTOs

- `EntrySnapshot` — single file/directory snapshot (path, name, extension, kind)
- `DirSnapshot` — all immediate children of one directory
- `Candidate` — a cleanup target (path + reason string)
- `DeletionPlan` / `PlanItem` — the reviewed plan file
- `DeletionReceipt` / `DeletionResult` — proof of what happened

`DirSnapshot` helpers (`has_file`, `has_dir`, `has_file_ext`, `dirs_with_suffix`, `dirs_with_prefix`) are the primary interface for project-type detection in `domain::artifact`.

### Traversal model

`integration::fs::scan_root` uses the `ignore` crate walker. Directories in `traversal_barrier_names()` (e.g., `node_modules`, `target`, `.next`) are recorded as candidates but **not walked**. `is_macos_os_dir` and `is_global_cache` guard against touching system or global tool cache paths.

## Doctests are specification

Domain functions carry doctests with positive + negative + refusal cases. These are the functional specification, not just examples. Run them via `cargo test --doc` or `oclnr doctor doctests`. Do not remove or weaken them.

## Output files (never commit)

`.gitignore` protects: `disk-audit.json`, `*.jsonocel`, `cleanup-plan.json`, `deletion-receipt.jsonocel`. These contain absolute paths, project names, and timestamps from the local machine.

## Key Dependencies

- `libc = "0.2"` — used by `integration::fs::volume_space` for `statvfs` free-space sampling

## Gall Checkpoints

The system evolves through checkpoints G0–G9 (see `docs/GALL_CHECKPOINTS.md`). Current state: G0–G7 substantially complete. G8 (privacy gate) and G9 (doctor self-verification) are in-progress. Do not add new capabilities without adding corresponding receipts/evidence.
