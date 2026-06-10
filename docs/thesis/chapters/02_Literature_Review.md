# Chapter 2: Literature Review and State of the Art

## 2.1 The Evolution of Process Mining
Process mining emerged in the late 1990s as a discipline designed to extract knowledge from event logs readily available in today's information systems. The seminal works of Dr. Wil van der Aalst established the three primary capabilities of process mining: Process Discovery, Conformance Checking, and Process Enhancement.

## 2.2 Object-Centric Process Mining (OCPM) and OCEL 2.0
To address limitations surrounding the single case identifier, van der Aalst introduced Object-Centric Process Mining (OCPM) in 2019. OCEL 2.0 formalized this standard, allowing events to relate to multiple object types simultaneously. While OCPM has seen rapid adoption in ERP systems, its application to low-level OS semantics remains largely unexplored. This dissertation represents a pioneering effort to apply OCEL 2.0 ontologies to the chaotic domain of POSIX filesystems.

## 2.3 Typestate Programming and Compile-Time Safety
The concept of typestate programming was introduced by Strom and Yemini in 1986. Typestate tracking extends traditional type checking by associating a state with a variable. In modern systems engineering, Rust has popularized typestate enforcement through its affine type system and ownership semantics.

## 2.4 Synthesis: Typestate-Enforced Process Mining
This dissertation synthesizes OCPM with typestate programming. While process mining traditionally operates *a posteriori*, we propose an architecture where the formal constraints of the process model are enforced *a priori* by the typestate compiler. The artifact cannot proceed to the deletion engine unless it mathematically satisfies the conformance rules, bridging descriptive process mining with deterministic systems safety.

## 2.5 The Universal Chatman Equation and Cross-Domain Generalization
This work does not exist in isolation. It forms the empirical validation of the Universal Chatman Equation, $\mathcal{A}_{\mathcal{U}} = \mu_{\mathcal{U}}(\mathcal{O}^*_{\mathcal{B}})$, which posits that raw, continuous observations ($\mathcal{O}^*_{\mathcal{B}}$) can be functorially mapped to discrete, unforgeable evidence ($\mathcal{A}_{\mathcal{U}}$) via a structured transformation mechanism ($\mu_{\mathcal{U}}$). 

The Chatman Equation was previously formalized over programming language domains (e.g., `tower-lsp-max`) and symbolic language families via the Universal Semantic Physics Engine. The present work demonstrates that the identical transformation applies to the foundational filesystem domain. We prove that applying $\mu$ over the OCEL ontology $\mathcal{O}^*_{\text{fs}}$ produces process evidence with the same formal guarantees: $O(1)$ admission, cryptographic receipts, LTL-certified conformance, and causal auditability. The sheer domain generality—scaling from symbolic language semantics down to raw POSIX disk manipulation—is the definitive proof of the equation's universality.
