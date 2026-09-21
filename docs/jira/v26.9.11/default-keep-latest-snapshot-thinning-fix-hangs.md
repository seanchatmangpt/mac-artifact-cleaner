# Default cleaning to keep-latest snapshot thinning; fix two real hangs and a delete-both bug

## Summary

While running a real cleanup session against this machine, three real defects
were found and fixed: snapshot thinning was not part of the default
`autoclean run` path (so freed space wasn't actually released), an iCloud
Drive exclusion check was dead code and caused multi-minute hangs when
`plan build` / `audit breakdown` walked into a CloudDocs placeholder file, and
the "keep latest snapshot" selection logic computed its keep-count from the
wrong population (raw snapshot count instead of dated-snapshot count),
causing it to delete both dated snapshots instead of keeping one.

## Status

Done — already merged/committed.

## Commits

- 3a96b0b Default cleaning runs to keep-latest snapshot thinning; fix two real hangs and a delete-both bug

## Changes

- Added `snapshot delete --which keep-latest`, which deletes every dated
  `com.apple.TimeMachine.*` local snapshot except the single most recent.
- Wired `snapshot delete --which keep-latest` into `autoclean run` as an
  unconditional final step after a successful delete execute, making snapshot
  thinning the default posture rather than an opt-in extra step.
- Fixed `is_macos_os_dir` never actually excluding iCloud Drive: it tested for
  the substring `/Library/Application Support/CloudDocs` (an unrelated path)
  instead of iCloud Drive's real location
  (`Library/Mobile Documents/com~apple~CloudDocs`), inside a branch that had
  already required NOT containing that substring — making the check dead code
  twice over. Removed the dead branch and excluded iCloud Drive outright, the
  same way `/System` and `/Library` already were.
- Extracted `select_snapshots_to_keep_latest` into a real domain function
  (previously inlined as `before.len().saturating_sub(1)` in
  `nouns/snapshot.rs`), fixing it to compute "how many to keep" from the
  *dated* snapshot count rather than the raw snapshot count (which also
  includes non-dated `com.apple.os.update-*` snapshots `parse_snapshot_date`
  never matches). The new function is used by both
  `snapshot delete --which keep-latest` and `autoclean run`, and ships with
  its own doctest reproducing the exact failure scenario.
- Files touched: `src/domain/artifact.rs` (+23/-4), `src/domain/time.rs`
  (+39 new), `src/nouns/autoclean.rs` (+45 new), `src/nouns/snapshot.rs`
  (+12/-2). Total: 4 files changed, 113 insertions, 6 deletions.

## Verification

Per the commit message, stated as done during this session:

- `cargo build` / `cargo build --release`: exit 0
- `cargo fmt -- --check`: clean
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test`: same pre-existing `test_github_discover_candidates` failure as
  before this change (confirmed via `git stash` on the prior commit earlier in
  the session) — nothing else regressed
- `cargo test --doc`: 109 passed (including the new
  `select_snapshots_to_keep_latest` doctest)
- Reproduced the iCloud hang fix directly: the same
  `plan build --root /Users/<user> --deps --aggressive` invocation that hung 12+
  minutes before the fix completed in 59 seconds after it
- Ran the real pipeline end to end: `audit scan` -> `plan build` (7.16GB) ->
  `plan approve` -> `delete execute` (7.09GB freed, 1 permission failure on a
  go module cache file, noted as pre-existing/unrelated) ->
  `snapshot delete --which keep-latest` (76.27GB reclaimed, confirmed via
  `df` free space going from 4.8GB to 144GB)

## Related

None stated — no PR numbers or branch names referenced in the commit subject
or message.
