# osx-clnr migration plan v26.11.16

Standing: UNVERIFIED

## Identity
- Path: /Users/sac/osx-clnr
- Remote: https://github.com/seanchatmangpt/mac-artifact-cleaner.git (origin/main)
- Branch: main@24cbeaf; last commit 2026-10-22; 154 commits; 1375 MB
- Languages/build: Rust (Cargo.toml, build.rs), Justfile canonical; CLAUDE.md + AGENTS.md
- Nested repos: vendor/autofde-lab — registered submodule @61840ec (heads/master), inside
  vendor/ (298 MB total)

## Observed state
- Dirty 13: src/ (10: domain/artifact,dcm,pressure; integration/fs,monitor,notify;
  nouns/autoclean,daemon,mod), AGENTS.md, CLAUDE.md, README.md, plus receipt JSONs
- Untracked 3: dev-cleanup-receipt.thin.json, deletion-receipt-graphlaw.json,
  cleanup-receipt.thin.json
- Ahead 0 / behind 0 on main; stash count 75 (census) — very large
- Unpushed local branches 16: chore/pre-cleanup-review-2026-08-19, feat/autoclean-ocel-
  evidence, feat/colima-management, feat/mcp-docker-colima-prune, fix-parallel-delete-
  ordering-agent, fix/byte-truth-and-autonomics, fix/ocel-validation-defects,
  fix/photos-tcc-scan-fence, redact-wiring-g8, redact-wiring-g8-impl, release/v26.9.21,
  worktree-agent-* (3), worktree-wf_* (2). Only main and fix/v26926-redaction-test-fixture
  track origin.
- Big dirs: .git 319 MB, vendor/ 298 MB, archive/ 30 MB, disk-audit.jsonocel 3.8 MB,
  archive-2026-09-18, archive-2026-09-21; .oclnr-cache/ present. No target/ present.

## What it is
Pentecost (`oclnr`): plan-bound macOS developer disk auditor/cleaner that emits OCEL evidence,
deletes only from approved plans, and records receipts. First praxis-standard project.

## Classification
ACTIVE (census agrees)

## Migration risk
HIGH. 16 unpushed branches, 75 stashes, 13 dirty src files in domain/integration/nouns, and
local receipt/evidence JSONs. .mcp.json present (check for keys). .oclnr-cache is
regenerable.

## Recommended action
COMMIT-PUSH
1. `cd /Users/sac/osx-clnr && git diff --stat` — review the 13-file WIP.
2. `git checkout -b wip/migration-2026-11 && git add -A && git commit -m "wip: pre-migration"`
3. `git push -u origin wip/migration-2026-11`
4. `git push origin --all` — pushes all 16 local branches (DECISION: prune worktree-agent-*
   and worktree-wf_* branches first if they are obsolete).
5. DECISION: 75 stashes — triage on the new machine. Preserve them via a bundle:
   `git bundle create ~/migration-bundles/osx-clnr-full.bundle --all` (stashes are NOT in
   bundles; to keep them, `git stash list` and `git stash show -p N > stash-N.patch` for the
   ones worth keeping — DECISION: Sean picks which).
6. `grep -il 'key\|token' .mcp.json` — audit before relying on push.
7. `git submodule status` — autofde-lab is pinned; re-init on new machine, do not copy vendor/.
8. `rm -rf .oclnr-cache archive-2026-09-18 archive-2026-09-21` after confirming archived.
9. Re-clone with `--recurse-submodules`; do not copy the 1.4 GB tree.

## First commands on the new machine
`git clone --recurse-submodules https://github.com/seanchatmangpt/mac-artifact-cleaner.git osx-clnr`
`cd osx-clnr && cargo check && just --list`

## See Also
- ../v26.9.19/ and ../v26.9.11/
- /Users/sac/docs/jira/v26.11.16/README.md (master index)
