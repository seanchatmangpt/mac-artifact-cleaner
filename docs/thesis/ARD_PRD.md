# Architectural & Product Requirements Document (ARD/PRD)
## Mapping Chatman Fabric and OCPM to osx-clnr and cfab-surface

This document details the alignment between the theoretical concepts of Sean Chatman's doctoral thesis—specifically **Fabric Category Morphisms**, **Object-Centric Process Mining (OCEL 2.0)**, and **Cryptographic Receipts**—and their concrete realization in the `osx-clnr` and `cfab-surface` Rust crates.

---

## 1. Executive Summary & Core Mapping

The core thesis of "The Chatman Equation and Fabric Intelligence" is that local system resources should not be governed by heuristic metrics, but by a deterministic, auditable workflow net where every transition is type-checked and receipted.

The following table establishes the mapping between theoretical concepts and the codebase:

| Thesis Concept | Mathematical Formulation | Rust Crate | Module/Struct |
| :--- | :--- | :---: | :--- |
| **Surface (Digital Resource)** | $S \in \mathbf{Fab}_O$ | `cfab-surface` | `Surface` struct, `SurfaceKind` enum |
| **Relation (Morphism)** | $f: S_1 \rightarrow S_2 \in \mathbf{Fab}_M$ | `cfab-surface` | `Relation` struct, `RelationKind` enum |
| **Fabric (Resource Network)** | $\mathbf{Fab}$ Category / Graph | `cfab-surface` | `Fabric` struct (DiGraph using `petgraph`) |
| **Observation / State** | $O^* \in \mathcal{O}^*$ | `cfab-surface` | `SurfaceState`, `Surface::observe_state()` |
| **Transformation Mapping** | $\mu_B: O^* \rightarrow \mathcal{D}$ | `cfab-surface` | `RelationKind::Transformation` |
| **Evidence Turnstile** | $R_B \vdash A = \mu_B(O^*)$ | `cfab-surface` | `RelationKind::Evidence` |
| **Object-Centric Event Log** | OCEL v2 Tuple $L$ | `osx-clnr` | `domain::ocel::build_disk_audit_ocel` |
| **Deletion Plan** | Plan token state in Place $p_{\text{plan}}$ | `osx-clnr` | `domain::plan::DeletionPlan` |
| **Deletion Receipt** | Receipt token state in Place $p_{\text{del}}$ | `osx-clnr` | `domain::receipt::DeletionReceipt` |
| **Cryptographic Witness** | BLAKE3 Collision Bound $\Omega(2^{128})$ | `osx-clnr` | `domain::receipt::DeletionReceipt::verify()` |
| **Reclaim Reality Law** | measured free-space delta $\Delta$ | `osx-clnr` | `domain::receipt::check_reclaim()`, `ReclaimCheck` |

---

## 2. Structural Realization of the Category $\mathbf{Fab}$

In the `cfab-surface` crate, the category $\mathbf{Fab}$ is constructed as a directed graph representing surfaces and their relations.

### 2.1 Surfaces ($S$)
A `Surface` is uniquely identified by a URI (`url::Url`). The scheme of the URI dictates the resource type:
- `file:///Users/sac/osx-clnr/...` $\rightarrow$ `SurfaceKind::LocalDirectory` (Local POSIX disk directories scanned for artifacts).
- `github://owner/repo` $\rightarrow$ `SurfaceKind::GitHubRepository` (Remote developer VCS states scanned for branches, runs, releases).
- `plan:///Users/sac/osx-clnr/maintenance-plan.json` $\rightarrow$ `SurfaceKind::Plan` (Ineffective dry-run deletion plans).
- `receipt:///Users/sac/osx-clnr/maintenance-receipt.json` $\rightarrow$ `SurfaceKind::Receipt` (Unforgeable post-execution evidence).
- `doc:///Users/sac/osx-clnr/docs/thesis/...` $\rightarrow$ `SurfaceKind::Document` (Explanatory logs and publications).

### 2.2 Relations (Morphisms $f$)
Morphisms connect surfaces and are enforced via directional rules:
- **Dependency:** Direct parent-child links or package configuration requirements.
- **Transformation ($\mu$):** The operation that compiles or maps one surface state into another (e.g., audit scanning mapping a `LocalDirectory` or `GitHubRepository` to a `Plan`).
- **Evidence Turnstile ($R \vdash A$):** The operational gate verifying that an execution receipt was strictly generated from the corresponding plan.

### 2.3 Graph Invariant Rules (Acyclicity & Directionality)
The `Fabric` graph enforces invariants at runtime:
1. **Acyclicity:** Cycles represent infinite caching dependencies or recursive deletion risks and are rejected with `FabricError::CycleDetected`.
2. **Directional Law:**
   - A `Receipt` cannot point back to a `Plan` (it must flow `Plan` $\rightarrow$ `Receipt`).
   - A `Receipt` cannot point directly to a raw `LocalDirectory` or `GitHubRepository` without an intermediate `Plan`.
   - A `Plan` cannot point back to a `GitHubRepository`.

---

## 3. Integration with the `osx-clnr` Engine

To align the codebase with Chatman Fabric, the `osx-clnr` execution pipeline integrates the `cfab-surface` graph:

1. **Observe Phase:**
   - The CLI initiates a scan. `osx-clnr` instantiates a `Surface` node of type `LocalDirectory` or `GitHubRepository`.
   - It performs $O^*$ observation using `Surface::observe_state()`.
2. **Plan Phase:**
   - The dry-run analyzer discovers candidates. It creates a `SurfaceKind::Plan` node.
   - It connects the input source surface to the plan surface via a `RelationKind::Transformation` edge.
3. **Exclusion Phase:**
   - Time Machine exclusions are drafted. An exclusion `Document` Surface node is added.
4. **Execution Phase:**
   - The plan is consumed, deletions are performed, and a `DeletionReceipt` is written.
   - A `SurfaceKind::Receipt` node is added.
   - An `Evidence` relation connects the `Plan` node to the `Receipt` node.

---

## 4. Operational Alignment with OCEL v2 Logs

Object-Centric Event Logs (OCEL 2.0) are utilized to capture the full execution trace:
- **Objects:** Artifact candidates, plans, tool roots, and APFS snapshots are recorded as typed OCEL objects.
- **Events:** `disk_audit_started`, `traversal_barrier_applied`, `deletion_plan_written`, and `artifact_deleted` are events.
- **Synchronizing Transitions:** The OCPN synchronizes the deletion event across the plan object and the multiple directory objects targeted for removal, proving alignment.

---

## 5. Verification & Cryptographic Receipts

The `DeletionReceipt` represents the terminal token state:
- **BLAKE3 Commitments:** Receipts serialize metadata and sign it with a BLAKE3 hash to prevent tampering.
- **Reclaim Delta Verification:** `check_reclaim()` enforces the volumetric delta validation. If the target volume fails to free up space (e.g., because APFS snapshots are pinning blocks), the receipt fails to witness the claim, reporting a `Shortfall`.
