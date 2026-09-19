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

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::{
    integration::monitor::{check_and_notify, DiskPressureCheck},
    nouns::autoclean,
};

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

/// Pure trigger decision for pressure-activated cleanup: only while under
/// pressure, and only when the previous trigger (if any) is at least
/// `cooldown_hours` old. The cooldown bounds how often an ongoing pressure
/// situation re-fires the pipeline — the daily launchd schedule stays the
/// primary cadence, this is the on-demand path.
///
/// ```
/// use osx_clnr::nouns::monitor::should_trigger_pressure_autoclean;
///
/// let now = 1_000_000;
/// let hour = 3_600;
///
/// // Positive: pressure with no prior trigger fires immediately.
/// assert!(should_trigger_pressure_autoclean(true, None, now, 6));
///
/// // Refusal: no pressure never triggers, regardless of history.
/// assert!(!should_trigger_pressure_autoclean(false, None, now, 6));
///
/// // Refusal: a trigger 1 hour ago is inside a 6-hour cooldown.
/// assert!(!should_trigger_pressure_autoclean(true, Some(now - hour), now, 6));
///
/// // Positive: a trigger exactly the cooldown age fires again (`>=`).
/// assert!(should_trigger_pressure_autoclean(true, Some(now - 6 * hour), now, 6));
/// ```
pub fn should_trigger_pressure_autoclean(
    under_pressure: bool,
    last_trigger_unix: Option<i64>,
    now_unix: i64,
    cooldown_hours: u64,
) -> bool {
    if !under_pressure {
        return false;
    }
    match last_trigger_unix {
        None => true,
        Some(last) => now_unix.saturating_sub(last) >= (cooldown_hours as i64).saturating_mul(3600),
    }
}

/// Runs the same lawful `autoclean run --yes` pipeline the daily launchd job
/// runs, as a subprocess of this binary, anchored to the home directory.
/// The trigger stamp is written *before* the run so a crash or hang still
/// holds the cooldown open (a pressure situation must not be able to stack
/// cleanup runs on top of each other every monitor interval).
fn trigger_autoclean_run() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    autoclean::write_trigger_stamp(chrono::Utc::now().timestamp())?;
    println!(
        "[oclnr monitor] under pressure — triggering `autoclean run --yes` (cooldown recorded)..."
    );
    let status =
        Command::new(&exe).current_dir(&home).args(["autoclean", "run", "--yes"]).status()?;
    if status.success() {
        println!("[oclnr monitor] triggered autoclean run finished OK");
    } else {
        eprintln!("[oclnr monitor] triggered autoclean run FAILED (exit {status})");
    }
    Ok(())
}

pub fn handle(
    threshold_gb: f64,
    mount: String,
    watch: bool,
    interval_secs: u64,
    trigger_autoclean: bool,
    cooldown_hours: u64,
) -> anyhow::Result<()> {
    let mount_path: PathBuf = PathBuf::from(&mount);

    // One pass: notify (as always), then — only when explicitly enabled and
    // outside the cooldown — fire the capped autoclean pipeline.
    let check_once = |mount_path: &Path| -> anyhow::Result<()> {
        let check = check_and_notify(mount_path, threshold_gb)?;
        report(check, &mount);
        if trigger_autoclean {
            let now = chrono::Utc::now().timestamp();
            let last = autoclean::read_last_trigger_unix();
            if should_trigger_pressure_autoclean(check.under_pressure, last, now, cooldown_hours) {
                trigger_autoclean_run()?;
            } else if check.under_pressure {
                let last_display = last
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| "never".to_string());
                println!(
                    "[oclnr monitor] under pressure but autoclean trigger is in cooldown \
                     (last: {last_display}, cooldown: {cooldown_hours}h) — notifying only"
                );
            }
        }
        Ok(())
    };

    if !watch {
        check_once(&mount_path)?;
        return Ok(());
    }

    println!(
        "[oclnr monitor] watching {} every {}s (threshold {:.1} GB, autoclean trigger: {}); Ctrl-C to stop",
        mount,
        interval_secs,
        threshold_gb,
        if trigger_autoclean { "on" } else { "off" },
    );
    loop {
        match check_once(&mount_path) {
            Ok(()) => {}
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
