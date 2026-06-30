# Chapter 13: Autonomic RL Ecosystems

## 13.1 Markov Decision Process (MDP) Formulation
We model autonomic disk cleanup as a Markov Decision Process (MDP) tuple:
$$\langle S, \mathcal{A}, P, R, \gamma \rangle$$
where:
* **State Space $S$:** Multi-dimensional state representing available disk bytes, active artifact markings in the OCPN, and temporal staleness vectors.
* **Action Space $\mathcal{A}$:** Decisions to delete or retain candidates: $\{ \text{delete}(x), \text{retain}(x) \}$.
* **Transition Probabilities $P(s' | s, a)$:** The probability of moving to state $s'$ after action $a$. We decay probabilities dynamically to account for concept drift (e.g., developer switching packages).
* **Reward Function $R(s, a)$:** Formulated to optimize space and penalize re-acquisition (thrashing):
  $$R(s, a) = \Delta \text{bytes_freed} - \lambda \times \mathbb{I}[\text{Thrashing}(x)]$$
* **Discount Factor $\gamma$:** Values immediate vs. long-term storage efficiency.

By solving this MDP recursively using the Bellman Equation, the reinforcement learning agent discovers the optimal cleanup policy.
