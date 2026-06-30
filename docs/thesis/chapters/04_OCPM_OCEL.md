# Chapter 4: OCPM and OCEL

## 4.1 The Object-Centric Event Log Tuple
We define an Object-Centric Event Log (OCEL 2.0) as a tuple:
$$L = (E, O, T_E, T_O, \pi_{type}, \pi_{rel}, \pi_{time}, \pi_{attr})$$
where:
* $E$ is the set of events.
* $O$ is the set of objects.
* $T_E$ is the set of event types.
* $T_O$ is the set of object types.
* $\pi_{type}: (E \cup O) \rightarrow (T_E \cup T_O)$ maps events and objects to their respective types.
* $\pi_{rel}: E \rightarrow \mathcal{P}(O)$ maps events to the set of related objects.
* $\pi_{time}: E \rightarrow \mathcal{T}$ associates a timestamp with each event.
* $\pi_{attr}: (E \cup O) \times \mathcal{A} \rightarrow \mathcal{V}$ assigns attribute values to events and objects.

## 4.2 Divergence and Convergence
In traditional flat event logs, representing many-to-many relationships (e.g., one deletion plan covering multiple files) requires either replicating events (causing divergence) or flattening the process (causing convergence). By retaining the multi-object mapping natively, OCPM maps filesystem operations precisely without losing causal associations.
