//! Daemon installation and management noun.
//!
//! Generates and manages a launchd LaunchAgent plist for background disk monitoring.

use std::path::PathBuf;

use clap::Subcommand;
use dialoguer::Confirm;

#[derive(Subcommand, Debug)]
pub enum DaemonAction {
    /// Install the oclnr background monitor as a launchd LaunchAgent
    Install {
        /// Disk free space threshold in GB; notify when below this
        #[arg(long, default_value = "10")]
        threshold_gb: f64,
        /// Check interval in seconds
        #[arg(long, default_value = "300")]
        interval_secs: u64,
        /// Skip the confirmation prompt and load the LaunchAgent immediately
        #[arg(long)]
        yes: bool,
    },
    /// Install `oclnr autoclean run` as a daily launchd LaunchAgent — a
    /// separate job from `install` (alert-only monitor); this one actually
    /// deletes, bounded by `autoclean`'s own safety cap and reversibility
    /// gate (see `nouns::autoclean`).
    InstallAutoclean {
        /// Hard reclaim cap in GB per run — passed through to `autoclean run`
        #[arg(long, default_value = "50")]
        max_reclaim_gb: f64,
        /// Never touch anything modified within this many hours
        #[arg(long, default_value = "24")]
        ignore_recent_hours: u64,
        /// Hour of day (0-23, local time) to run at
        #[arg(long, default_value = "4")]
        hour: u32,
        /// Minute of hour (0-59) to run at
        #[arg(long, default_value = "15")]
        minute: u32,
        /// Skip the confirmation prompt and load the LaunchAgent immediately
        #[arg(long)]
        yes: bool,
    },
    /// Uninstall the launchd LaunchAgent
    Uninstall,
    /// Uninstall the autoclean launchd LaunchAgent
    UninstallAutoclean,
    /// Show daemon status
    Status,
}

const PLIST_LABEL: &str = "com.oclnr.monitor";
const AUTOCLEAN_PLIST_LABEL: &str = "com.oclnr.autoclean";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

fn autoclean_plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", AUTOCLEAN_PLIST_LABEL))
}

/// Generates the autoclean plist. Uses `StartCalendarInterval` (a fixed
/// daily wall-clock time) rather than `StartInterval` (every N seconds since
/// boot/load) — appropriate for "once a day, off-peak" the way it isn't for
/// the monitor's frequent polling.
///
/// # Examples
///
/// ```
/// use osx_clnr::nouns::daemon::generate_autoclean_plist;
///
/// let plist = generate_autoclean_plist(50.0, 24, 4, 15);
///
/// // Positive case: the daily fixed-time trigger and the safety-cap/
/// // recency flags are all present, passed through to `autoclean run`.
/// assert!(plist.contains("StartCalendarInterval"));
/// assert!(plist.contains("<integer>4</integer>"));
/// assert!(plist.contains("<integer>15</integer>"));
/// assert!(plist.contains("--max-reclaim-gb"));
/// assert!(plist.contains("50"));
/// assert!(plist.contains("--ignore-recent-hours"));
/// assert!(plist.contains("24"));
/// assert!(plist.contains("autoclean"));
///
/// // Refusal case: never a StartInterval (that's the monitor plist's
/// // shape, not this one's) and never `--yes` baked in as a literal an
/// // editor could accidentally strip context from — it's its own array
/// // element like every other flag, not implied.
/// assert!(!plist.contains("StartInterval"));
/// assert!(plist.contains("<string>--yes</string>"));
/// ```
pub fn generate_autoclean_plist(
    max_reclaim_gb: f64,
    ignore_recent_hours: u64,
    hour: u32,
    minute: u32,
) -> String {
    let binary = oclnr_binary_path();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>autoclean</string>
        <string>run</string>
        <string>--max-reclaim-gb</string>
        <string>{max_reclaim_gb}</string>
        <string>--ignore-recent-hours</string>
        <string>{ignore_recent_hours}</string>
        <string>--yes</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{hour}</integer>
        <key>Minute</key>
        <integer>{minute}</integer>
    </dict>
    <key>RunAtLoad</key>
    <false/>
    <key>StandardOutPath</key>
    <string>/tmp/oclnr-autoclean.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/oclnr-autoclean.err</string>
</dict>
</plist>
"#,
        label = AUTOCLEAN_PLIST_LABEL,
        binary = binary,
        max_reclaim_gb = max_reclaim_gb,
        ignore_recent_hours = ignore_recent_hours,
        hour = hour,
        minute = minute
    )
}

fn oclnr_binary_path() -> String {
    which::which("oclnr")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/usr/local/bin/oclnr".to_string())
}

fn generate_plist(threshold_gb: f64, interval_secs: u64) -> String {
    let binary = oclnr_binary_path();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>monitor</string>
        <string>--threshold-gb</string>
        <string>{threshold}</string>
    </array>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/oclnr-monitor.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/oclnr-monitor.err</string>
</dict>
</plist>
"#,
        label = PLIST_LABEL,
        binary = binary,
        threshold = threshold_gb,
        interval = interval_secs
    )
}

pub fn handle(action: DaemonAction) -> anyhow::Result<()> {
    match action {
        DaemonAction::Install { threshold_gb, interval_secs, yes } => {
            let plist = plist_path();
            let dir = plist.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            let contents = generate_plist(threshold_gb, interval_secs);
            std::fs::write(&plist, &contents)?;
            println!("Wrote plist: {}", plist.display());

            // Always show exactly what was written before performing the
            // side-effecting launchctl load below.
            println!("--- plist content ---");
            println!("{}", contents);
            println!("----------------------");

            // launchctl load registers a persistent background job with
            // launchd (RunAtLoad + StartInterval) — require explicit
            // confirmation before doing that, same as other destructive/
            // system-changing actions in this CLI (see `emergency --yes`).
            let proceed = yes
                || Confirm::new()
                    .with_prompt(format!(
                        "Load LaunchAgent '{}' now via `launchctl load -w`?",
                        PLIST_LABEL
                    ))
                    .default(false)
                    .interact()
                    .unwrap_or(false);

            if !proceed {
                println!("Skipped launchctl load (pass --yes to load immediately).");
                println!("Run manually: launchctl load -w {}", plist.display());
                return Ok(());
            }

            // Load with launchctl
            let status = std::process::Command::new("launchctl")
                .args(["load", "-w", &plist.to_string_lossy()])
                .status()?;
            if status.success() {
                println!(
                    "Loaded: {} (threshold: {} GB, interval: {}s)",
                    PLIST_LABEL, threshold_gb, interval_secs
                );
            } else {
                eprintln!("Warning: launchctl load failed — plist written but daemon not started.");
                eprintln!("Run: launchctl load -w {}", plist.display());
            }
            Ok(())
        }
        DaemonAction::InstallAutoclean {
            max_reclaim_gb,
            ignore_recent_hours,
            hour,
            minute,
            yes,
        } => {
            let plist = autoclean_plist_path();
            let dir = plist.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            let contents =
                generate_autoclean_plist(max_reclaim_gb, ignore_recent_hours, hour, minute);
            std::fs::write(&plist, &contents)?;
            println!("Wrote plist: {}", plist.display());

            println!("--- plist content ---");
            println!("{}", contents);
            println!("----------------------");

            // This LaunchAgent runs `oclnr autoclean run --yes` daily,
            // unattended, with real deletion power (bounded by autoclean's
            // own safety cap and reversibility gate) — require explicit
            // confirmation before registering it with launchd, same as the
            // plain monitor install above and other system-changing actions
            // in this CLI.
            let proceed = yes
                || Confirm::new()
                    .with_prompt(format!(
                        "Load LaunchAgent '{}' now via `launchctl load -w`? This will run \
                         `oclnr autoclean run` daily at {:02}:{:02} and CAN DELETE FILES \
                         (capped at {} GB/run, never touches unknown/irreversible items).",
                        AUTOCLEAN_PLIST_LABEL, hour, minute, max_reclaim_gb
                    ))
                    .default(false)
                    .interact()
                    .unwrap_or(false);

            if !proceed {
                println!("Skipped launchctl load (pass --yes to load immediately).");
                println!("Run manually: launchctl load -w {}", plist.display());
                return Ok(());
            }

            let status = std::process::Command::new("launchctl")
                .args(["load", "-w", &plist.to_string_lossy()])
                .status()?;
            if status.success() {
                println!(
                    "Loaded: {} (daily at {:02}:{:02}, cap: {} GB, ignore-recent: {}h)",
                    AUTOCLEAN_PLIST_LABEL, hour, minute, max_reclaim_gb, ignore_recent_hours
                );
            } else {
                eprintln!("Warning: launchctl load failed — plist written but daemon not started.");
                eprintln!("Run: launchctl load -w {}", plist.display());
            }
            Ok(())
        }
        DaemonAction::UninstallAutoclean => {
            let plist = autoclean_plist_path();
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", "-w", &plist.to_string_lossy()])
                    .status();
                std::fs::remove_file(&plist)?;
                println!("Uninstalled {}", AUTOCLEAN_PLIST_LABEL);
            } else {
                println!("Autoclean daemon not installed (plist not found: {})", plist.display());
            }
            Ok(())
        }
        DaemonAction::Uninstall => {
            let plist = plist_path();
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", "-w", &plist.to_string_lossy()])
                    .status();
                std::fs::remove_file(&plist)?;
                println!("Uninstalled {}", PLIST_LABEL);
            } else {
                println!("Daemon not installed (plist not found: {})", plist.display());
            }
            Ok(())
        }
        DaemonAction::Status => {
            let plist = plist_path();
            if !plist.exists() {
                println!("Monitor daemon ({}): not installed.", PLIST_LABEL);
            } else {
                println!("Monitor plist: {} (exists)", plist.display());
                let output =
                    std::process::Command::new("launchctl").args(["list", PLIST_LABEL]).output()?;
                if output.status.success() {
                    println!("Monitor status: running");
                    println!("{}", String::from_utf8_lossy(&output.stdout));
                } else {
                    println!("Monitor status: not loaded (plist exists but daemon not running)");
                    println!("Run: launchctl load -w {}", plist.display());
                }
            }

            println!();
            let autoclean_plist = autoclean_plist_path();
            if !autoclean_plist.exists() {
                println!("Autoclean daemon ({}): not installed.", AUTOCLEAN_PLIST_LABEL);
            } else {
                println!("Autoclean plist: {} (exists)", autoclean_plist.display());
                let output = std::process::Command::new("launchctl")
                    .args(["list", AUTOCLEAN_PLIST_LABEL])
                    .output()?;
                if output.status.success() {
                    println!("Autoclean status: loaded");
                    println!("{}", String::from_utf8_lossy(&output.stdout));
                } else {
                    println!("Autoclean status: not loaded (plist exists but daemon not running)");
                    println!("Run: launchctl load -w {}", autoclean_plist.display());
                }
                let log = dirs::home_dir()
                    .map(|h| h.join("Library/Logs/oclnr/autoclean.log"))
                    .filter(|p| p.exists());
                match log {
                    Some(p) => println!("Autoclean log: {}", p.display()),
                    None => println!("Autoclean log: none yet (no run has completed)"),
                }
            }
            Ok(())
        }
    }
}
