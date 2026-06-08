# Domain & OCEL v2 Class Diagram

This diagram illustrates the core entities of the `osx-clnr`, focusing on the **Inert DTOs** that isolate the Domain layer from side-effects and the **OCEL v2** objects that represent the system's audit and execution state.

```mermaid
classDiagram
    direction LR

    %% --- Inert DTOs (The "Evidence" Layer) ---
    class EntrySnapshot {
        +PathBuf path
        +String file_name
        +Option~String~ extension
        +EntryKind kind
        +is_dir() bool
        +is_file() bool
    }

    class DirSnapshot {
        +Vec~EntrySnapshot~ children
        +has_file(name) bool
        +has_dir(name) bool
    }

    class EntryKind {
        <<enumeration>>
        File
        Dir
    }

    DirSnapshot "1" *-- "many" EntrySnapshot
    EntrySnapshot --> EntryKind

    %% --- OCEL v2 Core Objects (Domain Layer) ---
    
    class DiskAudit {
        <<OCEL Object: disk_audit>>
        +u64 files_seen
        +u64 dirs_seen
        +u64 bytes_seen
        +u64 projects_seen
        +u64 candidates_seen
        +DateTime created_at
    }

    class ArtifactCandidate {
        <<OCEL Object: artifact_candidate>>
        +PathBuf path
        +String reason
    }

    class DeletionPlan {
        <<OCEL Object: deletion_plan>>
        +u32 version
        +u64 created_unix
        +Vec~PathBuf~ roots
        +Vec~PlanItem~ items
    }

    class DeleteReceipt {
        <<OCEL Object: delete_receipt>>
        +u32 version
        +u64 execution_started_unix
        +u64 execution_completed_unix
        +Vec~DeletionResult~ results
    }

    class SnapshotState {
        <<OCEL Object: snapshot_state>>
        +String volume
        +u64 requested_bytes
        +Vec~String~ snapshots
    }

    %% --- Support Structs ---

    class PlanItem {
        +PathBuf path
        +PlanItemKind kind
        +String reason
    }

    class DeletionResult {
        +PathBuf path
        +DeletionStatus status
        +Option~String~ error
    }

    DeletionPlan "1" *-- "many" PlanItem
    DeleteReceipt "1" *-- "many" DeletionResult

    %% --- Relationships & Flow ---

    DirSnapshot ..> ArtifactCandidate : "Domain Classification (Pure)"
    ArtifactCandidate "many" --o "1" DeletionPlan : "Proposed for"
    DeletionPlan <.. DeleteReceipt : "Verification"
    DiskAudit "1" -- "many" ArtifactCandidate : "Identifies"

    %% --- OCEL v2 Infrastructure ---

    class OcelLog {
        +Vec~OcelEvent~ events
        +Vec~OcelObject~ objects
    }

    class OcelObject {
        +String id
        +String type
        +Vec~OcelTimedAttributeValue~ attributes
        +Vec~OcelRelationship~ relationships
    }

    class OcelEvent {
        +String id
        +String type
        +DateTime time
        +Vec~OcelRelationship~ relationships
    }

    OcelLog "1" *-- "many" OcelObject
    OcelLog "1" *-- "many" OcelEvent
    
    note for EntrySnapshot "DTOs allow Domain logic to be\npure and side-effect free."
    note for DiskAudit "Maps to 'Stats' struct\nin Rust implementation."
    note for SnapshotState "Captured during snapshot\naudit or thinning."
```

## Implementation Mapping

| OCEL v2 Object | Rust Domain Struct | Location |
| :--- | :--- | :--- |
| `disk_audit` | `Stats` | `src/domain/audit.rs` |
| `artifact_candidate` | `Candidate` | `src/domain/artifact.rs` |
| `deletion_plan` | `DeletionPlan` | `src/domain/plan.rs` |
| `delete_receipt` | `DeletionReceipt` | `src/domain/receipt.rs` |
| `snapshot_state` | `SnapshotThinReceipt` | `src/domain/time.rs` |
| `N/A (DTO)` | `EntrySnapshot` | `src/domain/artifact.rs` |
| `N/A (DTO)` | `DirSnapshot` | `src/domain/artifact.rs` |

## Purity Boundary

The **Integration Layer** (in `src/integration/*.rs`) is responsible for all OS calls (e.g., `std::fs`, `tmutil`). It constructs inert **DTOs** (`EntrySnapshot`, `DirSnapshot`) and passes them into the **Domain Layer**. 

The Domain Layer functions are deterministic and side-effect free, returning classified **Candidates** or **Plans**. These are eventually exported as **OCEL v2** objects, providing a cryptographically verifiable audit trail of all "observations" and "decisions" made by the tool.
