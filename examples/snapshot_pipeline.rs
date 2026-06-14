//! Demonstrates the APFS snapshot selection and thinning receipt pipeline —
//! a cross-product example showing `select_oldest_snapshots`,
//! `identify_thinned_snapshots`, and `SnapshotThinReceipt` composing together.
//!
//! Documented at: `src/domain/time.rs`
//! Guide reference: `docs/TIME_MACHINE_MODEL.md` §3 "APFS Snapshot Lifecycle"
//!
//! What you should see:
//!   select 2 oldest  -> ["2026-05-25-080000", "2026-05-26-135630"]
//!   thinned list     -> the two older snapshots removed
//!   receipt          -> snapshots_thinned matches thinned list
//!   edge: n=0        -> selects nothing (refusal case)
//!   edge: unparseable names are silently ignored

use osx_clnr::domain::time::{
    identify_thinned_snapshots, select_oldest_snapshots, SnapshotThinReceipt,
};

fn main() {
    let all_snapshots = vec![
        "com.apple.TimeMachine.2026-05-26-135630.local".to_string(),
        "com.apple.TimeMachine.2026-05-26-140000.local".to_string(),
        "com.apple.TimeMachine.2026-05-27-090000.local".to_string(),
        "com.apple.TimeMachine.2026-05-25-080000.local".to_string(),
    ];

    // ── select the 2 oldest ───────────────────────────────────────────────────
    let oldest_two = select_oldest_snapshots(&all_snapshots, 2);
    println!("select 2 oldest  -> {:?}", oldest_two);
    assert_eq!(
        oldest_two,
        vec!["2026-05-25-080000", "2026-05-26-135630"],
        "oldest first, by date suffix lexical order"
    );

    // ── simulate deletion: remove those two from the snapshot list ────────────
    let remaining: Vec<String> = all_snapshots
        .iter()
        .filter(|s| {
            !oldest_two
                .iter()
                .any(|date| s.contains(date.as_str()))
        })
        .cloned()
        .collect();

    // ── identify which snapshots were thinned ─────────────────────────────────
    let thinned = identify_thinned_snapshots(&all_snapshots, &remaining);
    println!("thinned list     -> {:?}", thinned);
    assert_eq!(thinned.len(), 2, "two snapshots must have been thinned");
    assert!(
        thinned
            .iter()
            .any(|s| s.contains("2026-05-25-080000")),
        "oldest must be in thinned list"
    );

    // ── build a receipt from the before/after snapshot lists ─────────────────
    let receipt = SnapshotThinReceipt::new(
        "/".to_string(),
        0, // count-driven delete, not byte-target
        1_716_768_000,
        all_snapshots.clone(),
        remaining.clone(),
    );
    println!("receipt          -> snapshots_thinned={:?}", receipt.snapshots_thinned);
    assert_eq!(
        receipt.snapshots_thinned.len(),
        2,
        "receipt must record the two thinned snapshots"
    );
    assert_eq!(receipt.volume, "/");
    assert_eq!(receipt.snapshots_before.len(), 4);
    assert_eq!(receipt.snapshots_after.len(), 2);

    // ── edge: n=0 selects nothing ─────────────────────────────────────────────
    let none_selected = select_oldest_snapshots(&all_snapshots, 0);
    println!("edge: n=0        -> {:?}", none_selected);
    assert!(none_selected.is_empty(), "n=0 must select nothing");

    // ── edge: unparseable names are ignored ───────────────────────────────────
    let garbage = vec!["not-a-snapshot".to_string(), "also-garbage".to_string()];
    let none = select_oldest_snapshots(&garbage, 5);
    println!("edge: unparseable -> {:?}", none);
    assert!(none.is_empty(), "unparseable names must be ignored");

    println!("snapshot_pipeline: all assertions passed");
}
