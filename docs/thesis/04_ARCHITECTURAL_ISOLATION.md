# Chapter 4: Architectural Isolation

## 4.1 CLI, Domain, and Integration Boundaries

For the execution-trust pipeline to function, the codebase itself must be structurally isolated. If the logic that decides *what* to delete is intertwined with the OS calls that *perform* the deletion, the system is impossible to verify.

`mac-artifact-cleaner` enforces a tripartite architecture:
1.  **CLI (Noun) Layer:** Validates user intent, orchestrates workflows, and delegates commands. It holds no policy.
2.  **Domain Layer:** The pure, side-effect-free Rust core. It contains the rules for artifact classification, plan building, and OCEL structure. Every public function must be secured by executable documentation (Doctests / Gall Locks).
3.  **Integration Layer:** The boundary that touches the real world. It handles `std::fs` traversal, `tmutil` invocations, and UI progress reporting.

## 4.2 The Necessity of Inert DTOs

Early iterations of the system suffered from "architectural drift" when the Domain layer was allowed to accept `std::fs::DirEntry` or `std::fs::Metadata` objects directly. While these objects are technically read-only, they tether the pure Domain to the live filesystem's state and behavior.

To achieve true purity, the Integration layer must construct "inert snapshots" (Data Transfer Objects) during its traversal.

```rust
pub struct EntrySnapshot {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: EntryKind,
    pub len: Option<u64>,
    pub modified_unix: Option<i64>,
}
```

The Domain layer receives only these inert snapshots. This isolation guarantees that domain decisions (like `detect_project` or `is_artifact_candidate`) are deterministic, fully unit-testable without a mocked filesystem, and absolutely incapable of mutating the host machine.

## 4.3 The "Doctor" as an Architectural Gatekeeper

As a system evolves (per Gall's Law), the entropy of technical debt inevitably increases. To prevent the degradation of the isolated layers, the system must be capable of self-verification.

This is the role of the **G9 Checkpoint: The Doctor**.
The Doctor is not a user-facing disk cleaner; it is a repository governance tool built into the CLI.
- `doctor architecture` scans the codebase to ensure no `std::fs` calls have leaked into `src/domain/`.
- `doctor privacy` scans `docs/` and `tests/` to ensure no real local developer paths (e.g., `/Users/sac/`) have bypassed the redaction gate.

By formalizing the Doctor, the system enforces its own operating laws automatically, ensuring that the complex system maintains the structural integrity of the simple system from which it evolved.
