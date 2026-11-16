# Changelog

## 26.9.22 (unreleased — `Cargo.toml` still 26.9.21; cut the version bump + `v26.9.22` tag to make this a release)

### Security
- Supply-chain floor (44ffe7d): `crossbeam-epoch` bumped 0.9.18 → 0.9.21 in `Cargo.lock` (RUSTSEC-2026-0204); `cargo deny check` (advisories v2, vulnerabilities always denied) is clean, and the new lockfile admission guard (`tests/lockfile_supply_chain.rs`) refuses any lock regressing below the observed advisory floor (crossbeam-epoch ≥ 0.9.20).

### Changed
- `oclnr` resolution order extension (44ffe7d): `oclnr-mcp` also probes a cargo-built `oclnr` in `target/<profile>/` (the parent of a `deps/` test-harness directory) before falling back to `PATH`, so MCP server tests no longer require `oclnr` on PATH and a stale PATH install cannot shadow the built binary.

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
