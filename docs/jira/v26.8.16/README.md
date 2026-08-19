# v26.8.16 — Scan Reliability Fixes

Two production bugs in the `oclnr` scan/plan pipeline, found via three consecutive
days of the `ecosystem-disk-hygiene` recurring workflow (2026-08-13 through
2026-08-16) and fixed the same day they were root-caused. Status: **ALIVE** —
both fixes compile, pass real tests, and were verified against the unmodified
tree to confirm they are not masking a pre-existing failure elsewhere.

## Summary

| # | Bug | File | Severity | Status |
|---|-----|------|----------|--------|
| 1 | Nested rayon thread-pool storm in `plan build` | `src/integration/fs.rs` | Perf — 30-70+ min stalls | ALIVE |
| 2 | False-positive `agents`/`logs` deletion candidates | `src/domain/artifact.rs` | Correctness — real data at risk | ALIVE |

## Background: how these were found

The `ecosystem-disk-hygiene` workflow (a recurring cron-driven `oclnr` sweep of
`/Users/sac`) ran daily starting 2026-08-13. Days 1 and 2 completed but flagged
`"ai agents dir"` / `"ai tool logs"` categories for manual review — the review
agents caught the scanner nominating real project directories
(`/Users/sac/mmdio/.agents`, `/Users/sac/ferroplan/plugins/chatman-ecosystem/agents`,
`.../logs`) for deletion, purely because of their directory name. Day 3
(2026-08-15) failed outright: `plan(action:"build")` stalled past the MCP
client's idle timeout on all 6 retry attempts, burning ~7 hours of wall time
before the underlying cause was diagnosed.

## Bug 1 — nested rayon pool storm

**Symptom:** `plan build` took 30–70+ minutes on a 218-candidate scan, versus
running fine at 119 candidates on prior days. The MCP client's per-call idle
timeout (~1837s) aborted the tool call client-side while the server call kept
running in the background, eventually finishing ~6 minutes after the client
gave up — confirming the call was extremely slow, not infinitely hung.

**Root cause:** `physical_dir_size` (`src/integration/fs.rs`) used
`jwalk::Parallelism::RayonNewPool(0)`, which spins up a brand-new,
all-CPU-sized rayon thread pool on **every call**. `src/nouns/plan.rs` calls
it once per plan candidate, inside an outer `candidate_vec.par_iter()`. With
N candidates in flight concurrently under that outer `par_iter`, N nested
full-size thread pools contend for the same CPUs simultaneously — contention
that scales with candidate count, matching the observed 119-fine /
218-stalled split exactly.

**Fix:** split into two functions —
- `physical_dir_size(path)` — now walks with `Parallelism::Serial`. Safe to
  call from inside an outer `par_iter` (the `plan.rs` case), since the outer
  loop already supplies cross-candidate parallelism; each individual walk
  just needs to run on its own worker thread.
- `physical_dir_size_parallel(path)` — keeps the original
  `RayonNewPool(0)` behavior, for the one other caller
  (`src/nouns/emergency.rs`), which walks a handful of cache paths
  sequentially on the main thread with no outer parallel context — there,
  jwalk spinning up its own pool per walk is a net win with no nesting hazard.

**Test:** `many_calls_from_nested_par_iter_complete_quickly`
(`src/integration/fs.rs`) — 60 real temp directories, each summed via
`physical_dir_size` from inside a real `rayon::par_iter`, asserts wall-clock
completion under 5s. Reproduces the exact call shape that caused the
production stall.

```
$ cargo test physical_dir_size
running 4 tests
test integration::fs::physical_dir_size_tests::permission_denied_subtree_silently_undercounts_documented_gap ... ok
test integration::fs::physical_dir_size_tests::empty_dir_has_zero_size ... ok
test integration::fs::physical_dir_size_tests::sums_physical_size_of_real_files ... ok
test integration::fs::physical_dir_size_tests::many_calls_from_nested_par_iter_complete_quickly ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 68 filtered out; finished in 0.04s
```

## Bug 2 — false-positive `agents`/`logs` classification

**Symptom:** Three consecutive daily scans nominated real, authored project
directories for deletion:

- `/Users/sac/mmdio/.agents` — numbered agent-run subdirectories, live content
- `/Users/sac/ferroplan/plugins/chatman-ecosystem/agents` — hand-written
  markdown agent definitions
- `/Users/sac/ferroplan/plugins/chatman-ecosystem/logs` — real `.ocel.json`
  event-log artifacts from a live project

**Root cause:** `detect_project_from_snapshot` (`src/domain/artifact.rs`)
tagged **any** project `"ai_project"` purely from the presence of a
root-level `tmp`, `logs`, `chats`, or `agents` subdirectory — no other
signal. `artifact_candidates_from_snapshot`'s `"ai_project"` match arm then
unconditionally nominated the bare `agents`/`logs` dirs for deletion with
zero check on actual content. These are generic names any project can
legitimately have (a web app's `logs/`, a multi-agent framework's `agents/`
source dir) — directory name alone was not sufficient signal.

**Fix:**
- Removed the standalone `"ai_project"` trigger keyed on bare
  `tmp`/`logs`/`chats`/`agents`. Kept only the stronger-signal trigger:
  `.agents`, `.gemini`, or `.claude` present (genuine, dot-prefixed AI-tool
  marker directories — much lower false-positive risk).
- In the `"ai_project"` candidate-nomination arm, removed the unconditional
  `add_dir(..., "agents", "ai agents dir", ...)` and
  `add_dir(..., "logs", "ai tool logs", ...)` calls. Kept nomination of only
  `.agents`, `.gemini/tmp`, `.claude/tmp` — paths that are structurally
  tool-internal, not generic project directories.
- `.claude`'s mere presence is deliberately *not* used to nominate bare
  `agents`/`logs` either, since `.claude` is also legitimate Claude Code
  project config, not exclusively an AI-scratch-space marker.

**Acceptance criterion:** a project with just `Cargo.toml` and a root-level
`logs/` or `agents/` directory, with no `.agents`/`.gemini`/`.claude`
markers, must not produce `"ai tool logs"` or `"ai agents dir"` candidates.
Genuine `.agents`/`.claude/tmp` markers must still be detected and nominated
— the fix narrows false positives without regressing real detection.

**Tests:** `src/domain/artifact.rs::ai_project_false_positive_tests` —
`bare_logs_and_agents_dirs_are_not_flagged_as_ai_project` (negative case,
mirrors the exact production false positives above) and
`genuine_ai_tool_markers_are_still_detected_and_nominated` (positive case,
regression guard against over-correcting).

```
$ cargo test --lib artifact && cargo test --doc artifact
test domain::artifact::ai_project_false_positive_tests::genuine_ai_tool_markers_are_still_detected_and_nominated ... ok
test domain::artifact::ai_project_false_positive_tests::bare_logs_and_agents_dirs_are_not_flagged_as_ai_project ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 68 filtered out; finished in 0.00s
Doc-tests: test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 78 filtered out; finished in 12.90s
```

## Full verification

Real command output, run against the working tree with both fixes applied:

```
$ cargo fmt -- --check
# clean (one formatting diff in the new test module was found and applied
# via `cargo fmt`; re-run confirms clean)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.57s
# 0 warnings

$ OCLNR_BIN=$(pwd)/target/debug/oclnr cargo test
# 1 failure: tests/github_tests.rs::test_github_discover_candidates
#   (assertion left==right failed, left: 10, right: 9)
```

The `github_tests.rs` failure is **pre-existing**, not a regression —
confirmed via `git stash` + rerun against the unmodified tree, where it fails
identically. It uses `MockCommandExecutor` for the `gh` CLI subprocess, which
is this repo's one documented legitimate mocking exception (external GitHub
API, genuinely infeasible in-process) — not a testing-discipline violation.
Left untouched; out of scope for this milestone.

```
$ grep -rn "unittest.mock\|Mock(\|MagicMock\|patch(\|monkeypatch" src/ tests/
# zero matches
```

## Not done in this milestone

- `tests/github_tests.rs::test_github_discover_candidates` — pre-existing
  failure, confirmed unrelated, left for separate triage.
- Fixes are **uncommitted** in the working tree as of this doc
  (`src/domain/artifact.rs`, `src/integration/fs.rs`,
  `src/nouns/emergency.rs` modified, nothing staged).

## See Also

- [`../../GALL_CHECKPOINTS.md`](../../GALL_CHECKPOINTS.md) — evolutionary
  milestone framework this project follows
- `CLAUDE.md` (repo root) — MCP server tool surface, disk cleanup protocol
- `~/.claude/rules/testing-chicago-style.md` — real-collaborator testing
  discipline both fixes' tests follow (real `DirSnapshot`/`EntrySnapshot`
  construction, real temp directories on disk — no mocking)
