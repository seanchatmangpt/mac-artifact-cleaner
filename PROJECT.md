# Project: mac-artifact-cleaner

## Architecture
`mac-artifact-cleaner` is a plan-bound macOS disk auditor and cleanup utility. It runs in three distinct layers to isolate domain policy from CLI and I/O integration:
1. **Domain Layer (`src/domain/`)**: Pure Rust domain logic that is side-effect-free. Contains core definitions of artifacts, audits, plans, deletion receipts, OCEL v2 format, privacy redaction, and Time Machine policies. Every public function has doctests.
2. **Noun/CLI Layer (`src/nouns/`)**: Handles parsing, validating, and formatting. Uses `clap-noun-verb` structure where each CLI command maps to a noun-verb pair (e.g. `plan build`, `delete execute`). Delegates all policy decisions immediately to the Domain Layer.
3. **Integration Layer (`src/integration/`)**: Handles actual interactions with the external environment, including filesystem walking, invoking `tmutil`, communicating with container runtimes (docker), and progress-bar rendering.

## Code Layout
```text
src/
  main.rs                 - Thin CLI entrypoint using clap-noun-verb structure
  lib.rs                  - Library registering domain, nouns, and integration
  domain/
    mod.rs                - Registers domain modules
    artifact.rs           - G0/G1/G2: artifact rules, project detection, traversal barriers
    plan.rs               - G3: Plan build, inspection, and verification (JSON format)
    delete.rs             - Plan-bound deletion rules and validation
    receipt.rs            - Deletion receipts (JSON format)
    audit.rs              - G4: Disk inventory and statistics estimation
    time.rs               - G5: Time Machine exclusions & APFS snapshot checks
    tool_roots.rs         - G6: Root tool categorization & aging recommendation
    ocel.rs               - G7: OCEL v2 event & object reporting
    redaction.rs          - G8: Privacy/Redaction of local paths
  nouns/
    mod.rs                - Registers nouns
    audit.rs              - CLI handlers for audit
    artifact.rs           - CLI handlers for artifact
    tool_roots.rs         - CLI handlers for tool-roots
    plan.rs               - CLI handlers for plan
    delete.rs             - CLI handlers for delete
    receipt.rs            - CLI handlers for receipt
    snapshot.rs           - CLI handlers for snapshot
    exclusion.rs          - CLI handlers for exclusion
    ocel.rs               - CLI handlers for ocel
    privacy.rs            - CLI handlers for privacy
    doctor.rs             - CLI handlers for doctor diagnostics
  integration/
    mod.rs                - Registers integrations
    fs.rs                 - Filesystem traversal wrapper (WalkBuilder, estimate_size)
    tmutil.rs             - Time machine CLI wrapper (`tmutil`)
    docker.rs             - Docker runtime state analyzer
    progress.rs           - Indicatif progress spinner / progress bar
```

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|---|---|---|---|
| 1 | Restructuring & Safe Verification Foundation | Set up modules, implement G0-G3 (Artifacts, Project Detection, Barriers, and Safe Plan-Bound Deletion). Document & doctest everything in domain. | None | DONE |
| 2 | Inventory, Caches & Snapshots | G4-G6 (Disk inventory, Time Machine exclusions, APFS snapshot thin, Tool Roots aging analysis). | M1 | IN PROGRESS |
| 3 | Reporting, Privacy & Noun-Verb CLI | G7-G9 (OCEL v2 logs, privacy redaction gate, full clap-noun-verb CLI implementation). | M2 | IN PROGRESS |

## Detailed Roadmap
For the granular path to G9 completion, see [Gall Checkpoint Roadmap](docs/GALL_ROADMAP.md).


## Interface Contracts
### `domain::plan` ↔ `domain::delete`
- `domain::plan::DeletionPlan` represents the serialized authority.
- `domain::delete::validate_plan_item` verifies that an item proposed for deletion is present in the approved plan.
- Deletion execution cannot proceed unless a validated plan is provided and loaded.
