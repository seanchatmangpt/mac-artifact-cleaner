# Chapter 9: Affine Typestate

## 9.1 Affine Types and Ownership
Rust's affine type system guarantees that a value can be consumed at most once. Ownership and move semantics allow us to enforce protocol transitions at compile time. By representing states as distinct types, we prevent compile-time bugs like using an uninitialized or unvalidated resource.

## 9.2 Raw vs. Admitted Plans
We model `DeletionPlan` using typestates:
* `DeletionPlan<Raw>`: A plan that has been created but not yet evaluated against safety heuristics.
* `DeletionPlan<Admitted>`: A plan that has successfully passed all safety checks in the adjudicator.

The signature of the deletion execution engine is:
```rust
fn execute_deletion(plan: DeletionPlan<Admitted>) -> DeletionReceipt
```
Because the engine only accepts `DeletionPlan<Admitted>`, it is compilationally impossible to execute a raw, unchecked plan. The transition from `Raw` to `Admitted` requires a cryptographic witness and adjudicator logic, bridging the Curry-Howard isomorphism directly to filesystem safety.
