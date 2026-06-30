# Chapter 8: Gall Pipeline

## 8.1 The Gall Pipeline Sequence
The Gall Pipeline enforces a non-negotiable sequence of phases for resource lifecycle transitions:
$$\text{Observe} \rightarrow \text{Plan} \rightarrow \text{Exclusion} \rightarrow \text{Deletion} \rightarrow \text{Receipt} \rightarrow \text{Verification}$$

No raw, direct deletion can bypass this pipeline.

## 8.2 Linear Temporal Logic (LTL) Constraints
We formalize these sequential constraints using Linear Temporal Logic (LTL) formulas over the filesystem event stream:
1. **Precedence Constraint ($\Phi_1$):** Deletion must be preceded by planning.
   $$\square ( \text{artifact_deleted} \rightarrow \lozenge_{\leq 0} \text{deletion_plan_created} )$$
2. **Response Constraint ($\Phi_2$):** Planning must eventually lead to Time Machine exclusion before execution.
   $$\square ( \text{deletion_plan_created} \rightarrow \lozenge \text{tm_exclusion} )$$
3. **Chain Succession Constraint ($\Phi_3$):** Deletion must immediately follow exclusion verification.
   $$\square ( \text{tm_exclusion} \rightarrow \bigcirc \text{artifact_deleted} )$$

We prove that this constraint set is complete and sufficient to guarantee safety against accidental data loss.
