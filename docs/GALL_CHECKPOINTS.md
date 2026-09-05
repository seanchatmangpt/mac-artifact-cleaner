# Gall Checkpoints: The Evolution of Pentecost

Gall’s Law states that a complex working system is almost always found to have evolved from a simple working system. The key is not the platitude "start small." The key is:

> **Do not advance system complexity until the previous operational layer has produced evidence.**

This project is a perfect exemplar of this principle. Every new requirement emerged directly from contact with reality. Instead of engaging in feature creep (e.g., adding more delete patterns or more aggressive `rm -rf` behavior), the system evolved through a lawful construction path:

```text
observe → constrain → receipt → review → act → verify
```

## The Core Gall Checkpoint Definition

A **Gall Checkpoint** is a bounded operational milestone where the system must prove that one layer works under real-world conditions before a more powerful layer is admitted.

Each checkpoint is defined by five gates:
*   **Capability:** What new thing can it do?
*   **Evidence:** What observed reality justified it?
*   **Constraint:** What can it no longer do unsafely?
*   **Receipt:** What artifact proves what happened?
*   **Promotion Rule:** What allows the next layer to exist?

---

## The Evolutionary Checkpoints of this Project

### G0 — Simple Artifact Cleaner
*   **Capability:** "Remove build artifacts."
*   **Evidence:** Basic need to reclaim disk space.

### G1 — Language/Project Detection
*   **Capability:** Detect Node, Python, Rust, Java, Go, Erlang, Elixir, Next, Nuxt.
*   **Evidence:** Generic `build` folder deletion is highly unsafe.

### G2 — Traversal Barriers
*   **Capability:** Detect massive dependency folders (`node_modules`, `target`, `.next`, etc.) but do not walk inside them.
*   **Evidence:** The scanner felt stuck and wasted immense amounts of time computing deep trees that were already marked for deletion.

### G3 — Dry-Run Plan File
*   **Capability:** Dry run writes a reviewable deletion plan. Delete phase only reads that plan.
*   **Evidence:** Live deletion from a fresh, real-time scan is far too dangerous and unpredictable.
*   **Constraint:** Scanner cannot delete directly.
*   **Receipt:** `cleanup-plan.json`
*   **Promotion Rule:** Delete mode is allowed only if it reads a structurally valid, user-reviewed plan file.

### G4 — Disk Inventory & UX Visibility
*   **Capability:** Measure where the 600 GB actually lives; add spinners, byte-rates, and discrete execution phases.
*   **Evidence:** "It looks stuck." Build artifacts alone were not the whole problem, and silent failures look identical to slow execution.

### G5 — Time Machine / APFS Snapshot Awareness
*   **Capability:** Delete live files, then thin snapshots. 
*   **Evidence:** The script successfully cleared 300GB of files, but Disk Utility showed no reclaimed space because APFS snapshots pinned the deleted blocks.

### G6 — Root-Tool Aging Analysis
*   **Capability:** Inspect and classify hidden infrastructure (`.gemini`, `.cargo`, `.cache`, `.rustup`, Docker, model stores, etc.) based on size and the age of the newest descendant.
*   **Evidence:** Not all large files are project artifacts; many are obsolete tool states that require distinct judgment logic rather than blanket deletion.

### G7 — OCEL v2 Reporting
*   **Capability:** Emit object-centric evidence (tool roots, files, folders, events, candidates, plans, receipts) rather than just standard logging.
*   **Evidence:** True system health requires review, aging context, causality, and decision support, not just `du` outputs. 

### G8 — Privacy / Redaction Gate
*   **Capability:** Redact local machine evidence and prevent accidental publication.
*   **Evidence:** Real reports contain usernames, absolute paths, project names, hidden tool roots.
*   **Constraint:** Real reports/plans/receipts cannot enter docs/examples/releases unredacted.
*   **Receipt:** Privacy scan report.
*   **Promotion Rule:** doctor privacy reports 0 violations.

### G9 — Doctor / Self-Verification
*   **Capability:** Tool verifies its own operating law.
*   **Evidence:** As capabilities increased, architecture drift became possible.
*   **Constraint:** Release/promotion blocked if architecture, privacy, OCEL, or destructive-action receipt checks fail.
*   **Receipt:** Doctor report.
*   **Promotion Rule:** doctor architecture + doctor privacy + doctor ocel all pass.

### G10 — Unattended Autoclean & CLI-Only Approval
*   **Capability:** `oclnr plan approve` gives the CLI (not just the MCP server) a path to HMAC-sign a plan, and `oclnr autoclean run` chains `plan build → plan approve → delete execute → receipt verify` as a single unattended pass. `oclnr daemon install-autoclean` schedules it daily via a separate `com.oclnr.autoclean` launchd job (distinct from the pre-existing alert-only `com.oclnr.monitor` job, which never deletes).
*   **Evidence:** Every prior checkpoint assumed a human runs each stage interactively and reviews the plan before approval; there was no way for a scheduled/non-interactive caller to complete the pipeline at all, since only the MCP server could sign a plan.
*   **Constraint:** Autoclean is strictly more conservative than an interactive session — a hard `--max-reclaim-gb` cap (default 50) refuses rather than executes any plan claiming more; any plan containing an Unknown/Irreversible-reversibility item is skipped and logged, never auto-acknowledged; `--ignore-recent-hours` defaults to 24h; it never touches Docker/Colima or a wholesale `~/Library/Caches`.
*   **Receipt:** Every run's plan/receipt file paths plus a one-line summary appended to `~/Library/Logs/oclnr/autoclean.log`, so an unattended run is still fully auditable after the fact.
*   **Promotion Rule:** Unattended execution is admitted only once plan-bound deletion (G3), snapshot awareness (G5), and receipted verification (G3/G9) already hold — it adds no new deletion mechanism, only a scheduler and safety caps around the existing one.

---

## The Principle of Receipted Execution

The first version was merely a cleaner. Its current shape is an **execution-trust pipeline**:

```text
filesystem observation
  → artifact classification
  → root-tool inventory
  → age/size/update evidence
  → OCEL report
  → reviewable plan
  → deletion from plan only
  → deletion receipt
  → snapshot thinning verification
```

This sequence proves the core architectural law of the project:

**Never increase destructive power without simultaneously increasing receipts.**

---

---

## Current Status (June 2026)

| Checkpoint | Status | Notes |
|---|---|---|
| G0 Simple Artifact Cleaner | ✅ Complete | |
| G1 Language/Project Detection | ✅ Complete | |
| G2 Traversal Barriers | ✅ Complete | `traversal_barrier_names()`, doctor architecture gate |
| G3 Dry-Run Plan File | ✅ Complete | plan-bound deletion, receipt verify |
| G4 Disk Inventory & UX | ✅ Complete | `VolumeSpace`/`statvfs`, free-space header, bytes accounting |
| G5 Time Machine / Snapshots | ✅ Complete | `snapshot audit/thin/delete`, `emergency`, `check_reclaim` law |
| G6 Root-Tool Aging Analysis | ✅ Complete | `tool-roots audit`, `recommend_tool_root`, `--include-global-caches` |
| G7 OCEL v2 Reporting | ✅ Substantially complete | All operations emit OCEL; `snapshot_delete_requested` distinct from thin |
| G8 Privacy / Redaction Gate | 🔄 In progress | `doctor privacy` exists; auto-redaction path not yet wired |
| G9 Doctor / Self-Verification | 🔄 In progress | `doctor architecture/substrate/doctests` pass; full G9 promotion rule pending |
| G10 Unattended Autoclean | ✅ Complete | `plan approve` (CLI), `autoclean run`, `daemon install-autoclean` (`com.oclnr.autoclean`), safety cap + reversibility gate + log receipt |

## Roadmap to G9
For the current execution plan and status of remaining checkpoints, see the [Gall Checkpoint Roadmap](GALL_ROADMAP.md).