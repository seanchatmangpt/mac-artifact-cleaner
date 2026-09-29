# PERFORMANCE

Measured performance notes for `oclnr` scan, plan and delete on one machine (16 logical CPUs,
APFS, ~4.8M files under `/Users/sac`). **Last Updated:** 2026-09-28. Every number is wall clock
from a release build; treat differences under ~3 s as noise. Read this before adding
concurrency: two of the four experiments below made things slower.

## Shipped

| Change | Before | After | Where |
|---|---|---|---|
| Skip the `git` fork for candidates with no `.git` ancestor | plan build cold 61.1 s, warm 9.3 s | cold 45.4 s, warm 7.4 s | `integration/git_tracked.rs` |
| Size audit candidates in-process (`physical_dir_size`) instead of one `du` fork each | `audit run --ocel-output` 52-55 s | 42-48 s (bare scan: 45 s) | `nouns/audit.rs` |
| `plan build --verbose` prints phase timings (`[timing]` on stderr) | n/a | git filter 0.63 s, sizing 5.6 s | `nouns/plan.rs` |

Plan item count was 31 before and after; the git filter keeps the same candidates.

## Floors (kernel-bound, not worth more threads)

- Cold home scan is ~45 s: a `sample` profile is dominated by `lstat`/`getdirentries`, and the
  scan is already parallel per root and per directory (`ignore::WalkBuilder::build_parallel`).
- A warm scan cache brings it to ~0.8 s but can miss a new deep project (directory mtime does
  not reflect deep changes).
- Sizing 1,109 candidates takes ~5.6 s and is bound by stats of every file under them.

## Falsified experiments (not shipped)

| Idea | Result |
|---|---|
| One shared CPU-sized rayon pool for `jwalk` sizing, to remove the long tail of one huge `target/` | sizing 6.2 s -> 6.8-7.0 s (3 runs): slower |
| Parallel unlink in `delete_dir_all_with_progress`, files chunked across threads | 100k files: serial 6.3-6.8 s vs 7.2-7.7 s (3 rounds each): slower |
| Same, but one worker per parent directory | 11.7-14.4 s: about 2x slower |

APFS serializes unlinks in the kernel, so more threads add contention. The per-item `par_iter`
in `delete execute` (different directories at once) is the only delete-side parallelism kept.
Method for the delete numbers: a fresh 100k-file fixture per run, 12 s pause to let Spotlight
settle, three alternating rounds; an uncontrolled first attempt varied 13-18 s for identical code.

## See Also
- [Gall Checkpoints](GALL_CHECKPOINTS.md)
- [Future Capabilities](FUTURE_CAPABILITIES.md)
