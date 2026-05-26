# mac-artifact-cleaner: Audit & Plan Sequence Diagrams

This document details the flow of the **Audit** and **Plan** phases, highlighting the interaction between the CLI Noun layer, the Integration layer's parallel traversal, and the pure Domain classification.

## 1. Audit Phase Sequence

The Audit phase performs a high-speed parallel scan of the filesystem to identify developer artifacts and tool-root caches. It produces a detailed report and an optional OCEL v2 event log.

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant CLI as Nouns::Audit
    participant FS as Integration::FS
    participant Domain as Domain::Artifact
    participant OCEL as Domain::OCEL
    participant Disk as Filesystem (OS)

    User->>CLI: audit run --ocel-output disk-audit.jsonocel
    CLI->>FS: scan_root(roots, args, candidates, stats)
    
    Note over FS: Initialize WalkBuilder (Parallel)
    
    par Worker Threads
        FS->>Disk: parallel walk (ignore crate)
        Disk-->>FS: entry (Dir/File)
        
        alt is Directory
            FS->>Disk: read_dir_snapshot(path)
            Disk-->>FS: std::fs::read_dir results
            FS->>FS: Construct inert DirSnapshot (DTO)
            
            FS->>Domain: detect_project_from_snapshot(snap)
            Note right of Domain: Pure classification (no OS calls)
            Domain-->>FS: Option<ProjectKind>
            
            opt Project detected
                FS->>Domain: artifact_candidates_from_snapshot(path, project, snap)
                Domain-->>FS: Vec<Candidate>
                FS->>FS: Insert into shared candidates set
                FS->>FS: Update shared Stats
            end
        else is File
            FS->>FS: Update stats (bytes_seen, files_seen)
            opt Tool Roots Enabled
                FS->>FS: record_tool_root_file(path, meta)
            end
        end
    end

    FS-->>CLI: Scan Complete
    
    CLI->>OCEL: build_disk_audit_ocel(roots, candidates, reports, stats)
    OCEL-->>CLI: OcelLog (DTO)
    
    CLI->>Disk: write_file("disk-audit.jsonocel", JSON)
    CLI-->>User: Display Audit Summary Dashboard
```

## 2. Plan Phase Sequence

The Plan phase uses the same scanning engine as Audit but focuses on preparing a `DeletionPlan`. This plan serves as a safety barrier for subsequent deletion actions.

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant CLI as Nouns::Plan
    participant FS as Integration::FS
    participant Domain as Domain::Artifact
    participant Plan as Domain::Plan
    participant Disk as Filesystem (OS)

    User->>CLI: plan build --output cleanup-plan.jsonocel
    CLI->>FS: scan_root(roots, args, candidates, stats)
    
    Note over FS: Parallel Traversal & Pure Classification
    Note over FS: (Same logic as Audit Phase)
    
    FS-->>CLI: Scan Complete
    
    CLI->>CLI: Map Candidates to PlanItems
    
    CLI->>Plan: DeletionPlan::new(roots, items, tool_roots)
    Plan-->>CLI: DeletionPlan (DTO)
    
    CLI->>Disk: write_file("cleanup-plan.jsonocel", JSON)
    CLI-->>User: ✨ Success: Wrote deletion plan
```

## Key Architectural Principles

1.  **Integration/Domain Separation**: The Integration layer (`src/integration/fs.rs`) is the only part of the scanner that touches the OS (`std::fs`). It converts live OS handles into inert Data Transfer Objects (DTOs) like `DirSnapshot`.
2.  **Pure Domain Classification**: The Domain layer (`src/domain/artifact.rs`) performs classification using only the DTOs. This makes the core logic testable without mocking the filesystem.
3.  **Parallel Execution**: The `ignore` crate's parallel walker is used to saturate CPU cores while traversing the macOS filesystem, with thread-safe aggregation via `Arc`, `Mutex`, and `DashMap`.
4.  **Artifact-Centric Observability**: The generation of `disk-audit.jsonocel` allows for external process mining and auditing of what was discovered on the disk.
