//! CLI-level regression tests for `plan build --include-global-caches`
//! nomination merging.
//!
//! Before `merge_global_cache_candidates` existed, `plan build` skipped a
//! curated global-cache nomination whenever it overlapped ANY scanned
//! candidate in EITHER direction. The descendant-suppresses-ancestor half of
//! that rule inverted the priorities: a tiny scanned sub-item inside a cache
//! dir vetoed the multi-GB curated nomination of the cache dir itself. On
//! the real machine this was diagnosed on, a 0-byte scanned
//! `~/.cargo/registry/src/…/oxrocksdb-sys/lz4/build` suppressed the whole
//! `~/.cargo/registry/src` nomination, and `.cache/tmp/*/deps` scan hits
//! suppressed `~/.cache` — together ~11.5 GB per plan never reached the
//! plan file. These tests exercise the real binary end-to-end (real scan,
//! real merge, real plan JSON) against a fixture HOME.

use std::{fs, path::Path, process::Command};

use osx_clnr::domain::plan::DeletionPlan;

fn write_file(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, vec![b'x'; bytes]).expect("write");
}

/// Builds a fixture HOME shaped like the failure mode:
/// - an elixir project under `.cache/tmp/e2e/support_desk` (the scanner
///   nominates its `deps/`),
/// - a `Library/Caches/Homebrew` download cache (the newly added allowlist
///   entry the old list never covered).
fn fixture_home(root: &Path) {
    write_file(&root.join(".cache/tmp/e2e/support_desk/deps/phoenix/ebin.beam"), 64 * 1024);
    write_file(&root.join(".cache/tmp/e2e/support_desk/mix.exs"), 128);
    write_file(&root.join("Library/Caches/Homebrew/downloads/bottle.tar.gz"), 128 * 1024);
}

fn run_plan_build(home: &Path, out: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
        .env("HOME", home)
        .args([
            "plan",
            "build",
            "--root",
            home.to_str().expect("utf8 home"),
            "--deps",
            "--ignore-recent-hours",
            "0",
            "--include-global-caches",
            "--output",
            out.to_str().expect("utf8 out"),
        ])
        .output()
        .expect("run oclnr plan build")
}

#[test]
fn global_cache_ancestor_swallows_scanned_descendants() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fixture_home(&home);
    let out = tmp.path().join("plan.json");

    let output = run_plan_build(&home, &out);
    assert!(
        output.status.success(),
        "plan build failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan: DeletionPlan =
        serde_json::from_str(&fs::read_to_string(&out).expect("read plan")).expect("parse plan");

    // The curated `~/.cache` root IS nominated (ancestor preference)…
    assert!(
        plan.items.iter().any(|i| i.path == home.join(".cache")),
        "expected the whole .cache dir to be nominated, got: {:?}",
        plan.items.iter().map(|i| &i.path).collect::<Vec<_>>()
    );
    // …and the scanned `deps/` inside it is NOT (it would race the ancestor
    // under the parallel delete executor, and it captured only a fraction of
    // the bytes).
    assert!(
        !plan.items.iter().any(|i| i.path.ends_with("support_desk/deps")),
        "scanned sub-item inside a nominated cache dir must be swallowed: {:?}",
        plan.items.iter().map(|i| &i.path).collect::<Vec<_>>()
    );
    // The swallowed bytes are still accounted for: the `.cache` item must
    // cover at least what the deps dir held.
    let cache_item =
        plan.items.iter().find(|i| i.path == home.join(".cache")).expect(".cache item");
    assert!(
        cache_item.bytes >= 64 * 1024,
        "ancestor byte count must include swallowed descendants"
    );
}

#[test]
fn library_caches_allowlist_covers_common_macos_caches() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fixture_home(&home);
    let out = tmp.path().join("plan.json");

    let output = run_plan_build(&home, &out);
    assert!(output.status.success(), "plan build failed");

    let plan: DeletionPlan =
        serde_json::from_str(&fs::read_to_string(&out).expect("read plan")).expect("parse plan");

    // The old allowlist named only `Mozilla.sccache` — real machines pile up
    // Homebrew/go-build/ms-playwright/uv downloads instead, none of which
    // were ever nominated. Homebrew is present in the fixture, so it must
    // appear as its own auditable plan line.
    assert!(
        plan.items.iter().any(|i| i.path == home.join("Library/Caches/Homebrew")),
        "expected Library/Caches/Homebrew nomination, got: {:?}",
        plan.items.iter().map(|i| &i.path).collect::<Vec<_>>()
    );
    // The wholesale ~/Library/Caches parent is still never nominated.
    assert!(!plan.items.iter().any(|i| i.path == home.join("Library/Caches")));
}
