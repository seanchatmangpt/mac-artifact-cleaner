# CLAUDE.md

## Identity

**Package:** `osx-clnr` | **Binary:** `oclnr` and `oclnr-mcp`  
**Codename:** Pentecost | **First praxis project** (house standard: CalVer, MIT OR Apache-2.0, workspace lints, justfile canonical, star-toml admitted config)  
**Purpose:** macOS developer disk auditor and cleanup utility.  
**Core invariant:** Never increase destructive power without increasing receipts. The scanner cannot delete; the deleter cannot scan.

## Build

```bash
cargo build
cargo test                        # unit + integration
cargo test <name> -- --nocapture
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin oclnr -- <noun> <verb>
cargo run --bin oclnr-mcp         # MCP server (7 resource-grouped tools)

# Aliases (.cargo/config.toml)
cargo dx        # all tests
cargo sanity    # fmt → clippy → test → 4 doctor checks
cargo doctor-arch / doctor-sub / doctor-doc / doctor-priv
```

`scripts/sanity.sh` runs the full 7-step pipeline.

**Tests:** unit/doctests inline in `src/**/*.rs`; integration in `tests/`.

## Architecture

```
src/
  domain/      — Pure Rust. Zero std::fs, std::process, or OS calls. Business logic only.
  integration/ — All filesystem, OS, tmutil, Docker, GitHub I/O lives here.
  nouns/       — CLI subcommand handlers. Bridges integration ↔ domain.
  mcp/         — MCP server (oclnr-mcp): 7 resource-grouped tools exposing the full workflow over JSON-RPC.
```

**Workspace members:** `osx-clnr` (root), `cfab-surface` (visualization)  
**Path deps:** `clap-noun-verb` (sibling) — noun-verb CLI dispatch framework

### Domain purity (hard constraint)

`src/domain/**` must have **zero** `std::fs`, `std::process`, or OS calls. Domain receives inert DTOs (`EntrySnapshot`, `DirSnapshot`) built by the integration layer. Violations are architectural defects.

### Domain modules

| Module | Role |
|---|---|
| `artifact` | Project-type detection (Rust, Node, Docker, …) |
| `audit` | Core scanning logic |
| `plan` | Plan generation from audit results |
| `delete` | Deletion logic (reads plan, produces results) |
| `receipt` | Receipt validation and proof generation |
| `ocel` | OCEL v2 builders and validators |
| `affidavit_integration` | Receipt sealing via `affidavit` crate |
| `fabric` | SurfaceGraph fabric |
| `tool_roots` | Tool root detection |
| `doctor` | Self-verification and diagnostics |
| `policy`, `time`, `redaction`, `crypto`, `github`, `ocl` | Supporting domains |

### Integration modules

`fs`, `tmutil`, `doctor`, `github`, `docker`, `monitor`, `progress`

### MCP server (`src/mcp/`)

`oclnr-mcp` exposes 7 resource-grouped tools over JSON-RPC, each dispatched by an `action`
parameter, for Claude to drive the full workflow:

| Tool | Actions |
|---|---|
| `workflow` | `query` \| `clear` \| `rollback` |
| `audit` | `scan` \| `parse` \| `breakdown` |
| `plan` | `build` \| `inspect` \| `validate` \| `approve` |
| `delete` | `dry_run` \| `execute` |
| `receipt` | `parse` \| `verify` (pass `seal: true` to also seal with an affidavit proof chain) |
| `snapshot` | `audit` \| `thin` \| `delete` |
| `emergency_reclaim` | *(no actions — standalone; scans and deletes in one call, kept separate from `delete` deliberately)* |

**When using Claude to clean disk: use MCP tools — not raw `cargo run` or shell commands.**
This is enforced, not just advisory: `.claude/settings.json` has a `PreToolUse` hook on `Bash`
that inspects every shell command for two categories:
- **Destructive cleanup-shaped commands** (`rm -rf .../target`, `find ... -delete`,
  `docker system/container/image prune`, `colima delete`, `tmutil deletelocalsnapshots`,
  `cargo clean`) — blocked behind an explicit confirmation prompt naming the MCP tool that
  should have been used instead. Covers the failure mode from a prior session where
  Docker/Colima cleanup happened via raw `Bash` (`docker system prune`, `colima delete`) with
  no receipt, entirely outside this audit trail — Time Machine/APFS snapshot work has an MCP
  tool (`snapshot`) and stayed inside the trail; Docker/Colima do not yet, so that specific gap
  can still recur until an MCP tool exists for them.
- **Read-only disk-usage inspection commands** (`df`, `du`, `diskutil list`/`apfs list`/`info`,
  `tmutil listlocalsnapshots`) — allowed to run (no confirmation needed, nothing destructive),
  but the hook injects a note recommending `audit(action: "breakdown")` instead: it walks
  hidden dirs and non-artifact data (Library, caches, VM disk images) with physical block
  allocation and returns structured JSON, where `df`/`du`/`diskutil` return text that has to be
  parsed and — as happened in the same prior session — is easy to reach for reactively instead
  of reaching for the MCP tool built for exactly this. `tmutil listlocalsnapshots` gets pointed
  at `snapshot(action: "audit")` the same way.

### Disk cleanup protocol (mandatory)

Always drive cleanup through the MCP server. Never use `rm -rf`, `find -delete`, or raw `cargo run` for deletion — those bypass the audit trail and leave no receipt.

**Correct sequence:**
1. `workflow(action: "query")` — check if a prior scan/plan exists
2. `audit(action: "scan")` — scan filesystem, discover candidates (`tool_roots: true` for
   dev-tool caches); `audit(action: "breakdown")` for a full byte-accounted usage picture
   (includes hidden dirs and non-artifact data) when the question is "where did the space go"
   rather than "what's safe to delete"
3. `plan(action: "build")` — build deletion plan (supports `aggressive` and `deps` flags for
   `target/`, `node_modules`, caches)
4. `plan(action: "inspect")` / `plan(action: "validate")` — review what will be deleted
5. `delete(action: "dry_run")` — preview without deleting
6. `delete(action: "execute")` — execute with `confirm: true`
7. `receipt(action: "verify")` — verify claimed vs actual free-space delta; pass `seal: true`
   (with `confirm: true`) to also seal with a cryptographic proof chain

**Never do:**
- `rm -rf ~/*/target` or any direct shell deletion
- `find ~ -name target -exec rm -rf` 
- `cargo clean` as a substitute for the MCP workflow
- Skip straight to `delete(action: "execute")` without `plan(action: "inspect")` first

### CLI noun → domain mapping

| Noun | Domain |
|---|---|
| `audit run/summarize` | `artifact`, `audit` |
| `plan build/inspect` | `plan` |
| `delete execute` | `delete` |
| `receipt verify` | `receipt`, `affidavit_integration` |
| `snapshot` | `integration::tmutil` |
| `emergency` | `artifact`, `integration::fs` |
| `doctor` | `doctor` |
| `privacy` | `redaction` |

### Execution pipeline

```
oclnr audit run          →  disk-audit.json + disk-audit.jsonocel
oclnr plan build         →  cleanup-plan.json  (human reviews)
oclnr delete execute     →  reads plan only; no fresh scan
oclnr receipt verify     →  deletion-receipt.jsonocel
```

## Key Invariants

**Plan-bound deletion:** `delete execute` reads only from a saved `cleanup-plan.json`. No live filesystem scan during deletion.

**Dry run default:** Audit and plan phases produce JSON evidence only. `delete execute` requires `--receipt` path; destructive ops require explicit confirmation.

**Receipts seal everything:** After deletion, `affidavit_integration` wraps the receipt in a cryptographic chain (`affidavit` crate, `core` feature). `receipt verify` checks claimed vs actual free-space delta and validates OCEL referential integrity.

**OCEL delete event requirements:** Events must carry relationships to `deletion_plan`, `deletion_receipt`, `artifact_candidate`, and `filesystem_object`. Event type must be truthful: `snapshot_delete_requested` for deletes, `snapshot_thin_requested` for thins.

**Doctests are specification:** Domain functions carry doctests with positive + negative + refusal cases. Do not remove or weaken them.

## Dependencies

- **affidavit** (git) — Receipt sealing. Features: `["core"]` **only**. Never enable `discovery`/`conformance` — they pull `wasm4pm` which hard-pins `wasm-bindgen =0.2.100` and conflicts with chrono's transitive dep.
- **wasm4pm-compat** — OCEL v2 types and validation
- **ignore** — Filesystem traversal with barrier support
- **clap-noun-verb** (path dep) — CLI framework
- **libc** — `statvfs` for free-space sampling
- **sled** — Persistent KV store

## Output Files (never commit)

`disk-audit.json`, `*.jsonocel`, `cleanup-plan.json`, `deletion-receipt.jsonocel` — contain absolute paths and local machine state. Covered by `.gitignore`.

## Gall Checkpoints

G0–G7 complete. G8 (privacy gate) and G9 (doctor self-verification) in progress.  
See `docs/GALL_CHECKPOINTS.md`. Do not add capabilities without corresponding receipts.
