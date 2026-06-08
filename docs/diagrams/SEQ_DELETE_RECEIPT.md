# Sequence Diagram: Delete and Receipt Phase

This diagram details the strict plan-bound execution of the `osx-clnr` delete phase, ensuring no destructive behavior occurs without a validated plan and that all consequences are recorded in an OCEL-compliant receipt.

```mermaid
sequenceDiagram
    autonumber
    participant U as User (CLI)
    participant C as CLI Entrypoint (nouns::mod)
    participant N as Delete Noun (nouns::delete)
    participant P as Deletion Plan (domain::plan)
    participant V as Validator (domain::delete)
    participant I as Integration Layer (integration::fs)
    participant R as Receipt Domain (domain::receipt)
    participant O as OCEL Domain (domain::ocel)
    participant D as Disk (Filesystem)

    U->>C: osx-clnr delete execute --plan <path> --receipt <path>
    C->>N: handle(DeleteAction::Execute)
    
    Note over N, D: Phase 1: Strict Plan Loading & Validation
    N->>D: Read plan file from disk
    D-->>N: plan_content (JSON/OCEL)
    N->>P: Deserialize into DeletionPlan struct
    N->>V: validate_plan(plan)
    activate V
    V->>V: Verify version compatibility (v1)
    V->>V: Enforcement: Check for system path violations
    V-->>N: Result (Ok/Err)
    deactivate V

    Note over N: [SAFETY] Scanner is disabled.<br/>No fresh discovery or improvisational destruction.

    Note over N, I: Phase 2: Plan-Bound Execution via Integration Layer
    loop For each PlanItem in plan.items
        N->>D: Check if path exists
        D-->>N: Boolean
        
        alt Path Missing
            N->>N: Status: SkippedMissing
        else Path Exists
            alt Kind == File
                N->>I: delete_file(path)
                I->>D: std::fs::remove_file(path)
                D-->>I: Result (Success/Failure)
                I-->>N: Result
            else Kind == Dir
                N->>I: delete_dir_all(path)
                I->>D: std::fs::remove_dir_all(path)
                D-->>I: Result (Success/Failure)
                I-->>N: Result
            end
            N->>N: Status: Deleted OR Failed (with error capture)
        end
    end

    Note over N, O: Phase 3: Receipt Emission (OCEL v2)
    N->>R: DeletionReceipt::new(results, timestamps)
    R-->>N: Receipt Instance
    
    N->>O: build_deletion_receipt_ocel(receipt)
    Note over O: Transform receipt results into<br/>Object-Centric Event Log
    O-->>N: OcelLog (OCEL v2 JSON)
    
    N->>D: Write deletion-receipt.jsonocel
    Note right of D: Immutable record of consequences

    N-->>U: Display Summary (Total, Deleted, Skipped, Failed)
    Note over U: Execution complete with cryptographic evidence
```

## Key Constraints

1.  **Scanner Disabled**: The discovery logic (`integration::fs::scan_root`) is never invoked during the delete phase. Only the paths explicitly listed in the authorized plan are considered.
2.  **Validation Gate**: The `validate_plan` function acts as a safety barrier, preventing any system-critical paths from being included in the execution loop, even if they were accidentally included in the plan.
3.  **Integration Layer Decoupling**: Deletions are performed via `integration::fs` functions which wrap `std::fs`, ensuring a single point of entry for OS-level mutations.
4.  **OCEL Traceability**: Every deletion (or failure) is recorded as an event in the `deletion-receipt.jsonocel`, relating back to the plan and the original artifact candidate.
