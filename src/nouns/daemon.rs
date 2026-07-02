//! Daemon installation and management noun.
//!
//! Generates and manages a launchd LaunchAgent plist for background disk monitoring.

use clap::Subcommand;
use dialoguer::Confirm;
use std::path::PathBuf;

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
    /// Uninstall the launchd LaunchAgent
    Uninstall,
    /// Show daemon status
    Status,
}

const PLIST_LABEL: &str = "com.oclnr.monitor";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
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
        DaemonAction::Install {
            threshold_gb,
            interval_secs,
            yes,
        } => {
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
        DaemonAction::Uninstall => {
            let plist = plist_path();
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", "-w", &plist.to_string_lossy()])
                    .status();
                std::fs::remove_file(&plist)?;
                println!("Uninstalled {}", PLIST_LABEL);
            } else {
                println!(
                    "Daemon not installed (plist not found: {})",
                    plist.display()
                );
            }
            Ok(())
        }
        DaemonAction::Status => {
            let plist = plist_path();
            if !plist.exists() {
                println!("Daemon not installed.");
                return Ok(());
            }
            println!("Plist: {} (exists)", plist.display());
            let output = std::process::Command::new("launchctl")
                .args(["list", PLIST_LABEL])
                .output()?;
            if output.status.success() {
                println!("Status: running");
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                println!("Status: not loaded (plist exists but daemon not running)");
                println!("Run: launchctl load -w {}", plist.display());
            }
            Ok(())
        }
    }
}
