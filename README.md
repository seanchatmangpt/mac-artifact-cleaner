# Pentecost (`oclnr`)

**A plan-bound macOS developer disk auditor and cleanup utility. It observes first, emits reviewable OCEL evidence, deletes only from approved plans, and records receipts.**

Unlike traditional cleaners that blindly execute `rm -rf` from a live scan, `osx-clnr` enforces a strict, multi-phase execution pipeline: never increase destructive power without simultaneously increasing receipts.

**First praxis project.** osx-clnr is the reference implementation of the [praxis](https://github.com/seanchatmangpt/praxis) house standard: CalVer (`26.7.0`), dual `MIT OR Apache-2.0` license, workspace lints, rustfmt/deny/typos, pinned toolchain, canonical `justfile`, macOS CI, `osxclnr.toml` policy admitted via [star-toml](https://crates.io/crates/star-toml) `TrustedLoader`, and a `cicd.toml` for cargo-cicd.

## The Old Computing Gap

Since the beginning of practical computing, machines have separated syntax from consequence.

A command could be valid.
A process could be permitted.
An exit code could be zero.
A file could be changed.

But the machine still could not publicly prove that the consequence belonged to an admitted order.

Pentecost addresses this gap.

It makes local command execution pass through public naming, separated powers, plan admission, bounded materialization, receipt, replay, and checkpoint promotion.

> **Computing learned to execute before it learned to testify. Pentecost teaches the computer to testify before it acts.**

---

## Architecture & Workflow

Deletion is plan-bound by design:

1. **Audit observes:** The filesystem is scanned with intelligent traversal barriers to avoid crawling massive dependencies (like `node_modules` or `target`) while accurately inventorying hidden tool caches (`.cargo`, `.cache`, `.npm`, etc.).
2. **Plan proposes:** A dry run generates a reviewable JSON plan identifying cleanup candidates based on age, size, and tool-specific heuristics. Add `--include-global-caches` to nominate regenerable global caches (`.cargo/registry`, `Library/Caches`, etc.). Safety filters: any candidate containing git-tracked files is dropped from the plan, and installed extensions/plugins/tool caches are fenced off as package stores (never nominated).
3. **Human reviews:** The user inspects the plan or the emitted Object-Centric Event Log (OCEL v2) to verify what will be deleted.
4. **Delete executes only from a saved plan:** The scanner is disabled during deletion. The utility reads the reviewed plan and strictly deletes only the exact paths listed.
5. **Receipt records the result:** Progress and consequences are tracked without fresh discovery. Receipt verification (`oclnr receipt verify`) checks that measured volume delta is within tolerance of claimed reclaim — surfacing APFS snapshot pinning if space didn't come back.

### Snapshot Management

APFS local snapshots can pin deleted blocks, preventing freed space from appearing in `df`. Use:

```bash
oclnr snapshot audit          # list all local snapshots
oclnr snapshot thin --bytes 20GB
oclnr snapshot delete --which oldest
oclnr snapshot delete --which all
```

### Emergency Reclaim

When disk is critically full (ENOSPC):

```bash
oclnr emergency        # dry run: show what would be reclaimed
oclnr emergency --yes  # execute: delete all local snapshots + sweep regenerable caches
```

### Measurement & Guest Reclaim (non-destructive)

```bash
oclnr dedupe scan            # read-only: bytes reclaimable by replacing duplicate regular
                             # files with APFS clones (clonefile); measures, never rewrites
oclnr docker scan            # Colima VM disk usage + Docker.raw host-side footprint
oclnr docker trim --confirm  # run fstrim inside the Colima VM so freed guest blocks return
                             # to the host — data-preserving; refuses without --confirm
```

### Read-only Fleet Standing

```bash
oclnr tools git-worktrees    # read-only standing report over git worktrees (git-health)
```

### Plan Approval (CLI)

A plan can be HMAC-signed for deletion without going through the MCP server:

```bash
oclnr plan approve --plan cleanup-plan.json --yes
# a plan containing any Unknown/Irreversible-reversibility item also requires:
oclnr plan approve --plan cleanup-plan.json --yes --acknowledge-unknown-reversibility
```

### Unattended Autoclean

`oclnr autoclean run` chains plan build -> plan approve -> delete execute -> receipt
verify as subprocesses of the running binary, for scheduled/unattended use:

```bash
oclnr autoclean run --yes
```

Safety posture: a hard `--max-reclaim-gb` cap (default 50) refuses — logs, does not
delete — a plan claiming more; any plan containing an Unknown/Irreversible-reversibility
item is skipped rather than overridden; `--ignore-recent-hours` defaults to 24h; it never
touches Docker/Colima or wholesale `~/Library/Caches`. Every run's plan/receipt files and
a one-line summary are logged to `~/Library/Logs/oclnr/autoclean.log`.

To run this daily and unattended via `launchd`:

```bash
oclnr daemon install-autoclean    # installs com.oclnr.autoclean, daily at 04:15 local
oclnr daemon install-pressure-monitor --threshold-gb 20
                                   # installs com.oclnr.pressure (KeepAlive): oclnr monitor --watch
                                   # --reclaim snapshots[,builds] — reclaims when free space drops
                                   # below --threshold-gb (required, no default; pass it explicitly),
                                   # polling --interval-secs (default 60 on install-pressure-monitor;
                                   # 300 on a standalone `monitor --watch`). `--reclaim` defaults to
                                   # snapshots and excludes --trigger-autoclean
oclnr daemon status               # reports com.oclnr.monitor (alert-only), com.oclnr.autoclean,
                                   # and com.oclnr.pressure (reclaiming) when installed
oclnr daemon uninstall-autoclean
```

Besides `--threshold-gb` and `--interval-secs` above, `oclnr monitor` accepts:

- `--margin-gb` (default 5) — hysteresis headroom above `--threshold-gb` to reclaim toward
- `--urgency` (1-4, default 4) — `tmutil thinlocalsnapshots` urgency for pressure thins
- `--snapshot-cooldown-secs` (default 600) / `--builds-cooldown-secs` (default 3600) — minimum seconds between pressure-triggered snapshot thins / build reclaims; per-strategy cooldown stamps live under `~/.oclnr/`
- `--builds-max-reclaim-gb` (default 50) — per-run cap for the builds reclaim
- `--builds-ignore-recent-hours` (default 2) — skip build dirs modified within this many hours
- `--receipt-dir` (default `~/Library/Logs/oclnr/pressure`) — directory for pressure-reclaim receipts

`daemon install*` refuses to install a plist the current binary cannot run (no
crash-looping agents), and reinstalling replaces the running agent and verifies
its program before reporting success.

## Privacy and Safety

This tool is safe to publish as source code, but its generated reports are machine-local evidence files.

**Do not commit real output files such as:**

- `disk-audit.json`
- `disk-audit.jsonocel`
- `cleanup-plan.json`
- `cleanup-plan.jsonocel`
- `deletion-receipt.jsonocel`
- `*.r.json` (the `R = receipt(A)` projection written beside every native receipt)

These files can contain absolute paths, project names, hidden tool directories, timestamps, file sizes, and local development patterns. The included `.gitignore` will protect against accidental commits of these file patterns.

## Documentation
- [Gall Checkpoints: The Evolution of Pentecost](docs/GALL_CHECKPOINTS.md)
- [Privacy Model and Redaction Guidelines](docs/PRIVACY_MODEL.md)
- [Time Machine & APFS Snapshot Model](docs/TIME_MACHINE_MODEL.md)
- [OCEL v2 Reporting Model](docs/OCEL_MODEL.md)
