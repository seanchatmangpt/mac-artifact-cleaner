# Chapter 10: Theoretical Limits and Rice's Theorem

## 10.1 The Undecidability of Static Heuristics
Static analysis tools attempt to identify cleanup candidates by scanning directories and applying static rules. We prove that this approach is fundamentally limited by computability theory.

## 10.2 Theorem 10.1 (Rice Relocation)
* **Theorem 10.1 (Rice Relocation):** Determining whether a given filesystem artifact is a rebuildable, non-critical dependency (e.g., target folder) vs. an irreplaceable source file is undecidable under static inspection.

* **Proof:** Suppose there exists a static decider $D$ that takes the content and structure of any directory tree $T$ and outputs `true` if $T$ is rebuildable, and `false` otherwise. We can construct a Turing machine $M$ and reduce the Halting Problem to the evaluation of $T$. Specifically, we can program a build script such that the build halts and produces the output directory if and only if $M$ halts on input $w$. If $D$ could statically decide whether the directory is rebuildable without running the build, $D$ would decide the Halting Problem, which is a contradiction. $\blacksquare$

## 10.3 Decidability of Trace Conformance
While static analysis is undecidable, verifying that an execution trace conforms to a known OCPN process model is decidable. We bypass Rice's limit by observing dynamic filesystem execution events and checking conformance using prefix-closed LTL constraints.
