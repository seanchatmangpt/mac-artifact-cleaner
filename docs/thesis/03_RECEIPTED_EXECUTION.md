# Chapter 3: The Receipted Execution Pipeline

## 3.1 Observation vs. Action

The fundamental law of the `mac-artifact-cleaner` is:

> **Never increase destructive power without simultaneously increasing receipts.**

This requires a strict temporal and architectural separation between observation and action.
In a traditional tool, observation and action are interleaved in a tight loop:
`[Observe path] -> [Match Rule] -> [Delete path]`

In the receipted execution pipeline, they are severed:
`[Observe] -> [Classify] -> [Report] -> [Plan] ... (Human Review Gate) ... [Validate] -> [Act] -> [Receipt]`

During the **Act** phase, the scanner is explicitly disabled. The integration layer receives a list of absolute, verified paths from the Domain layer. It cannot improvise. It cannot say, "While I am deleting this directory, I noticed another one next to it; I will delete that too."

## 3.2 The Role of OCEL v2 Evidence

Simple logging is insufficient for a complex state-mutating system. Traditional logs are flat, string-based, and difficult to query for causality.

We utilize the Object-Centric Event Log (OCEL v2) standard to provide multi-dimensional evidence. Every major operation emits an OCEL report.
- A **Disk Audit** is an object.
- A **File System Directory** is an object.
- A **Deletion Plan** is an object.
- A **Deletion Event** links the Disk Audit, the File System Directory, and the Deletion Plan.

This means that after execution, an auditor can query the receipt to understand exactly *why* a specific directory was deleted, which plan authorized it, and what the state of the disk was when the plan was formulated.

## 3.3 Time Machine and System Substrates

The pipeline proves its worth when interacting with opaque OS subsystems like APFS local snapshots via `tmutil`.

Deleting 300GB of `node_modules` often reclaims zero bytes of visible disk space on macOS if Time Machine has pinned the filesystem state via an APFS snapshot. The naive approach is to add a silent `tmutil thinning` call to the end of the script.

Following our core law, this is forbidden. Snapshot thinning is a destructive action against the backup substrate. Therefore, it requires the same pipeline:
1. Observe the snapshot state.
2. Formulate a snapshot thin plan (or append to the existing deletion plan).
3. Apply the plan explicitly.
4. Emit a snapshot thin receipt.

This ensures the user is always aware of when and why their backup state is being mutated.
