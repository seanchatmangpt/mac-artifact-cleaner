# Time Machine & APFS Snapshot Model

This document explains how `pentecost` interacts with macOS APFS local snapshots and Time Machine exclusions.

---

## 1. The APFS Reclaimed Space Paradox

A common developer frustration on macOS:
1.  A cleanup tool runs and reports: `Successfully deleted 120 GB of build targets.`
2.  The developer runs `df -h` or opens Disk Utility.
3.  **Available storage has not increased by a single byte.**

### Why does this happen?
Under Apple File System (APFS), when files are deleted, their blocks are not actually marked as free if they are pinned by a local APFS backup snapshot. Time Machine automatically creates local snapshots (typically every hour) and keeps them until the OS requires space or the snapshot expires.

```text
Active Directory  ────────> [ File Blocks (120 GB Target cache) ]
                                 ▲
APFS Local Snapshot ─────────────┘ (Blocks are retained!)
```

To solve this, `pentecost` models APFS snapshots and Time Machine backup exclusions as core domain concepts.

---

## 2. Time Machine Exclusion Management

The cleanest way to prevent build artifacts from bloating Time Machine backups (and pinning deleted files) is to mark directories as **excluded** prior to deletion.

### 2.1 The `tmutil` Exclusion Plan
We do not make direct, unchecked modifications to Time Machine configurations. Instead, the application generates a reviewable exclusion plan script.

The logic in [tmutil.rs](../src/integration/tmutil.rs) implements this behavior:

```rust
pub fn write_tm_exclusions_script(
    script_path: &Path,
    candidates: &[Candidate],
) -> std::io::Result<()> {
    // Generates a script that invokes `tmutil addexclusion` on each candidate path
}
```

The script can be reviewed by the system administrator and run via:
```bash
snapshot exclusion plan --from cleanup-plan.jsonocel --output tm-exclusions.sh
snapshot exclusion apply --from tm-exclusions.sh
```

> [!NOTE]
> `tmutil addexclusion` applies a sticky metadata attribute to the folder. Even if the folder is deleted and recreated, macOS honors the exclusion, keeping target, build, and node_modules folders out of both local snapshots and external backups.

---

## 3. APFS Snapshot Lifecycle & Thinning

If files are deleted and space is still pinned, local snapshots must be thinned.

```text
1. Exclude rebuildable junk from backups
  └── 2. Delete rebuildable junk from reviewed plan
        └── 3. Observe APFS local snapshot capacity
              └── 4. Thin snapshots explicitly (only when requested)
                    └── 5. Verify freed capacity and write receipt
```

### 3.1 Rules of Snapshot Thinning

1.  **No Automatic Thinning**: The tool must never thin APFS snapshots without an explicit command and user verification.
2.  **Explicit Target Sizes**: The thinning command must specify target capacities or age bounds (e.g. `--bytes 100GB`).
3.  **System Utility Delegation**: Thinning is delegated to macOS native binaries (`tmutil thinlocalsnapshots`) using safe arguments.

### 3.2 Snapshot Commands

```bash
# Analyze APFS local snapshot usage and space pinned
oclnr snapshot audit

# Thin local snapshots to try and reclaim up to 200 GB
oclnr snapshot thin --bytes 200GB

# Delete the single oldest snapshot (precise, count-driven)
oclnr snapshot delete --which oldest

# Delete the three oldest snapshots
oclnr snapshot delete --which oldest --oldest-n 3

# Delete all local snapshots
oclnr snapshot delete --which all

# Delete a specific snapshot by date suffix
oclnr snapshot delete --which 2026-05-26-135630
```

`snapshot delete` emits `snapshot_delete_requested` OCEL events (distinct from `snapshot_thin_requested`) and prints before/after free-space deltas.

### 3.3 Emergency Reclaim

When disk is critically full and standard tools can't write output files:

```bash
# Dry run: show what would be reclaimed without deleting
oclnr emergency

# Execute: delete all local snapshots + sweep regenerable global caches
oclnr emergency --yes
```

`oclnr emergency` is the escalation path for ENOSPC conditions. It combines snapshot deletion and global cache nomination in a single command. The `check_reclaim` law (in `domain::receipt`) then validates that the measured volume delta is within 50% of the claimed reclaim — signaling when APFS snapshot pinning is still blocking freed space.

### 3.4 The APFS Snapshot Caveat in Receipt Verification

`domain::receipt::check_reclaim` compares `bytes_freed_total` (sum of plan-declared sizes) against the actual `available_after - available_before` volume delta. When this delta falls below 50% of the claimed value, `BytesFreedMismatch` is raised in the receipt verification report.

This is **correct signal**, not a false positive: it means blocks were deleted but are still pinned by snapshots. The right response is to run `oclnr snapshot thin` or `oclnr emergency`.

Every snapshot operation is logged in the final delete receipt, showing the before-and-after free space comparison to verify successful block reclamation.
