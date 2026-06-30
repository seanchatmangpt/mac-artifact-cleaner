# Chapter 5: Filesystem Ontologies

## 5.1 Event Abstraction
Raw POSIX operating system operations are high-frequency, noisy, and lack semantic process context. Event Abstraction is the technique of aggregating and mapping low-level operating system events (such as `open`, `write`, `close`, and `unlink`) into higher-level process events (such as `artifact_proposed` or `artifact_deleted`).

## 5.2 The POSIX-to-OCEL Lemma
We formalize this aggregation via the POSIX-to-OCEL Lemma:
* **Lemma 5.1 (POSIX-to-OCEL Event Abstraction):** Any sequence of raw filesystem operations $\sigma_{POSIX}$ occurring within a bounded directory tree can be mapped deterministically to a sequence of macro-level process events $\sigma_{OCEL}$ such that the causal ordering of object interactions is preserved, and the resulting event stream qualifies as a valid OCEL 2.0 log.

By validating this lemma, we prove that raw disk operations can be modeled and analyzed using standard workflow net tools.
