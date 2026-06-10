# Chapter 1: Introduction

## 1.1 The Entropic Nature of Modern Filesystems
The modern polyglot software engineering ecosystem is characterized by an unprecedented velocity of artifact generation. Package managers, build systems, and container runtimes function as isolated, sovereign actors operating upon a shared, stateful medium: the local developer filesystem. Because these tools lack a unified protocol for lifecycle management, they collectively induce a state of unbounded systemic entropy, which we define as the "Bloat Cascade."

Historically, the mitigation of this entropy has relied on heuristic-based, static analysis tools. These tools operate on a snapshot of the filesystem's current state, performing arbitrary size-based aggregations and offering candidates for manual deletion.

## 1.2 The Order Parameter is Standing
The decisive failure of static state governance is not that it is "inefficient," but that it produces the wrong output type. Tools like `ncdu` produce **measurements**: bytes, paths, sizes, and timestamps. 

This dissertation argues that civilization, even at the micro-scale of the developer filesystem, cannot operate securely on measurements alone; it requires artifacts with **standing**. `osx-clnr` proves the transition from measurement to standing. It produces:
* Admitted / Refused decisions
* BLAKE3 cryptographic receipt chains
* Gall Pipeline conformance results
* Object-Centric Event Log (OCEL) causal graphs

Before this transition, an artifact has only measurable properties (path, bytes, mtime). After this transition, the artifact possesses an evidentiary status ($Standing(A) \in \{Admitted, Refused\}$). Only `Admitted` artifacts cross the execution boundary.

## 1.3 The Three Phase-Change Properties
This transition represents a strict phase change, characterized by three properties:

1. **Discontinuity:** There is no smooth interpolation from a heuristic byte-count to a typestate-admitted deletion authority. The output type fundamentally breaks and changes.
2. **Symmetry Breaking:** Before OCEL, the disk is a symmetric namespace of bytes. After OCEL, the namespace is broken into explicit process roles (`tool_root`, `deletion_plan`, `tm_script`). They cease to be labels of convenience and become objects bound by lifecycle constraints.
3. **Emergence:** Below the boundary, questions of causality, conformance, and RL optimization are undefined. Above the boundary, they become native properties of the system. 

## 1.4 Research Questions
To validate this hypothesis, this thesis formally investigates the following Research Questions (RQs):
*   **RQ1 (Ontological Formulation):** How can chaotic filesystem operations be formally modeled and extracted using Object-Centric Event Logs (OCEL 2.0)?
*   **RQ2 (Typestate Safety):** How does the integration of Rust's affine typestate system guarantee the discontinuity of standing?
*   **RQ3 (Alignment-Based Verification):** Can conformance checking perfectly detect non-conforming traces that violate the temporal safety constraints of the Gall Pipeline?
*   **RQ4 (Autonomic Optimization):** How can predictive models applied over the discovered Object-Centric Petri Nets (OCPN) transition the environment to Autonomic Optimization?

## 1.5 The Mundanity of the Proof Domain
Applying process intelligence to enterprise ERP or medical records relies on the inherent prestige and process-rich nature of those domains. Disk cleanup, however, is offensively mundane: find a big folder, delete it, free space. 

By proving the phase change from measurement to standing in the most mundane domain possible—the developer filesystem—we demonstrate that the transition is produced strictly by the mechanism, not borrowed from the domain. This establishes the universality of the framework.
