# mac-artifact-cleaner migration plan v26.11.16

Standing: UNVERIFIED

## Identity
- Path: /Users/sac/mac-artifact-cleaner
- Remote: https://github.com/seanchatmangpt/mac-artifact-cleaner.git (upstream origin/main)
- Branch@sha: main@b3f5565
- Last commit: 2026-09-26; commits: 106; size: 7 MB
- Languages/build: Rust (Cargo.toml, rust-toolchain.toml, deny.toml), Justfile; vendor/, ui/
- Nested repos: none

## Observed state
- Dirty: 1 — M CHANGELOG.md
- Untracked: 0; stash: 0
- Ahead/behind origin/main: 0/39 — remote is 39 commits ahead of local main.
- Unpushed local branch: v26926/redaction-integration-repair@2afb58a (no upstream)
- Build/dep dirs: no target/ present.
- Submodules: none

## What it is
Pentecost (`osx-clnr`, binaries `oclnr` and `oclnr-mcp`): a plan-bound macOS developer disk
auditor and cleanup utility that emits OCEL evidence and deletes only from approved plans.
First "praxis" house-standard project.

## Classification
ACTIVE — census ACTIVE; local is stale (39 behind) but the project is clearly alive upstream.

## Migration risk
LOW. One CHANGELOG edit and one unpushed branch (redaction fixture fix) would be lost;
everything else is on the remote. No secrets.

## Recommended action
COMMIT-PUSH
1. `cd /Users/sac/mac-artifact-cleaner && git diff CHANGELOG.md` — DECISION: keep or discard.
2. `git push -u origin v26926/redaction-integration-repair` (unpushed branch).
3. If keeping the CHANGELOG edit: `git switch -c wip/changelog && git commit -am "wip: changelog" && git push -u origin HEAD`
4. Re-clone on the new machine (local main is 39 behind; a fresh clone is simpler than pull).

## First commands on the new machine
`git clone https://github.com/seanchatmangpt/mac-artifact-cleaner.git && cd mac-artifact-cleaner && cargo check --all-targets`
then `just --list`.

## See Also
- ../v26.9.19/
- /Users/sac/docs/jira/v26.11.16/README.md
