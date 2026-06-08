# Chapter 2: Gall's Law in Software Construction

## 2.1 Theoretical Foundations

John Gall, in his seminal work *Systemantics*, posited that "A complex system that works is invariably found to have evolved from a simple system that worked. A complex system designed from scratch never works and cannot be patched up to make it work. You have to start over with a working simple system."

In modern software engineering, this is often interpreted merely as an agile endorsement to "start small." However, applied rigorously to system safety and automation, Gall's Law demands a strict progression of verified capabilities. You cannot build a system that manages complex APFS snapshot exclusions if it cannot reliably and safely delete a simple node module without traversing it entirely.

## 2.2 The Gall Checkpoint Model

To operationalize Gall's Law in the `osx-clnr` architecture, we define the concept of the **Gall Checkpoint**. A checkpoint is an evolutionary lock. It prevents the system from advancing to a more complex state until the current state has proven its reliability through evidence.

A Gall Checkpoint consists of five gates:
1.  **Capability:** The new operational power (e.g., deleting a file).
2.  **Evidence:** The real-world observation that justifies the capability.
3.  **Constraint:** The hard boundary placed on the system to prevent unsafe usage of the new capability (e.g., deletion can only happen from a plan).
4.  **Receipt:** The durable, reviewable artifact that proves what the capability did.
5.  **Promotion Rule:** The specific, automated test or diagnostic that must pass before the next capability can be developed.

## 2.3 Evolutionary Milestones (G0 to G9)

The project’s evolution is mapped strictly to these checkpoints:

*   **G0-G2 (Observation & Classification):** The system learned to find build artifacts, detect project types, and respect traversal barriers to avoid algorithmic bloat.
*   **G3 (Plan-Bound Actuation):** The introduction of the dry-run plan. The crucial constraint was established: the scanner cannot delete.
*   **G4-G6 (Deep System Awareness):** The system evolved to understand not just artifacts, but inventory sizes, tool-root aging (e.g., `.cargo`, `.docker`), and Time Machine APFS snapshots. Crucially, snapshot thinning was only allowed because G3 proved plan-bound actuation worked.
*   **G7-G9 (Governance & Evidence):** The final evolution introduces Object-Centric Event Logging (OCEL v2) to track complex state changes across objects, enforces a Privacy Redaction Gate to prevent telemetry leaks, and deploys the "Doctor" tool to continually verify the system's architectural integrity.
