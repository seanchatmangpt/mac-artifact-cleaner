# Changelog

## 26.9.22 (unreleased — `Cargo.toml` still 26.9.21; cut the version bump + `v26.9.22` tag to make this a release)

### Added
- `dedupe scan` (47291ce): read-only measurement of how many bytes could be reclaimed by replacing duplicate regular files with APFS clones (`clonefile`); measurement-only, never rewrites.
- `docker trim --confirm` (5add5ff): data-preserving guest `fstrim` inside the Colima VM so freed guest blocks return to the host; refuses to run without `--confirm`. `docker scan` also reports Colima VM disk images / Docker.raw host-side footprint.
- `tools git-worktrees` (7c70590): read-only git-worktree standing report (git-health).
- Pressure-triggered reclaim (6ed9727): `daemon install-pressure-monitor` installs a `com.oclnr.pressure` KeepAlive LaunchAgent running `monitor --watch --reclaim snapshots[,builds]`; new `monitor` flags `--threshold-gb` / `--interval-secs` / `--reclaim` (`--reclaim` and `--trigger-autoclean` are mutually exclusive).
- Fleet R-projection receipts (acdf150): every native receipt (delete execute, snapshot thin incl. the pressure path) now also emits an `R = receipt(A)` projection at `<stem>.r.json` beside it; `build.rs` stamps `OCLNR_BUILD_SHA` / `OCLNR_BUILD_DIRTY` at compile time.

### Changed
- `daemon install*` hardening (3b3fae4, d5fb783): agents must run an oclnr-owned binary — install refuses a plist the current binary cannot run (no crash-looping cleanup agents); reinstall replaces the running agent, reloads it, and verifies the running program before reporting success.
- Plan safety filters (ec1f8c1, fbda47f, 079cee9): `plan build` drops any candidate containing git-tracked files and guards `uv-cache` as a package store; installed extensions/plugins/tool caches (`.vscode`/`.cursor` extensions, zcode plugin cache, bun, pre-commit, act caches) are fenced off from nomination (package-store classifier revision 3); the scan cache is namespaced by classifier revision so stale nominations cannot leak across revisions.
- MCP receipt surface (0bde148): `receipt(action: "verify")` verifies the receipt on disk (not just the in-memory copy); the approve schema adds `acknowledge_unknown_reversibility`; failed deletes are distinguished from live-build recreation.

## 26.9.21

### Added
- `doctor ocel`: admits every workspace OCEL evidence log plus a generated one through the OCEL v2 adjudicator; recognizes deletion receipts (`execution_record`) and defers them to `receipt verify`. Step 8 of `scripts/sanity.sh`, alias `cargo doctor-ocel`.
- `--redact` on `snapshot audit/thin/delete` OCEL output (G8).
- `--redact` on `audit run` and `plan build` via the shared `write_output_file` writer (G8).
- Run-level OCEL v2 evidence for every autoclean run outcome.
- `doctor daemon` health check and durable launchd job logs.
- Unattended autoclean: budget cap, status, notifications, pressure trigger, daily launchd job, CLI `plan approve`.
- Docker.raw host-side footprint in scan/summary/prune; plain JSON receipt for docker/colima prune.
- DCM reversibility classification on plans and receipts; broader reversibility classifier.
- Global cache nomination in `plan build`; `--all-filesystems` on `audit scan`; `audit breakdown`.
- Fences for Photos libraries and Apple sandbox containers (TCC-protected).

### Changed
- `doctor privacy` honors `.gitignore` and skips `vendor/`; `.gitignore` covers `archive/`, `archive-*/`, `receipts/`.
- Hardlinked files are counted once; freed bytes are measured physically.
- MCP surface consolidated to resource-grouped tools.
- Version bump: `osx-clnr` 26.7.9 → 26.9.21, `cfab-surface` 26.7.0 → 26.9.21.

### Fixed
- OCEL validation defects: scan_root relationship, time-typed `created_at`, honest stage statuses.
- `delete_execute` MCP tool no longer times out mid-deletion at 30s.
- Volume probe no longer hardcodes `/`.
- Snapshot thinning hangs and delete-both bug.
- Clippy `-D warnings` errors; `test_github_discover_candidates` no longer depends on the calendar date.

### Not built
- `doctor diagnose`, `doctor plan-fix`, `doctor apply-fix` (UNSUPPORTED).
