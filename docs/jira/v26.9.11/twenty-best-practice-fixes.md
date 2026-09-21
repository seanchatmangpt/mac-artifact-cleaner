# Apply 20 targeted best-practice fixes across domain/integration/nouns/mcp

## Summary

Multi-agent analysis (3 parallel reviewers: domain, integration, nouns+mcp)
surfaced 25 real, pointable issues against this repo's own house rules
(`no-overclaiming-rust.md`'s unwrap/expect discipline, domain purity, DRY).
20 of these were low-risk and were applied directly, one at a time, each
verified to compile before moving to the next. 5 medium-risk findings
(result-swallowing via `unwrap_or_default` in `affidavit_integration.rs`/
`plan.rs`, a plan-parse-error swallowed via `.ok()` in `mcp/server.rs`, and an
overlong-handler-function pattern across MCP tool dispatch) were deliberately
deferred, not auto-applied, pending a closer follow-up pass.

## Status

Done — already committed to the repo (not a backlog item).

## Commits

- `a659d1c` Apply 20 targeted best-practice fixes across domain/integration/nouns/mcp

## Changes

- `domain/plan.rs`: `DeletionPlan::new()` now reuses the shared
  `system_time_to_unix()` helper instead of reimplementing it
- `domain/affidavit_integration.rs`: one `.expect("header event canonicalizes")`
  replaced with `?` to propagate the affidavit crate's real `Result`
- `domain/tool_roots.rs`: recover from mutex poisoning instead of panicking in
  `build_tool_root_report()`
- `domain/redaction.rs`: stop recomputing `to_lowercase()` every loop
  iteration; converted to `while let` per clippy (`while_let_loop`)
- `integration/toolchain.rs`: pass the path via `as_os_str()` instead of a
  lossy `to_str().unwrap_or("")` that could silently run `du -sk ""`
- `integration/git_health.rs`: `count_objects()` propagates parse errors
  instead of silently reporting 0 (matching its own doc comment's stated
  rationale); `list_worktrees()` now returns `Result` instead of swallowing
  failures into empty vectors
- `integration/backup.rs`: factored `extract_plist_tagged()` helper out of
  `extract_plist_string()`/`extract_plist_date()`; added a shared
  `du_bytes()` helper in `progress.rs` and migrated `backup.rs`'s
  `du_path()` to use it
- `integration/doctor.rs`: collapsed a 5-branch `starts_with` chain into an
  array + `.any()`; added a symlink guard before recursion in both
  `traverse()` closures to prevent a symlink-cycle stack overflow
- `integration/notify.rs`: documented that AppleScript escaping is minimal
  and callers must not pass untrusted text
- `nouns/audit.rs`: CLI `audit run`/`summarize` now require `--root` or
  explicit `--yes` before defaulting to a whole-home-directory scan,
  mirroring the MCP `audit_scan` handler's existing refusal of blank roots;
  also recovered a poisoned-mutex unwrap on the phase-status lock
- `nouns/daemon.rs`: factored `ensure_plist_dir()` helper, replacing two
  `unwrap()`s on `plist.parent()` with a real error path
- `nouns/delete.rs`: `.expect()` on a static, provably-valid template
  instead of a bare `.unwrap()`
- `nouns/dev.rs`: a discarded `remove_file()` error now logs a warning
  instead of vanishing silently
- `mcp/server.rs`: `emergency_reclaim`'s response now always warns when
  `target_free_gb` was supplied but is not yet wired through
- `nouns/mod.rs`: added the missing doc comment on `handle_cli()`

### Deferred (medium risk, not applied — needs a closer follow-up pass)

- `domain/affidavit_integration.rs:313,330` — `content_address()`/
  `serialize_receipt()` launder affidavit-crate errors into empty
  string/Vec via `unwrap_or_default()`
- `domain/plan.rs:196` — `content_hash()` hashes an empty payload on
  serialization failure instead of erroring
- `mcp/server.rs:979` — a plan-parse error is collapsed via `.ok()` into
  `Option::None`, indistinguishable from "no plan file"
- `mcp/server.rs` — several 90-215 line MCP handler functions repeat the
  same deserialize/dispatch/parse-stdout/map-to-struct shape; candidate for
  a shared `run_and_parse<T>` helper

## Verification

Per the commit message, stated as real, this-session verification:

- `cargo build`: exit 0
- `cargo fmt -- --check`: clean (one clippy-driven `while_let_loop` rewrite
  in `redaction.rs` needed a follow-up fmt pass, now clean)
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test`: 89+8+1+3+11 = all pass except `test_github_discover_candidates`,
  confirmed pre-existing by reproducing the identical failure on the
  pre-refactor commit via `git stash` — not introduced by this change
- `cargo test --doc`: 108 passed

## Related

No PR numbers or branch names mentioned in the commit subject or message.
