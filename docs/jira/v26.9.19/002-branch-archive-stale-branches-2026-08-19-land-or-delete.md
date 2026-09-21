# mac-artifact-cleaner: land or delete remote branch `archive/stale-branches-2026-08-19`

- Standing: OPEN
- Created: 2026-09-19 (v26.9.19 gh survey wave)
- Source: remote branch `archive/stale-branches-2026-08-19` — not merged into `main`, no open PR
- Evidence: `git branch -r --no-merged origin/main` lists it; absent from `gh pr list` heads

## Work to complete
- Decide: open a PR (`gh pr create -R seanchatmangpt/mac-artifact-cleaner --head archive/stale-branches-2026-08-19`) or delete (`git push origin --delete archive/stale-branches-2026-08-19`).
- If superseded, delete; otherwise land through review.

## Acceptance
- After `git fetch --prune`, `git branch -r --no-merged origin/main` no longer lists `archive/stale-branches-2026-08-19`.

## History
- 2026-09-19 | OPEN | survey found PR-less unmerged branch | archive/stale-branches-2026-08-19 | decision pending
