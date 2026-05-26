# Thesis Abstract & Table of Contents

**Title:** Gall’s Law in Systems Engineering: The Evolution of Receipt-Driven Disk Management
**Author:** mac-artifact-cleaner Architectural AI
**Date:** May 2026

## Abstract

This thesis explores the application of Gall's Law—"A complex system that works is invariably found to have evolved from a simple system that worked"—within the context of a modern, plan-bound macOS disk cleanup utility (`mac-artifact-cleaner`). Unlike traditional disk scanners that couple discovery with immediate destructive action (e.g., `rm -rf`), this system enforces an "execution-trust pipeline." This thesis argues that by treating the filesystem as an untrusted domain and enforcing strict layer isolation, systems can achieve higher reliability. We define a novel concept, the "Gall Checkpoint," which acts as an evolutionary gate requiring capability, evidence, constraint, and receipt before allowing architectural progression. Through the lens of this project, we demonstrate that system complexity should only advance when the preceding operational layer has produced verifiable, object-centric evidence (OCEL v2).

## Table of Contents

1. [Chapter 1: Introduction](01_INTRODUCTION.md)
   - 1.1 The Perils of Improvisational Destruction
   - 1.2 The Plan-Bound Architecture
2. [Chapter 2: Gall's Law in Software Construction](02_GALLS_LAW.md)
   - 2.1 Theoretical Foundations
   - 2.2 The Gall Checkpoint Model
   - 2.3 Evolutionary Milestones (G0 to G9)
3. [Chapter 3: The Receipted Execution Pipeline](03_RECEIPTED_EXECUTION.md)
   - 3.1 Observation vs. Action
   - 3.2 The Role of OCEL v2 Evidence
   - 3.3 Time Machine and System Substrates
4. [Chapter 4: Architectural Isolation](04_ARCHITECTURAL_ISOLATION.md)
   - 4.1 CLI, Domain, and Integration Boundaries
   - 4.2 The Necessity of Inert DTOs (EntrySnapshot)
   - 4.3 The "Doctor" as an Architectural Gatekeeper
5. [Chapter 5: Conclusion](05_CONCLUSION.md)
   - 5.1 Synthesis
   - 5.2 Future Applications of the Execution-Trust Pipeline
