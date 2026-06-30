# Chapter 3: Petri Nets

## 3.1 Petri Net Theory
A Petri net is a bipartite graph consisting of places $P$ and transitions $T$, connected by directed arcs $F$. Tokens reside in places and represent local resource states. A transition is enabled when all its input places contain a sufficient number of tokens. Firing a transition consumes tokens from its input places and produces tokens in its output places.

## 3.2 Liveness and Soundness in Workflow Nets
Workflow nets (WF-nets) are a subclass of Petri nets designed to model process lifecycles. A WF-net has a single source place $i$ and a single sink place $o$. Soundness is a fundamental correctness criterion for WF-nets, requiring that:
1. **Option to complete:** For any marking reachable from $i$, there exists a firing sequence to the marking $o$.
2. **Proper completion:** The terminal state is the only marked place when reached (no dangling tokens remain in the net).
3. **No dead transitions:** It is possible to fire any transition in the net.

Liveness ensures that the net never enters a deadlock state where no transitions can fire. In this chapter, we mathematically prove that our Object-Centric Petri Net representation of the developer filesystem is structurally sound and live.
