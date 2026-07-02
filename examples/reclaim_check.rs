//! Demonstrates `check_reclaim` and `ReclaimCheck` — the receipt reality law
//! that compares claimed reclaim against the measured volume delta.
//!
//! Documented at: `src/domain/receipt.rs` (`check_reclaim`, `ReclaimCheck`)
//! Guide reference: `docs/TIME_MACHINE_MODEL.md` §3.4 "The APFS Snapshot Caveat"
//!
//! What you should see:
//!   not-applicable  -> small claim (< 1 GB floor) is skipped
//!   witnessed       -> measured delta >= 50% of claimed
//!   shortfall       -> measured delta == 0, large claim -> BytesFreedMismatch
//!   back-compat     -> None/None (old receipt) skips the law

use osx_clnr::domain::receipt::{check_reclaim, ReclaimCheck};

fn main() {
    // ── case 1: claim below the 1 GB floor — not applicable ──────────────────
    let result = check_reclaim(500_000_000, Some(10_000_000_000), Some(10_500_000_000));
    println!("not-applicable (500 MB claim):  {:?}", matches!(result, ReclaimCheck::NotApplicable));
    assert!(
        matches!(result, ReclaimCheck::NotApplicable),
        "claims below 1 GB floor must be NotApplicable"
    );

    // ── case 2: measured delta is within tolerance → witnessed ────────────────
    // claimed 4 GB, measured delta 3 GB (75% >= 50%) → Witnessed
    let claimed = 4_000_000_000u64;
    let before = 20_000_000_000u64;
    let after = before + 3_000_000_000; // 3 GB actually freed
    let result = check_reclaim(claimed, Some(before), Some(after));
    println!(
        "witnessed (3 GB measured, 4 GB claimed):  {:?}",
        matches!(result, ReclaimCheck::Witnessed)
    );
    assert!(
        matches!(result, ReclaimCheck::Witnessed),
        "75% recovery (>= 50% floor) must be Witnessed"
    );

    // ── case 3: zero movement but large claim → Shortfall (ghost variant lives) ─
    // This is the APFS snapshot-pinning scenario: files deleted but blocks still
    // pinned. The reclaim law surfaces it — this is the failure mode the whole
    // check_reclaim function exists to catch.
    let claimed = 3_000_000_000u64;
    let before = 20_000_000_000u64;
    let after = before; // disk didn't move
    let result = check_reclaim(claimed, Some(before), Some(after));
    println!(
        "shortfall (0 measured, 3 GB claimed):  {:?}",
        matches!(result, ReclaimCheck::Shortfall { .. })
    );
    assert!(
        matches!(result, ReclaimCheck::Shortfall { .. }),
        "zero movement against a large claim must be Shortfall (snapshot pinning signal)"
    );
    if let ReclaimCheck::Shortfall { claimed, measured } = result {
        println!("  claimed={} measured={}", claimed, measured);
        assert_eq!(claimed, 3_000_000_000);
        assert_eq!(measured, 0);
    }

    // ── case 4: back-compat — None/None (old receipts) skips the law ─────────
    let result = check_reclaim(5_000_000_000, None, None);
    println!("back-compat (None/None):  {:?}", matches!(result, ReclaimCheck::NotApplicable));
    assert!(
        matches!(result, ReclaimCheck::NotApplicable),
        "old receipts without volume samples must not raise a false mismatch"
    );

    println!("reclaim_check: all assertions passed");
}
