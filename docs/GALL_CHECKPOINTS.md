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

## Roadmap to G9
For the current execution plan and status of remaining checkpoints, see the [Gall Checkpoint Roadmap](GALL_ROADMAP.md).