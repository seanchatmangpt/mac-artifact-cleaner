# Chapter 3: Mathematical Formalisms and Ontologies

## 3.1 Formalizing the Filesystem as an OCEL 2.0 Tuple
To apply process mining to the local disk, we define a rigorous mapping from POSIX operations to the OCEL 2.0 ontology.
Let $L = (E, O, T_E, T_O, \pi_{type}, \pi_{rel}, \pi_{time}, \pi_{attr})$:
*   $E$: set of events ($e \in E$).
*   $O$: set of objects ($o \in O$).
*   $T_E = \{ \text{scan\_started}, \text{artifact\_proposed}, \text{deletion\_plan\_created}, \text{tm\_exclusion}, \text{artifact\_deleted}, \text{snapshot\_thin} \}$.
*   $T_O = \{ \text{tool\_root}, \text{project\_root}, \text{deletion\_plan}, \text{tm\_script}, \text{snapshot\_state} \}$.
*   $\pi_{type}$: maps event/object to its type.
*   $\pi_{rel}: E \rightarrow \mathcal{P}(O)$: maps an event to related objects.
*   $\pi_{time}: E \rightarrow \mathcal{T}$: maps to Unix timestamps.

### 3.1.1 Completeness and Soundness of the OCEL Mapping
For this formalization to hold mathematical weight, the mapping $f: \text{POSIX\_Op} \rightarrow E$ must be complete and sound.
*   **Lemma 1 (Completeness and Event Abstraction):** Every POSIX operation that materially changes a tracked artifact maps to some $e \in E$. Crucially, this mapping performs **Event Abstraction**. It filters and aggregates stochastic, high-frequency POSIX interrupts (e.g., `open`, `write`, `close`, `stat`) into deterministic, macroscopic lifecycle events (e.g., `artifact_created`). By hooking into the filesystem polling layer via this abstraction, no destructive or exclusionary OS event drops silently.
*   **Lemma 2 (Soundness):** Every $e \in E$ corresponds to an actual POSIX operation that occurred. Because $E$ emissions are inextricably bound to cryptographic receipt creation, no phantom events can be generated. The emission of $e$ requires the resolution of its physical OS counterpart.

## 3.2 Formal OCPN Construction
To discover and check conformance, we construct the formal Object-Centric Petri Net (OCPN) $\mathcal{N} = (P, T, F, M_0, \mathcal{O}, \pi)$:
*   **Places $P$:** Artifact states $\{p_{\text{raw}}, p_{\text{cand}}, p_{\text{plan}}, p_{\text{excl}}, p_{\text{del}}, p_{\text{refused}}\}$.
*   **Transitions $T$:** The event types $T_E$.
*   **Object binding $\pi$:** Maps object types to specific transition arcs (e.g., $T_{\text{artifact\_deleted}}$ consumes from $p_{\text{excl}}$ and places tokens into $p_{\text{del}}$ for objects of type `tool_root`). Crucially, the binding $\pi$ employs **variable arc weights** and **Synchronizing Transitions**, allowing a single transition to dynamically consume, produce, and synchronize an arbitrary number of object tokens across different $T_O$ object types (e.g., synchronizing 1 `deletion_plan` token with 50,000 `filesystem_object` tokens) during batch operations.
*   **Marking $M_0$:** The initial state representing identified artifacts on disk.

*   **Theorem (Soundness & Liveness of $\mathcal{N}$):** The constructed OCPN is *sound* (no transition fires without all required typed object tokens) and *live* (every artifact token in $M_0$ that reaches $p_{\text{plan}}$ is guaranteed to eventually sink into $p_{\text{del}}$ or $p_{\text{refused}}$).

## 3.3 The Gall Checkpoint Pipeline and LTL Completeness
The Gall Checkpoint Pipeline asserts that no file can be deleted without an explicit plan and prior Time Machine exclusion. We formalize this using Linear Temporal Logic (LTL):
1.  **$\Phi_1$ (Precedence):** $\square ( \text{artifact\_deleted} \rightarrow \lozenge_{\leq 0} \text{deletion\_plan\_created} )$
2.  **$\Phi_2$ (Response):** $\square ( \text{deletion\_plan\_created} \rightarrow \lozenge \text{tm\_exclusion} )$
3.  **$\Phi_3$ (Chain Succession):** $\square ( \text{tm\_exclusion} \rightarrow \bigcirc \text{artifact\_deleted} )$

**LTL Constraint Set Completeness Analysis:**
Are these three constraints sufficient to capture the full safety policy? Yes. 
If an arbitrary deletion occurs that feels "unsafe," it must fall into one of three categories:
1. It was not intended (Violates $\Phi_1$, as no plan was created).
2. It causes unrecoverable ghost bloat in backups (Violates $\Phi_2$).
3. It suffered a race condition or state drift between planning and execution (Violates $\Phi_3$).
Because any theoretically unsafe operation mapping to $T_{\text{artifact\_deleted}}$ reduces to one of these three structural failures, the constraint set $\{\Phi_1, \Phi_2, \Phi_3\}$ is complete.
