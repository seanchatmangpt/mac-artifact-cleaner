# Glossary of Foundational Axioms, Laws, and Proofs

To ground the architectural and systemic claims of this dissertation in formal, inarguable mathematical logic, we explicitly define the following universally established axioms, theorems, and laws. The efficacy of `osx-clnr` does not rely on novel theoretical physics, but rather on the novel application of these incontrovertible proofs to the local filesystem domain.

### 1. Rice's Theorem (Static Analysis Undecidability)
**Definition:** Formulated by Henry Gordon Rice in 1953, Rice's Theorem states that all non-trivial semantic properties of programs (and by extension, the generative output of Turing machines) are undecidable.
**Application:** This provides the inarguable mathematical proof for why static, heuristic-based disk cleaners *must* fail. It is formally undecidable to determine whether a given artifact on disk is an implicit dependency required by a future build process purely via static analysis. Process mining (dynamic, causal observation) is the mandatory workaround to Rice's limitation.

### 2. The Curry-Howard Isomorphism (Propositions as Types)
**Definition:** A direct relationship between computer programs and mathematical proofs, establishing that a type signature is equivalent to a logical proposition, and a well-typed program is equivalent to a constructive proof of that proposition.
**Application:** This is the inarguable foundation of the typestate `$h_I$` boundary. When the `osx-clnr` Rust compiler guarantees that a `DeletionPlan` is of type `Admitted`, it is not merely executing a runtime check; it is providing a constructive, mathematical proof that the plan satisfies the safety proposition.

### 3. Little's Law ($L = \lambda W$)
**Definition:** In queuing theory, Little's Law states that the long-term average number of items in a stationary system ($L$) is equal to the long-term average effective arrival rate ($\lambda$) multiplied by the average time that an item spends in the system ($W$).
**Application:** This is the undeniable governing equation for filesystem entropy (The Bloat Cascade). The total disk space consumed by artifacts ($L$) can only be optimized by reducing the generation rate ($\lambda$) or decreasing the time-to-deletion ($W$). `osx-clnr` explicitly targets $W$ via process efficiency metrics.

### 4. Cryptographic Collision Resistance (The Birthday Bound)
**Definition:** A property of cryptographic hash functions (like BLAKE3) guaranteeing that it is computationally infeasible to find two distinct inputs $x$ and $y$ such that $H(x) = H(y)$. By the Pigeonhole Principle and the Birthday Paradox, an $n$-bit hash function requires $\Omega(2^{n/2})$ evaluations to find a collision.
**Application:** This forms the inarguable foundation of the `ReceiptChain` and `ReceiptEnvelope`. It mathematically guarantees that post-execution tampering of deletion logs is impossible against any computationally bounded adversary.

### 5. The Bellman Equation (Dynamic Programming)
**Definition:** A necessary condition for optimality associated with the mathematical optimization method known as dynamic programming, providing a recursive definition for the value function of a Markov Decision Process (MDP):
$V^\pi(s) = R(s, \pi(s)) + \gamma \sum_{s'} P(s' | s, \pi(s)) V^\pi(s')$
**Application:** This establishes the absolute mathematical foundation for Chapter 6's Autonomic RL Ecosystem. By formalizing disk cleanup as an MDP, the Bellman Equation guarantees that an optimal autonomic deletion policy *can* be mathematically discovered over time.

### 6. Soundness of Workflow Nets (Petri Net Theory)
**Definition:** In Petri Net theory (specifically Workflow Nets introduced by van der Aalst), *soundness* requires that from any reachable state, the terminal state can be reached (option to complete), the terminal state is the only marked state when reached (proper completion), and there are no dead transitions.
**Application:** This provides the undeniable structural proof that the Object-Centric Petri Net (OCPN) discovered from `osx-clnr` logs is free of infinite caching loops and deadlocks, ensuring artifacts can logically progress to deletion.

---
