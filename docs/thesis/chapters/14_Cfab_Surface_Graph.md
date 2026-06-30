# Chapter 14: Cfab Surface Graph

## 14.1 Graph Engine Architecture
The Category $\mathbf{Fab}$ is structurally implemented in the `cfab-surface` crate as a directed graph using the `petgraph` library. Surfaces represent nodes, and Relations represent edges.

## 14.2 Runtime Cycle Detection and Invariants
To maintain structural invariants:
1. **Acyclicity:** Cycles are strictly forbidden, as they would represent circular dependencies or recursive deletion traps. When an edge is added, `petgraph::algo::is_cyclic_directed` checks the graph. If a cycle is detected, the operation is rolled back and returns `FabricError::CycleDetected`.
2. **Directional Law:** We validate that all added edges obey directional rules:
   * A Receipt cannot point back to a Plan.
   * A Receipt cannot point directly to a LocalDirectory or GitHubRepository.
   * A Plan cannot point back to a GitHubRepository.

## 14.3 Pathfinding and Validation
We use the A* pathfinding algorithm (`petgraph::algo::astar`) to trace semantic paths between surfaces. This allows us to verify that a Deletion Receipt is indeed reachable from its parent Deletion Plan, proving structural alignment and compliance.
