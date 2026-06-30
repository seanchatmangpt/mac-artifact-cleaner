# Chapter 7: Category Fab

## 7.1 Category Fab Definition
We formalize the resource network as a category $\mathbf{Fab}$:
* **Objects:** The objects of $\mathbf{Fab}$ are `Surfaces` ($S \in \mathbf{Fab}_O$), which represent digital resources (local directories, plans, receipts, repositories) uniquely identified by URIs.
* **Morphisms:** The morphisms are `Relations` ($f: S_1 \rightarrow S_2 \in \mathbf{Fab}_M$), which represent dependencies, transformations, or evidence linkages between resources.

## 7.2 Morphism Composition
Morphism composition in $\mathbf{Fab}$ represents transitive dependencies or workflow stages. We define the composition law:
$$f \circ g: S_1 \rightarrow S_3$$
where $g: S_1 \rightarrow S_2$ and $f: S_2 \rightarrow S_3$. The composition is associative and respects identity relations.

## 7.3 Graph Representation
In practice, the category $\mathbf{Fab}$ is instantiated as a directed acyclic graph (DAG) where nodes represent surfaces and edges represent relations. This graph is dynamically built and verified by the `cfab-surface` engine to prevent circular dependencies or illegal state transitions.
