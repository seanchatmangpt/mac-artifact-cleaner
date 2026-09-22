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
    domain::pressure::{decide_reclaim, Decision, ReclaimModes},
    integration::{
        fs::volume_space,
        monitor::{check_and_notify, DiskPressureCheck},
        notify::notify_disk_pressure,
        pressure::{read_stamp, stamp_path, write_stamp},
    },
    nouns::{autoclean, snapshot},
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
    let status = Command::new(&exe)
        .current_dir(&home)
        .args(["autoclean", "run", "--yes"])
        // Tag the run so its OCEL evidence records `trigger: pressure`
        // rather than the default `scheduled`.
        .env("OCLNR_AUTOCLEAN_TRIGGER", "pressure")
        .status()?;
    if status.success() {
        println!("[oclnr monitor] triggered autoclean run finished OK");
    } else {
        eprintln!("[oclnr monitor] triggered autoclean run FAILED (exit {status})");
    }
    Ok(())
}

/// Pressure-reclaim configuration for `monitor --reclaim ...`.
#[derive(Debug, Clone)]
pub struct ReclaimConfig {
    pub modes: ReclaimModes,
    pub margin_gb: f64,
    pub snapshot_cooldown_secs: u64,
    pub builds_cooldown_secs: u64,
    pub urgency: u8,
    pub receipt_dir: PathBuf,
    pub builds_max_reclaim_gb: f64,
    pub builds_ignore_recent_hours: u64,
}

const SNAPSHOT_STAMP: &str = "last-pressure-snapshot-thin";
const BUILDS_STAMP: &str = "last-pressure-builds-reclaim";
const BYTES_PER_GIB: f64 = 1_073_741_824.0;

fn gb_to_bytes(gb: f64) -> u64 {
    (gb.max(0.0) * BYTES_PER_GIB) as u64
}

/// Default receipt directory for pressure-triggered reclaims.
pub fn default_pressure_receipt_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("Library/Logs/oclnr/pressure")
}

/// One pressure tick: sample free space (statvfs), notify if under pressure,
/// then run each enabled reclaim strategy through its pure decision.
/// Snapshots go first (cheap, and the only way to release blocks pinned by a
/// local APFS snapshot); builds are decided on a fresh sample afterwards, so a
/// thin that already cleared the pressure does not also delete build dirs.
fn pressure_tick(mount: &str, threshold_gb: f64, cfg: &ReclaimConfig) -> anyhow::Result<()> {
    let threshold_bytes = gb_to_bytes(threshold_gb);
    let margin_bytes = gb_to_bytes(cfg.margin_gb);
    let free = volume_space(Path::new(mount))?.available;
    let check = DiskPressureCheck {
        free_gb: free as f64 / BYTES_PER_GIB,
        threshold_gb,
        under_pressure: free < threshold_bytes,
    };
    if check.under_pressure {
        if let Err(e) = notify_disk_pressure(check.free_gb, threshold_gb) {
            eprintln!("[oclnr monitor] notification failed (non-fatal): {e}");
        }
    }
    report(check, mount);

    if cfg.modes.snapshots {
        let stamp = stamp_path(SNAPSHOT_STAMP)?;
        let now = chrono::Utc::now().timestamp();
        let decision = decide_reclaim(
            free,
            threshold_bytes,
            read_stamp(&stamp),
            now,
            cfg.snapshot_cooldown_secs,
            margin_bytes,
        );
        println!("[oclnr monitor] snapshots decision: {decision:?}");
        if let Decision::Thin { bytes } = decision {
            // Stamp before acting: a hung or failed tmutil still holds the
            // cooldown open instead of re-firing every interval.
            write_stamp(&stamp, now)?;
            std::fs::create_dir_all(&cfg.receipt_dir)?;
            let tag = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let receipt = cfg.receipt_dir.join(format!("{tag}-snapshot-thin-receipt.json"));
            let ocel = cfg.receipt_dir.join(format!("{tag}-snapshot-thin.jsonocel"));
            match snapshot::thin_and_seal(
                mount,
                bytes,
                cfg.urgency,
                Some(&receipt),
                Some(&ocel),
                false,
            ) {
                Ok(r) => println!(
                    "[oclnr monitor] pressure thin done: {} snapshot(s) removed, receipt {}",
                    r.snapshots_thinned.len(),
                    receipt.display()
                ),
                Err(e) => eprintln!("[oclnr monitor] pressure thin FAILED: {e}"),
            }
        }
    }

    if cfg.modes.builds {
        let free = volume_space(Path::new(mount))?.available;
        let stamp = stamp_path(BUILDS_STAMP)?;
        let now = chrono::Utc::now().timestamp();
        let decision = decide_reclaim(
            free,
            threshold_bytes,
            read_stamp(&stamp),
            now,
            cfg.builds_cooldown_secs,
            margin_bytes,
        );
        println!("[oclnr monitor] builds decision: {decision:?}");
        if let Decision::Thin { bytes } = decision {
            write_stamp(&stamp, now)?;
            // Budget = min(deficit + margin, configured per-run cap): never
            // delete more build output than the pressure calls for.
            let cap_gb = (bytes as f64 / BYTES_PER_GIB).min(cfg.builds_max_reclaim_gb);
            run_builds_reclaim(cap_gb, cfg.builds_ignore_recent_hours)?;
        }
    }
    Ok(())
}

/// Runs the plan-bound pipeline (`plan build` -> `plan approve` -> `delete
/// execute` -> `receipt verify`) via `autoclean run`, restricted to
/// regenerable build dirs with the live-cwd and recency exclusions.
fn run_builds_reclaim(max_reclaim_gb: f64, ignore_recent_hours: u64) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    println!(
        "[oclnr monitor] under pressure — `autoclean run --builds-only --exclude-live-cwds` \
         (cap {max_reclaim_gb:.2} GB, ignore-recent {ignore_recent_hours}h)..."
    );
    let status = Command::new(&exe)
        .current_dir(&home)
        .args(["autoclean", "run", "--yes", "--builds-only", "--exclude-live-cwds"])
        .args(["--max-reclaim-gb", &format!("{max_reclaim_gb:.3}")])
        .args(["--ignore-recent-hours", &ignore_recent_hours.to_string()])
        .env("OCLNR_AUTOCLEAN_TRIGGER", "pressure")
        .status()?;
    if status.success() {
        println!("[oclnr monitor] builds reclaim finished OK");
    } else {
        eprintln!("[oclnr monitor] builds reclaim FAILED (exit {status})");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn handle(
    threshold_gb: f64,
    mount: String,
    watch: bool,
    interval_secs: u64,
    trigger_autoclean: bool,
    cooldown_hours: u64,
    reclaim: Option<ReclaimConfig>,
    max_iterations: Option<u64>,
) -> anyhow::Result<()> {
    let mount_path: PathBuf = PathBuf::from(&mount);

    if let Some(cfg) = reclaim {
        if trigger_autoclean {
            anyhow::bail!("--reclaim and --trigger-autoclean are mutually exclusive");
        }
        println!(
            "[oclnr monitor] pressure reclaim on {} every {}s (threshold {:.1} GB, margin {:.1} GB, \
             snapshots: {}, builds: {}, receipts: {})",
            mount,
            interval_secs,
            threshold_gb,
            cfg.margin_gb,
            cfg.modes.snapshots,
            cfg.modes.builds,
            cfg.receipt_dir.display()
        );
        let mut i: u64 = 0;
        loop {
            if let Err(e) = pressure_tick(&mount, threshold_gb, &cfg) {
                eprintln!("[oclnr monitor] tick failed: {e}");
            }
            i += 1;
            if !watch || max_iterations.is_some_and(|m| i >= m) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(interval_secs));
        }
    }

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
    let mut i: u64 = 0;
    loop {
        match check_once(&mount_path) {
            Ok(()) => {}
            Err(e) => eprintln!("[oclnr monitor] check failed: {}", e),
        }
        i += 1;
        if max_iterations.is_some_and(|m| i >= m) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(interval_secs));
    }
}

/// Exposed for tests / callers that already have a `Path`.
#[allow(dead_code)]
pub fn check_once(mount: &Path, threshold_gb: f64) -> anyhow::Result<DiskPressureCheck> {
    check_and_notify(mount, threshold_gb)
}
