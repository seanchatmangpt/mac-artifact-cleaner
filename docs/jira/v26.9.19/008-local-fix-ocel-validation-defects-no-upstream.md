# mac-artifact-cleaner: push or delete local-only branch `fix/ocel-validation-defects`

- Standing: OPEN
- Created: 2026-09-19 (v26.9.19 gh survey wave)
- Source: local branch `fix/ocel-validation-defects` has 20 commit(s) not on `origin/main`, no upstream
- Evidence: `git rev-list --count origin/main..fix/ocel-validation-defects` = 20

## Work to complete
- Push (`git push -u origin fix/ocel-validation-defects`) if the work matters; otherwise delete the branch after confirming the commits are obsolete.

## Acceptance
- Branch pushed and visible on GitHub, or deleted locally with commits confirmed recoverable-or-unwanted.

## History
- 2026-09-19 | OPEN | survey found local-only branch | fix/ocel-validation-defects (20 commits) | decision pending
