# Chapter 3: Mathematical Formalisms and Ontologies

## 3.1 Formalizing the Filesystem as an OCEL 2.0 Tuple
To apply process mining techniques to the local disk, we must first define a rigorous mapping from raw POSIX filesystem operations to the OCEL 2.0 ontology.

We define the Object-Centric Event Log as a tuple $L = (E, O, T_E, T_O, \pi_{type}, \pi_{rel}, \pi_{time}, \pi_{attr})$, where:
*   $E$ is the set of events (e.g., $e_1 = \text{artifact\_deleted}$, $e_2 = \text{tm\_exclusion\_applied}$).
*   $O$ is the set of objects (e.g., $o_1 = \text{/target}$, $o_2 = \text{/node\_modules}$).
*   $T_E = \{ \text{scan\_started}, \text{artifact\_proposed}, \text{deletion\_plan\_created}, \text{tm\_exclusion}, \text{artifact\_deleted}, \text{snapshot\_thin} \}$ is the set of Event Types.
*   $T_O = \{ \text{tool\_root}, \text{project\_root}, \text{deletion\_plan}, \text{tm\_script}, \text{snapshot\_state} \}$ is the set of Object Types.
*   $\pi_{type}$ maps an event or object to its respective type.
*   $\pi_{rel}: E \rightarrow \mathcal{P}(O)$ maps an event to a set of related objects. For example, a `deletion_plan_created` event relates to all `tool_root` objects proposed for deletion.
*   $\pi_{time}: E \rightarrow \mathcal{T}$ maps an event to a precise Unix timestamp.
*   $\pi_{attr}$ maps events and objects to physical attributes (e.g., `bytes_reclaimed`, `category`).

## 3.2 The Gall Checkpoint Pipeline
The Gall Checkpoint Pipeline is the normative safety model governing destructive operations in `osx-clnr`. It asserts that no file can be deleted without a preceding explicit plan, and no file can be deleted without first being excluded from Time Machine backups to prevent "ghost bloat."

We formalize these constraints using Linear Temporal Logic (LTL) semantics within a Declare model. Let $\Sigma$ be the alphabet of event types $T_E$.

**Constraint 1: Safe Planning (Precedence)**
A deletion plan must be created before any artifact is deleted.
$\text{Precedence}(\text{deletion\_plan\_created}, \text{artifact\_deleted}) \equiv$
$\square ( \text{artifact\_deleted} \rightarrow \lozenge_{\leq 0} \text{deletion\_plan\_created} )$

**Constraint 2: Time Machine Safety (Response)**
If a deletion plan is created, a Time Machine exclusion plan must eventually be written.
$\text{Response}(\text{deletion\_plan\_created}, \text{tm\_exclusion}) \equiv$
$\square ( \text{deletion\_plan\_created} \rightarrow \lozenge \text{tm\_exclusion} )$

**Constraint 3: Direct Execution (Chain Succession)**
An artifact deletion must immediately follow the application of a Time Machine exclusion for that specific artifact, ensuring no state drift occurs between planning and execution.
$\text{ChainSuccession}(\text{tm\_exclusion}, \text{artifact\_deleted}) \equiv$
$\square ( \text{tm\_exclusion} \rightarrow \bigcirc \text{artifact\_deleted} ) \land \square ( \text{artifact\_deleted} \rightarrow \ominus \text{tm\_exclusion} )$

Any trace $\sigma \in L$ that violates these LTL constraints is mathematically flagged by the `wpm audit` Alignment-Based Conformance Checker as a non-conforming trace.