//! The real `oclnr` binary under test must pass preflight for every plist
//! its own `daemon install-*` generators emit — the invariant whose absence
//! let `com.oclnr.pressure` crash-loop on a stale build.

use osx_clnr::integration::daemon_binary::preflight_plist;

fn bin() -> String {
    env!("CARGO_BIN_EXE_oclnr").to_string()
}

#[test]
fn this_build_accepts_its_own_pressure_plist() {
    let dir = tempfile::tempdir().unwrap();
    let plist = osx_clnr::nouns::daemon::generate_pressure_plist(
        &bin(),
        20.0,
        60,
        "snapshots,builds",
        dir.path(),
    );
    assert_eq!(preflight_plist(&plist).unwrap(), Vec::<String>::new());
}

#[test]
fn this_build_accepts_its_own_autoclean_and_monitor_plists() {
    for plist in [
        osx_clnr::nouns::daemon::generate_autoclean_plist_for(&bin(), 50.0, 24, 4, 15),
        osx_clnr::nouns::daemon::generate_monitor_plist_for(&bin(), 20.0, 3600, true, 6),
    ] {
        assert_eq!(preflight_plist(&plist).unwrap(), Vec::<String>::new(), "{plist}");
    }
}

/// Falsifier against a real historical build: set `OCLNR_STALE_BINARY` to an
/// oclnr binary that predates `monitor --reclaim` (on the machine this was
/// written on: `~/.local/bin/oclnr.bak-20260919-99bf1494`, the build that
/// crash-looped `com.oclnr.pressure`). Skips with a printed reason otherwise.
#[test]
fn stale_real_build_is_refused_for_pressure_plist() {
    let Ok(stale) = std::env::var("OCLNR_STALE_BINARY") else {
        eprintln!("SKIPPED: OCLNR_STALE_BINARY not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let plist =
        osx_clnr::nouns::daemon::generate_pressure_plist(&stale, 20.0, 60, "snapshots", dir.path());
    let missing = preflight_plist(&plist).expect("stale binary still runs `monitor --help`");
    assert!(missing.contains(&"--reclaim".to_string()), "{missing:?}");
}
