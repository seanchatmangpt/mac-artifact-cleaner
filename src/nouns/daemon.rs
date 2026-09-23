//! Daemon installation and management noun.
//!
//! Generates and manages a launchd LaunchAgent plist for background disk monitoring.

use std::path::PathBuf;

use clap::Subcommand;
use dialoguer::Confirm;

use crate::nouns::autoclean;

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
        /// When under pressure, run the same capped, receipted `autoclean
        /// run --yes` pipeline on demand (cooldown-bounded). Off by default:
        /// the plain monitor only ever notifies.
        #[arg(long)]
        trigger_autoclean: bool,
        /// Minimum hours between pressure-triggered autoclean runs
        #[arg(long, default_value = "6")]
        autoclean_cooldown_hours: u64,
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
    /// Install `oclnr monitor --watch --reclaim ...` as a long-running
    /// launchd LaunchAgent (`com.oclnr.pressure`): samples free space every
    /// `--interval-secs` and, below `--threshold-gb`, thins local snapshots
    /// (and optionally reclaims build dirs) through the receipted paths.
    InstallPressureMonitor {
        /// Free-space threshold in GB that triggers reclaim
        #[arg(long)]
        threshold_gb: f64,
        /// Sample interval in seconds
        #[arg(long, default_value = "60")]
        interval_secs: u64,
        /// Reclaim strategies: `snapshots` or `snapshots,builds`
        #[arg(long, default_value = "snapshots", value_parser = parse_reclaim_value)]
        reclaim: String,
        /// Skip the confirmation prompt and load the LaunchAgent immediately
        #[arg(long)]
        yes: bool,
    },
    /// Uninstall the pressure-monitor launchd LaunchAgent
    UninstallPressureMonitor,
    /// Uninstall the launchd LaunchAgent
    Uninstall,
    /// Uninstall the autoclean launchd LaunchAgent
    UninstallAutoclean,
    /// Show daemon status
    Status,
}

const PLIST_LABEL: &str = "com.oclnr.monitor";
const AUTOCLEAN_PLIST_LABEL: &str = "com.oclnr.autoclean";
/// launchd label of the pressure-reclaim monitor.
pub const PRESSURE_PLIST_LABEL: &str = "com.oclnr.pressure";

/// Validates `--reclaim` through the same domain parser `monitor` uses, then
/// keeps the canonical string for the plist.
fn parse_reclaim_value(value: &str) -> Result<String, String> {
    let modes = crate::domain::pressure::parse_reclaim_modes(value)?;
    Ok(match (modes.snapshots, modes.builds) {
        (true, true) => "snapshots,builds",
        (true, false) => "snapshots",
        _ => "builds",
    }
    .to_string())
}

pub(crate) fn pressure_plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PRESSURE_PLIST_LABEL))
}

/// Generates the pressure-monitor plist. The job is a single long-running
/// `monitor --watch` process (`KeepAlive`, `RunAtLoad`) rather than a
/// `StartInterval` re-fire, so the in-loop cooldown and the persisted
/// last-action stamps both apply; launchd restarts it if it exits.
///
/// # Examples
///
/// ```
/// use osx_clnr::nouns::daemon::generate_pressure_plist;
/// use std::path::Path;
///
/// let plist = generate_pressure_plist(
///     "/usr/local/bin/oclnr", 20.0, 60, "snapshots,builds", Path::new("/Users/me/Library/Logs/oclnr"),
/// );
///
/// // Positive: label, watch loop, threshold, interval, strategies, keep-alive.
/// assert!(plist.contains("<string>com.oclnr.pressure</string>"));
/// assert!(plist.contains("<string>monitor</string>"));
/// assert!(plist.contains("<string>--watch</string>"));
/// assert!(plist.contains("<string>--threshold-gb</string>\n        <string>20</string>"));
/// assert!(plist.contains("<string>--interval-secs</string>\n        <string>60</string>"));
/// assert!(plist.contains("<string>--reclaim</string>\n        <string>snapshots,builds</string>"));
/// assert!(plist.contains("<key>KeepAlive</key>"));
/// assert!(plist.contains("/Users/me/Library/Logs/oclnr/pressure-launchd.log"));
///
/// // Negative: it is not the re-fired monitor shape.
/// assert!(!plist.contains("StartInterval"));
///
/// // Refusal: never bounded by --max-iterations (a bounded loop under
/// // KeepAlive would just restart-spin).
/// assert!(!plist.contains("--max-iterations"));
/// ```
pub fn generate_pressure_plist(
    binary: &str,
    threshold_gb: f64,
    interval_secs: u64,
    reclaim: &str,
    log_dir: &std::path::Path,
) -> String {
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
        <string>--watch</string>
        <string>--threshold-gb</string>
        <string>{threshold_gb}</string>
        <string>--interval-secs</string>
        <string>{interval_secs}</string>
        <string>--reclaim</string>
        <string>{reclaim}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ThrottleInterval</key>
    <integer>60</integer>
    <key>StandardOutPath</key>
    <string>{log_dir}/pressure-launchd.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/pressure-launchd.err</string>
</dict>
</plist>
"#,
        label = PRESSURE_PLIST_LABEL,
        log_dir = log_dir.display(),
    )
}

/// Writes a plist to `path`, creating its parent directory. Split from the
/// `launchctl load` step so it can be exercised against a tempdir.
pub fn write_plist(path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    ensure_plist_dir(path)?;
    std::fs::write(path, contents)?;
    Ok(())
}

/// Durable per-user log directory for the launchd jobs' stdout/stderr.
/// `/tmp` (the previous location) is wiped on reboot and has silently
/// swallowed unattended-job output.
pub(crate) fn launchd_log_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("Library/Logs/oclnr")
}

pub(crate) fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

pub(crate) fn autoclean_plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", AUTOCLEAN_PLIST_LABEL))
}

/// Creates the parent directory of `plist` (e.g. `~/Library/LaunchAgents`),
/// used by both the `Install` and `InstallAutoclean` branches. Returns a
/// real error instead of unwrapping if `plist` has no parent.
fn ensure_plist_dir(plist: &std::path::Path) -> anyhow::Result<()> {
    let dir = plist
        .parent()
        .ok_or_else(|| anyhow::anyhow!("plist path has no parent: {}", plist.display()))?;
    std::fs::create_dir_all(dir)?;
    Ok(())
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
///
/// // Job output goes to the durable per-user log directory, never /tmp
/// // (wiped on reboot, so a failed run's stderr would vanish).
/// assert!(plist.contains("Library/Logs/oclnr/autoclean-launchd.log"));
/// assert!(!plist.contains("/tmp/oclnr"));
/// ```
pub fn generate_autoclean_plist(
    max_reclaim_gb: f64,
    ignore_recent_hours: u64,
    hour: u32,
    minute: u32,
) -> String {
    generate_autoclean_plist_for(
        &oclnr_binary_path(),
        max_reclaim_gb,
        ignore_recent_hours,
        hour,
        minute,
    )
}

/// [`generate_autoclean_plist`] for an explicit binary path (used by
/// preflight tests against the binary under test).
pub fn generate_autoclean_plist_for(
    binary: &str,
    max_reclaim_gb: f64,
    ignore_recent_hours: u64,
    hour: u32,
    minute: u32,
) -> String {
    let log_dir = launchd_log_dir();
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
    <string>{log_dir}/autoclean-launchd.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/autoclean-launchd.err</string>
</dict>
</plist>
"#,
        label = AUTOCLEAN_PLIST_LABEL,
        binary = binary,
        log_dir = log_dir.display(),
        max_reclaim_gb = max_reclaim_gb,
        ignore_recent_hours = ignore_recent_hours,
        hour = hour,
        minute = minute
    )
}

/// The binary launchd agents run: the oclnr-owned copy at
/// `~/.oclnr/bin/oclnr` that `daemon install-*` refreshes from the running
/// executable — never whatever `which oclnr` happens to resolve (a stale
/// `~/.local/bin/oclnr` crash-looped `com.oclnr.pressure` on 2026-09-22).
fn oclnr_binary_path() -> String {
    crate::integration::daemon_binary::installed_binary_path().to_string_lossy().to_string()
}

/// `daemon status` check: would the installed agent's binary accept the
/// command line its plist passes? Catches the stale-binary crash loop
/// (`unexpected argument '--reclaim'`) without reading launchd logs.
fn report_preflight(name: &str, plist: &std::path::Path) {
    let Ok(contents) = std::fs::read_to_string(plist) else { return };
    match crate::integration::daemon_binary::preflight_plist(&contents) {
        Ok(missing) if missing.is_empty() => {
            println!("{name} preflight: binary accepts the plist's command line");
        }
        Ok(missing) => eprintln!(
            "⚠ {name} preflight FAILED: binary does not accept {} — the job crash-loops on \
             every fire. Reinstall with the matching `oclnr daemon install-*`.",
            missing.join(", ")
        ),
        Err(e) => eprintln!("⚠ {name} preflight FAILED: {e}"),
    }
}

/// Installs the running binary for launchd and refuses when it would not
/// accept the exact command line in `plist_contents`.
fn install_and_preflight(where_: &str, plist_contents: &str) -> anyhow::Result<()> {
    let installed = crate::integration::daemon_binary::install_self()?;
    println!("Installed agent binary: {}", installed.display());
    let missing = crate::integration::daemon_binary::preflight_plist(plist_contents)
        .map_err(|e| anyhow::anyhow!("{where_}: preflight failed, refusing to install: {e}"))?;
    if !missing.is_empty() {
        anyhow::bail!(
            "{where_}: refusing to install — {} does not accept {} (the job would \
             crash-loop under launchd)",
            installed.display(),
            missing.join(", ")
        );
    }
    Ok(())
}

fn generate_plist(
    threshold_gb: f64,
    interval_secs: u64,
    trigger_autoclean: bool,
    autoclean_cooldown_hours: u64,
) -> String {
    generate_monitor_plist_for(
        &oclnr_binary_path(),
        threshold_gb,
        interval_secs,
        trigger_autoclean,
        autoclean_cooldown_hours,
    )
}

/// The alert-only monitor plist for an explicit binary path.
pub fn generate_monitor_plist_for(
    binary: &str,
    threshold_gb: f64,
    interval_secs: u64,
    trigger_autoclean: bool,
    autoclean_cooldown_hours: u64,
) -> String {
    let log_dir = launchd_log_dir();
    let trigger_args = if trigger_autoclean {
        format!(
            r#"        <string>--trigger-autoclean</string>
        <string>--autoclean-cooldown-hours</string>
        <string>{autoclean_cooldown_hours}</string>
"#
        )
    } else {
        String::new()
    };
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
{trigger_args}    </array>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_dir}/monitor-launchd.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/monitor-launchd.err</string>
</dict>
</plist>
"#,
        label = PLIST_LABEL,
        binary = binary,
        threshold = threshold_gb,
        interval = interval_secs,
        trigger_args = trigger_args,
        log_dir = log_dir.display()
    )
}

/// Extracts the binary path (first `<string>` under `ProgramArguments`) from
/// a written plist, so `daemon status` can detect the silently-dead-job case:
/// the plist bakes an absolute path at install time, and if the binary later
/// moves or is deleted the job fails on every fire with nothing but a line
/// in `/tmp/oclnr-*.err`.
pub(crate) fn plist_program_arguments_binary(plist: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(plist).ok()?;
    let idx = content.find("<key>ProgramArguments</key>")?;
    let rest = &content[idx..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")? + start;
    Some(rest[start..end].to_string())
}

pub fn handle(action: DaemonAction) -> anyhow::Result<()> {
    match action {
        DaemonAction::Install {
            threshold_gb,
            interval_secs,
            trigger_autoclean,
            autoclean_cooldown_hours,
            yes,
        } => {
            let plist = plist_path();
            ensure_plist_dir(&plist)?;
            std::fs::create_dir_all(launchd_log_dir())?;
            let contents = generate_plist(
                threshold_gb,
                interval_secs,
                trigger_autoclean,
                autoclean_cooldown_hours,
            );
            install_and_preflight("daemon install", &contents)?;
            std::fs::write(&plist, &contents)?;
            println!("Wrote plist: {}", plist.display());

            // Always show exactly what was written before performing the
            // side-effecting launchctl load below.
            println!("--- plist content ---");
            println!("{}", contents);
            println!("----------------------");

            // launchd load registers a persistent background job with
            // launchd (RunAtLoad + StartInterval) — require explicit
            // confirmation before doing that, same as other destructive/
            // system-changing actions in this CLI (see `emergency --yes`).
            let proceed = yes
                || Confirm::new()
                    .with_prompt(format!(
                        "Load LaunchAgent '{}' now via `launchctl load -w`?{}",
                        PLIST_LABEL,
                        if trigger_autoclean {
                            " This monitor CAN TRIGGER the capped autoclean pipeline under \
                             disk pressure (cooldown-bounded)."
                        } else {
                            ""
                        }
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
            match crate::integration::daemon_binary::reload_agent(PLIST_LABEL, &plist) {
                Ok(program) => {
                    println!(
                        "Loaded: {} (threshold: {} GB, interval: {}s, autoclean trigger: {})",
                        PLIST_LABEL,
                        threshold_gb,
                        interval_secs,
                        if trigger_autoclean {
                            format!("on (cooldown {autoclean_cooldown_hours}h)")
                        } else {
                            "off".to_string()
                        }
                    );
                    println!("Verified running program: {program}");
                }
                Err(e) => {
                    anyhow::bail!(
                        "plist written to {} but the agent was NOT (re)loaded: {e}",
                        plist.display()
                    );
                }
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
            ensure_plist_dir(&plist)?;
            std::fs::create_dir_all(launchd_log_dir())?;
            let contents =
                generate_autoclean_plist(max_reclaim_gb, ignore_recent_hours, hour, minute);
            install_and_preflight("daemon install-autoclean", &contents)?;
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

            match crate::integration::daemon_binary::reload_agent(AUTOCLEAN_PLIST_LABEL, &plist) {
                Ok(program) => {
                    println!(
                        "Loaded: {} (daily at {:02}:{:02}, cap: {} GB, ignore-recent: {}h)",
                        AUTOCLEAN_PLIST_LABEL, hour, minute, max_reclaim_gb, ignore_recent_hours
                    );
                    println!("Verified running program: {program}");
                }
                Err(e) => {
                    anyhow::bail!(
                        "plist written to {} but the agent was NOT (re)loaded: {e}",
                        plist.display()
                    );
                }
            }
            Ok(())
        }
        DaemonAction::InstallPressureMonitor { threshold_gb, interval_secs, reclaim, yes } => {
            let plist = pressure_plist_path();
            let log_dir = launchd_log_dir();
            std::fs::create_dir_all(&log_dir)?;
            let binary = oclnr_binary_path();
            let contents =
                generate_pressure_plist(&binary, threshold_gb, interval_secs, &reclaim, &log_dir);
            install_and_preflight("daemon install-pressure-monitor", &contents)?;
            write_plist(&plist, &contents)?;
            println!("Wrote plist: {}", plist.display());

            println!("--- plist content ---");
            println!("{}", contents);
            println!("----------------------");

            // This agent thins local snapshots (and with `builds`, deletes
            // build dirs through the plan-bound pipeline) unattended —
            // explicit confirmation before registering it, same as the
            // other installers.
            let proceed = yes
                || Confirm::new()
                    .with_prompt(format!(
                        "Load LaunchAgent '{}' now via `launchctl load -w`? Below {} GB free it \
                         WILL thin local snapshots{} (cooldown-bounded, receipted).",
                        PRESSURE_PLIST_LABEL,
                        threshold_gb,
                        if reclaim.contains("builds") {
                            " and delete regenerable build dirs"
                        } else {
                            ""
                        }
                    ))
                    .default(false)
                    .interact()
                    .unwrap_or(false);

            if !proceed {
                println!("Skipped launchctl load (pass --yes to load immediately).");
                println!("Run manually: launchctl load -w {}", plist.display());
                return Ok(());
            }

            match crate::integration::daemon_binary::reload_agent(PRESSURE_PLIST_LABEL, &plist) {
                Ok(program) => {
                    println!(
                        "Loaded: {} (threshold: {} GB, interval: {}s, reclaim: {})",
                        PRESSURE_PLIST_LABEL, threshold_gb, interval_secs, reclaim
                    );
                    println!("Verified running program: {program}");
                }
                Err(e) => {
                    anyhow::bail!(
                        "plist written to {} but the agent was NOT (re)loaded: {e}",
                        plist.display()
                    );
                }
            }
            Ok(())
        }
        DaemonAction::UninstallPressureMonitor => {
            let plist = pressure_plist_path();
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", "-w", &plist.to_string_lossy()])
                    .status();
                std::fs::remove_file(&plist)?;
                println!("Uninstalled {}", PRESSURE_PLIST_LABEL);
            } else {
                println!("Pressure monitor not installed (plist not found: {})", plist.display());
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
                match plist_program_arguments_binary(&plist) {
                    Some(binary) => {
                        if std::path::Path::new(&binary).exists() {
                            println!("Monitor binary: {binary} (exists)");
                        } else {
                            eprintln!(
                                "⚠ Monitor binary '{binary}' does NOT exist — the job is \
                                 silently dead on every fire. Reinstall with \
                                 `oclnr daemon install` after placing the binary."
                            );
                        }
                    }
                    None => println!("Monitor binary: (unparsable plist)"),
                }
                report_preflight("Monitor", &plist);
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
                match plist_program_arguments_binary(&autoclean_plist) {
                    Some(binary) => {
                        if std::path::Path::new(&binary).exists() {
                            println!("Autoclean binary: {binary} (exists)");
                        } else {
                            eprintln!(
                                "⚠ Autoclean binary '{binary}' does NOT exist — the daily \
                                 cleanup is silently dead. Reinstall with \
                                 `oclnr daemon install-autoclean` after placing the binary."
                            );
                        }
                    }
                    None => println!("Autoclean binary: (unparsable plist)"),
                }
                report_preflight("Autoclean", &autoclean_plist);
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

            println!();
            let pressure_plist = pressure_plist_path();
            if !pressure_plist.exists() {
                println!("Pressure monitor ({}): not installed.", PRESSURE_PLIST_LABEL);
            } else {
                println!("Pressure plist: {} (exists)", pressure_plist.display());
                if let Some(binary) = plist_program_arguments_binary(&pressure_plist) {
                    println!("Pressure binary: {binary}");
                }
                report_preflight("Pressure", &pressure_plist);
                let output = std::process::Command::new("launchctl")
                    .args(["list", PRESSURE_PLIST_LABEL])
                    .output()?;
                if output.status.success() {
                    println!("Pressure monitor status: loaded");
                } else {
                    println!("Pressure monitor status: not loaded");
                    println!("Run: launchctl load -w {}", pressure_plist.display());
                }
            }

            // Last-run standing, same source `autoclean status` uses — a
            // daemon that is "loaded" but has failed its last three runs is
            // not healthy, and this is where that difference becomes visible.
            println!();
            match autoclean::read_last_trigger_unix() {
                Some(t) => println!(
                    "Last pressure-trigger: {}",
                    chrono::DateTime::from_timestamp(t, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| t.to_string())
                ),
                None => println!("Last pressure-trigger: never"),
            }
            let log = dirs::home_dir().map(|h| h.join("Library/Logs/oclnr/autoclean.log"));
            if let Some(log) = log.filter(|p| p.exists()) {
                if let Ok(content) = std::fs::read_to_string(&log) {
                    let lines: Vec<&str> = content.lines().collect();
                    let now = chrono::Utc::now().timestamp();
                    let s = autoclean::summarize_log(&lines, now);
                    if s.runs_total > 0 {
                        println!(
                            "Autoclean standing: {} run(s) ({} in last 7d), last: {}",
                            s.runs_total,
                            s.runs_last_7d,
                            s.last_outcome.map(|o| o.to_string()).unwrap_or_else(|| "?".into())
                        );
                        if let Some(freed) = &s.last_freed {
                            println!("  last measured reclaim: {freed}");
                        }
                        if s.consecutive_failures >= 2 {
                            eprintln!(
                                "⚠ {} consecutive autoclean failures — inspect with \
                                 `oclnr autoclean status`",
                                s.consecutive_failures
                            );
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the pressure plist into a real tempdir (never
    /// `~/Library/LaunchAgents`, never `launchctl load`) and checks it is a
    /// well-formed property list via the system `plutil -lint`.
    #[test]
    fn pressure_plist_written_to_tempdir_lints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("LaunchAgents").join(format!("{PRESSURE_PLIST_LABEL}.plist"));
        let contents =
            generate_pressure_plist("/usr/local/bin/oclnr", 15.5, 30, "snapshots", dir.path());
        write_plist(&path, &contents).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);

        match std::process::Command::new("plutil").arg("-lint").arg(&path).output() {
            Ok(out) => assert!(
                out.status.success(),
                "plutil -lint failed: {}",
                String::from_utf8_lossy(&out.stdout)
            ),
            Err(_) => eprintln!("SKIP: plutil not available"),
        }
    }

    #[test]
    fn reclaim_value_is_validated_and_canonicalized() {
        assert_eq!(parse_reclaim_value("builds,snapshots").unwrap(), "snapshots,builds");
        assert_eq!(parse_reclaim_value("snapshots").unwrap(), "snapshots");
        assert!(parse_reclaim_value("docker").is_err());
    }
}
