# Original User Request

## Initial Request — 2026-05-26T13:56:32-07:00

Restructure and fully implement the Gall Checkpoints (G0 to G9) for the `mac-artifact-cleaner` Rust project using module documentation and unit/doctests.

Working directory: /Users/sac/mac-artifact-cleaner
Integrity mode: development

## Requirements

### R1. Modular Restructuring
The project must be structured into modular domain, noun, and integration layers following the shape specified in `AGENTS.md`.

### R2. Doctest and Moduledoc Discipline
Every public function under the domain layers must have module-level documentation and at least one passing doctest (covering positive and negative/refusal cases).

### R3. Safe Plan-Bound Deletion Verification
Every checkpoint transition must be verified using unit tests, doctests, and/or integration tests to prove that deletion can only occur from a validated dry-run plan.

## Acceptance Criteria

### Restructuring
- [ ] Code compiles cleanly without warnings or errors.
- [ ] Project is partitioned into library and binary targets.

### Verification
- [ ] `cargo test` and `cargo test --doc` execute and pass.
