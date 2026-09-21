# Changelog

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
