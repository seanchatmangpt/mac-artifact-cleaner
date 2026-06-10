# Chapter 2: Literature Review and State of the Art

## 2.1 The Evolution of Process Mining
Process mining emerged in the late 1990s as a discipline designed to extract knowledge from event logs readily available in today's information systems. The seminal works of Dr. Wil van der Aalst established the three primary capabilities of process mining: Process Discovery, Conformance Checking, and Process Enhancement.

Historically, process mining techniques operated under a fundamental limitation: the necessity of a single case identifier (e.g., a patient ID in a hospital, an order ID in an e-commerce system). This "flat" event log structure forces analysts to flatten relational data, leading to the well-documented phenomena of convergence (an event being replicated multiple times for different objects) and divergence (the inability to distinguish which specific object within a case an event pertains to).

## 2.2 Object-Centric Process Mining (OCPM) and OCEL 2.0
To address these limitations, van der Aalst introduced Object-Centric Process Mining (OCPM) in 2019. OCPM liberates process mining from the tyranny of the single case identifier. In an Object-Centric Event Log (OCEL), an event can refer to any number of objects, and those objects can possess arbitrary properties. 

OCEL 2.0 further formalized this standard, introducing robust relational mapping between event types and object types. While OCPM has seen rapid adoption in enterprise resource planning (ERP) systems (e.g., SAP, Celonis), its application to low-level operating system semantics—specifically the lifecycle of transient developer artifacts—remains largely unexplored in the literature. This dissertation represents a pioneering effort to apply OCEL 2.0 ontologies to the chaotic domain of POSIX filesystems.

## 2.3 Typestate Programming and Compile-Time Safety
The concept of typestate programming was introduced by Strom and Yemini in 1986. Typestate tracking extends traditional type checking by associating a state with a variable; the set of valid operations on that variable depends strictly on its current state.

In modern systems engineering, the Rust programming language has popularized typestate enforcement through its affine type system and ownership semantics. Developers can encode states as distinct types (e.g., `RawData`, `ValidatedData`), ensuring that transitions only occur via specific, vetted functions. If a function requires `ValidatedData`, passing `RawData` results in a compile-time failure.

## 2.4 Synthesis: Typestate-Enforced Process Mining
This dissertation synthesizes OCPM with typestate programming. While process mining traditionally operates *a posteriori*—analyzing logs after execution has occurred—we propose an architecture where the formal constraints of the process model are enforced *a priori* by the typestate compiler.

By implementing an `Admit` boundary (inspired by the `wasm4pm-compat` crate), the transition of an artifact's state from `Raw` to `Admitted` acts as a physical gate. The artifact cannot proceed to the deletion engine unless it mathematically satisfies the conformance rules. This bridges the gap between descriptive process mining and deterministic systems safety.