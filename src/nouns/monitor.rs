//! Disk-pressure monitor noun.
//!
//! This is the command the `daemon install` launchd plist invokes
//! (`{binary} monitor --threshold-gb {x}`). launchd's `StartInterval` already
//! re-fires the job periodically, so by default this performs a single
//! check-and-notify pass and exits. Pass `--loop` to instead poll
//! continuously (useful when running outside of launchd).
//!
//! **Noun layer rule**: this module parses, routes, and formats output only.
//! The actual `statvfs(2)` call and notification delivery live in
//! `integration::monitor`.

use crate::integration::monitor::{check_and_notify, DiskPressureCheck};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn report(check: DiskPressureCheck, mount: &str) {
    if check.under_pressure {
        println!(
            "[oclnr monitor] {}: {:.1} GB free (threshold {:.1} GB) — UNDER PRESSURE, notification sent",
            mount, check.free_gb, check.threshold_gb
        );
    } else {
        println!(
            "[oclnr monitor] {}: {:.1} GB free (threshold {:.1} GB) — OK",
            mount, check.free_gb, check.threshold_gb
        );
    }
}

pub fn handle(
    threshold_gb: f64,
    mount: String,
    watch: bool,
    interval_secs: u64,
) -> anyhow::Result<()> {
    let mount_path: PathBuf = PathBuf::from(&mount);

    if !watch {
        let check = check_and_notify(&mount_path, threshold_gb)?;
        report(check, &mount);
        return Ok(());
    }

    println!(
        "[oclnr monitor] watching {} every {}s (threshold {:.1} GB); Ctrl-C to stop",
        mount, interval_secs, threshold_gb
    );
    loop {
        match check_and_notify(&mount_path, threshold_gb) {
            Ok(check) => report(check, &mount),
            Err(e) => eprintln!("[oclnr monitor] check failed: {}", e),
        }
        std::thread::sleep(Duration::from_secs(interval_secs));
    }
}

/// Exposed for tests / callers that already have a `Path`.
#[allow(dead_code)]
pub fn check_once(mount: &Path, threshold_gb: f64) -> anyhow::Result<DiskPressureCheck> {
    check_and_notify(mount, threshold_gb)
}
