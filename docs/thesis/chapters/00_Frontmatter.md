# Declaration of Authorship

I, **Sean Chatman**, declare that this thesis titled, *"Formalizing Filesystem Lifecycle Semantics: An Object-Centric Process Mining Framework for Autonomic Artifact Management"* and the work presented in it are my own. I confirm that:

* This work was done wholly or mainly while in candidature for a research degree.
* Where any part of this thesis has previously been submitted for a degree or any other qualification at this University or any other institution, this has been clearly stated.
* Where I have consulted the published work of others, this is always clearly attributed.
* Where I have quoted from the work of others, the source is always given. With the exception of such quotations, this thesis is entirely my own work.
* I have acknowledged all main sources of help.
* Where the thesis is based on work done by myself jointly with others, I have made clear exactly what was done by others and what I have contributed myself.

**Signed:** *Sean Chatman*  
**Date:** June 2026

\newpage

# Acknowledgements

I would like to express my deepest appreciation to my advisors and the broader Process Mining community. In particular, the foundational research of Prof. dr. ir. Wil M.P. van der Aalst on Object-Centric Process Mining provided the mathematical ontology required to map continuous filesystem entropy to discrete, executable process logic. I am also deeply indebted to the open-source Rust community for developing the affine typestate compiler guarantees that made the `$h_I$` safety boundary possible. 

Finally, I dedicate this work to every software engineer who has lost days of productivity to Cache Thrashing and silent filesystem bloat. May your future artifacts always possess standing.

\newpage

# Abstract

The lifecycle of local software artifacts is currently managed through ad-hoc, heuristic-based scripts, leading to unbounded storage bloat, broken dependencies, and opaque data loss. While Process Mining has revolutionized the discovery and conformance checking of enterprise workflows, its application to low-level operating system semantics remains largely unexplored. Prior work on filesystem governance treats deletion as a terminal operation outside the process model. We prove that deletion is a typed state transition admissible to the same OCEL (Object-Centric Event Log) ontology as creation, modification, and access. The receipt chain extends the process evidence boundary to include destructive operations for the first time.

This thesis introduces a mathematically rigorous framework that applies Object-Centric Process Mining (OCPM) to local disk lifecycle management. By enforcing the deterministic "Gall Pipeline" via Rust's typestate system and securing executions with unforgeable BLAKE3 receipt chains, we guarantee the safety of autonomic deletion policies. Through the integration of the `wasm4pm` engine, we formally evaluate alignment-based conformance checking over $N \ge 1,000$ traces. We demonstrate that process intelligence can transition filesystem management from static measurement to autonomic, evidentiary governance.

\newpage
