# Exclude the running binary's own target dir from cleaning plan build

## Summary

A live cleanup session against `~/osx-clnr` deleted its own `target/` directory via a
normal `plan build` -> `delete execute` pass. That `target/` happened to contain the
`oclnr` binary that the MCP server (`oclnr-mcp`) shells out to (co-located resolution
in `src/mcp/subprocess.rs`), so every subsequent MCP call failed with a generic
"Subprocess failed" error until the binary was rebuilt by hand mid-session. This
ticket documents the completed fix that excludes any candidate path that is an
ancestor of the running binary's own executable path.

## Status

Done - already merged/committed.

## Commits

- daa0036 Exclude the running binary's own target dir from plan build

## Changes

- Added `domain::artifact::exclude_self_binary_ancestors`: a pure filter that drops
  any candidate whose path is an ancestor of `std::env::current_exe()`.
- Wired the filter into `nouns::plan::handle` (`PlanAction::Build`), applied right
  after candidates are collected, before sizing/sorting.
- `None` exe_path (e.g. sandboxed callers) is a no-op passthrough — no candidates are
  dropped when the current executable path cannot be resolved.
- `src/domain/artifact.rs`: +47 lines (new filter + doctest).
- `src/nouns/plan.rs`: +12 lines (wiring into plan build).

## Verification

Per the commit message:

- Doctest passes.
- `fmt`/`clippy` clean.
- A real `oclnr plan build --root /Users/<user>/osx-clnr` run against this repo's own
  checkout now nominates 0 items instead of the previous `target/` candidate —
  reproducing the actual incident and confirming the fix.

## Related

- Claude-Session: https://claude.ai/code/session_01N64smjnqLFcuxvQAa1xn5U (stated in
  the commit message)
- No PR numbers or branch names stated in the commit subject.
