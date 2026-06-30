# Chapter 20: Appendix Verification Index

This appendix maps the mathematical formulations, equations, and rules discussed throughout this dissertation to their concrete implementations within the `osx-clnr` and `cfab-surface` source code.

## 20.1 The Universal Chatman Equation ($\mathcal{A}_{\mathcal{U}} = \mu_{\mathcal{U}}(\mathcal{O}^*)$)
The transformation $\mu$ mapping raw filesystem observations to admitted evidence and plans is implemented via the typestate admission boundary:
* **Implementation:** `DeletionPlanAdjudicator` struct and the `Admit` trait implementation.
* **Source File:** `src/domain/delete.rs`
* **Line Range:** Lines 112 to 184

## 20.2 The Gall Pipeline LTL Constraints ($\Phi_1, \Phi_2, \Phi_3$)
The safety checks verifying plan-bound constraints and preventing raw deletion bypass are implemented in the deletion validation:
* **Implementation:** `validate_plan_item` safety witness verification.
* **Source File:** `src/domain/delete.rs`
* **Line Range:** Lines 10 to 45

## 20.3 Cryptographic Commitments (BLAKE3 Receipt Chain)
The collision-resistant receipt hash commitment is computed when creating a new execution receipt:
* **Implementation:** `blake3::hash` calculation over the serialized execution record.
* **Source File:** `src/domain/receipt.rs`
* **Line Range:** Lines 183 to 185

## 20.4 The Volumetric Reclaim Delta Law ($\Delta = B_a - B_b \ge 0.5 \times C$)
The reality law comparing claimed storage space freed against physical available volume delta measurements is defined here:
* **Implementation:** `check_reclaim` function and the `ReclaimCheck` enum.
* **Source File:** `src/domain/receipt.rs`
* **Line Range:** Lines 141 to 161

## 20.5 Fabric Edge Directional Rules
The rules governing allowed and forbidden directed edge relations in the `Fabric` category are enforced here:
* **Implementation:** `validate_edge_rule` validator.
* **Source File:** `cfab-surface/src/lib.rs`
* **Line Range:** Lines 558 to 578
* **Integration mapping:** `build_fabric` in `src/domain/fabric.rs` at lines 70 to 148
