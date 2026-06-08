# Chapter 1: Introduction

## 1.1 The Perils of Improvisational Destruction

Traditional utility scripts designed for system maintenance, particularly disk cleanup tools, frequently suffer from a critical architectural flaw: the coupling of real-time discovery with immediate destruction. In a standard workflow, a tool scans the filesystem, identifies a target matching a heuristic (e.g., a folder named `node_modules` or `target`), and immediately issues an `rm -rf` command.

This pattern, which we term "improvisational destruction," is inherently unsafe. It assumes that the heuristics of discovery are infallible and that the state of the filesystem remains static between the moment of discovery and the moment of deletion. When failures occur—such as traversing into unintended symlinked directories or deleting vital tool-root caches—the system leaves no trace of its reasoning, only the absence of data.

The consequences of improvisational destruction on developer machines include:
- Unrecoverable loss of local configuration (`.config`, `.docker`).
- Wasted computational cycles traversing massive, already-condemned trees.
- Silent failures where files are deleted but disk space is not reclaimed due to hidden OS-level mechanics like APFS local snapshots.

## 1.2 The Plan-Bound Architecture

To address these perils, `osx-clnr` abandons improvisational destruction in favor of a **Plan-Bound Architecture**.

The core premise is that a utility must never increase its destructive power without simultaneously increasing its capacity to produce verifiable receipts. The system must:
1. **Observe first:** Perform a read-only scan.
2. **Emit evidence:** Produce an Object-Centric Event Log (OCEL).
3. **Propose a plan:** Generate a dry-run JSON artifact detailing proposed deletions.
4. **Execute strictly:** Disable discovery mechanisms; act *only* on the verified paths listed in the user-reviewed plan.
5. **Provide a receipt:** Document the exact consequences of the execution phase.

This methodology shifts the operational paradigm from an opaque, real-time script to an auditable, governed pipeline. The following chapters will explore how Gall's Law provides the necessary evolutionary framework to construct such a pipeline safely.
