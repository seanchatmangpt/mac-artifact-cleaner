//! CLI Nouns module registering subcommands.

pub mod artifact;
pub mod audit;
pub mod backup;
pub mod brew;
pub mod completion;
pub mod daemon;
pub mod delete;
pub mod dev;
pub mod docker;
pub mod doctor;
pub mod emergency;
pub mod exclusion;
pub mod github;
pub mod monitor;
pub mod ocel;
pub mod plan;
pub mod privacy;
pub mod receipt;
pub mod snapshot;
pub mod tool_roots;
pub mod tools;
pub mod wizard;
pub mod wpm;
pub mod wpm_use_cases;
pub mod xcode;

use crate::domain::policy::OclnrPolicy;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub static POLICY: OnceLock<OclnrPolicy> = OnceLock::new();

#[derive(Parser, Debug)]
#[command(
    name = "oclnr",
    about = "Pentecost: macOS developer disk auditor and cleanup utility"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Optional path to a custom OCLNR.yaml policy file
    #[arg(long)]
    pub policy: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    // ... (rest of commands)
    /// Interactive maintenance wizard (Audit -> Plan -> Delete -> Thin)
    #[command(alias = "clean")]
    Wizard,
    /// Emergency local cleanup for the current directory
    Dev {
        /// Optional specific path to clean instead of current directory
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
    /// Generate shell completions
    Completion {
        #[command(subcommand)]
        action: completion::CompletionAction,
    },
    /// Audit actions
    Audit {
        #[command(subcommand)]
        action: audit::AuditAction,
    },
    /// Artifact actions
    Artifact {
        #[command(subcommand)]
        action: artifact::ArtifactAction,
    },
    /// Plan actions
    Plan {
        #[command(subcommand)]
        action: plan::PlanAction,
    },
    /// Delete actions
    Delete {
        #[command(subcommand)]
        action: delete::DeleteAction,
    },
    /// Receipt actions
    Receipt {
        #[command(subcommand)]
        action: receipt::ReceiptAction,
    },
    /// Doctor diagnostics
    Doctor {
        #[command(subcommand)]
        action: doctor::DoctorAction,
    },
    /// Snapshot actions
    Snapshot {
        #[command(subcommand)]
        action: snapshot::SnapshotAction,
    },
    /// Emergency low-disk reclaim (works when the volume is critically full)
    Emergency {
        /// Volume mount point (defaults to "/")
        #[arg(long, default_value = "/")]
        mount: String,
        /// Actually delete; default is a dry-run preview
        #[arg(long)]
        yes: bool,
        /// Optional receipt path (written only if space allows afterward)
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
    /// Exclusion actions
    Exclusion {
        #[command(subcommand)]
        action: exclusion::ExclusionAction,
    },
    /// Tool roots actions
    #[command(name = "tool-roots")]
    ToolRoots {
        #[command(subcommand)]
        action: tool_roots::ToolRootsAction,
    },
    /// OCEL v2 actions
    Ocel {
        #[command(subcommand)]
        action: ocel::OcelAction,
    },
    /// Privacy actions
    Privacy {
        #[command(subcommand)]
        action: privacy::PrivacyAction,
    },
    /// wasm4pm process mining capabilities (Pillars 1-4)
    Wpm {
        #[command(subcommand)]
        action: wpm::WpmAction,
    },
    /// GitHub wrapper actions
    Github {
        #[command(subcommand)]
        action: github::GithubAction,
    },
    /// Docker container runtime scan
    Docker {
        #[command(subcommand)]
        action: docker::DockerAction,
    },
    /// Homebrew package manager scan
    Brew {
        #[command(subcommand)]
        action: brew::BrewAction,
    },
    /// Background monitor daemon management
    Daemon {
        #[command(subcommand)]
        action: daemon::DaemonAction,
    },
    /// Xcode simulator and DerivedData scan
    Xcode {
        #[command(subcommand)]
        action: xcode::XcodeAction,
    },
    /// iOS device backup scan
    Backup {
        #[command(subcommand)]
        action: backup::BackupAction,
    },
    /// Developer toolchain and package manager scans
    Tools {
        #[command(subcommand)]
        action: tools::ToolsAction,
    },
    /// Check disk free space against a threshold and notify if under pressure.
    ///
    /// This is the command the `daemon install` launchd plist invokes on its
    /// `StartInterval`. By default it performs a single check-and-exit; pass
    /// `--watch` to poll continuously instead (for standalone, non-launchd use).
    Monitor {
        /// Disk free space threshold in GB; notify when below this
        #[arg(long)]
        threshold_gb: f64,
        /// Volume mount point to check
        #[arg(long, default_value = "/")]
        mount: String,
        /// Poll continuously instead of checking once and exiting
        #[arg(long)]
        watch: bool,
        /// Poll interval in seconds, only used with --watch
        #[arg(long, default_value = "300")]
        interval_secs: u64,
    },
}

/// Returns the default list of roots to scan for developer artifacts.
/// Includes the user's home directory and the system temporary directory.
pub fn default_scan_roots() -> anyhow::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();

    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    } else {
        anyhow::bail!("Home directory not found");
    }

    // Always include /tmp for developer build artifacts and lock files
    roots.push(PathBuf::from("/tmp"));

    Ok(roots)
}

pub fn handle_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let policy = if let Some(path) = cli.policy {
        OclnrPolicy::load_from_file(&path)?
    } else {
        let default_path = Path::new("osxclnr.toml");
        if default_path.exists() {
            OclnrPolicy::load_from_file(default_path)?
        } else {
            OclnrPolicy::default()
        }
    };

    POLICY
        .set(policy)
        .map_err(|_| anyhow::anyhow!("Failed to set global policy"))?;

    match cli.command {
        Command::Wizard => wizard::handle(),
        Command::Dev { path } => dev::handle(path),
        Command::Completion { action } => completion::handle(action),
        Command::Audit { action } => audit::handle(action),
        Command::Artifact { action } => artifact::handle(action),
        Command::Plan { action } => plan::handle(action),
        Command::Delete { action } => delete::handle(action),
        Command::Receipt { action } => receipt::handle(action),
        Command::Doctor { action } => doctor::handle(action),
        Command::Snapshot { action } => snapshot::handle(action),
        Command::Emergency {
            mount,
            yes,
            receipt,
        } => emergency::handle(mount, yes, receipt),
        Command::Exclusion { action } => exclusion::handle(action),
        Command::ToolRoots { action } => tool_roots::handle(action),
        Command::Ocel { action } => ocel::handle(action),
        Command::Privacy { action } => privacy::handle(action),
        Command::Wpm { action } => wpm::handle(action),
        Command::Github { action } => github::handle(action),
        Command::Docker { action } => docker::handle(action),
        Command::Brew { action } => brew::handle(action),
        Command::Daemon { action } => daemon::handle(action),
        Command::Xcode { action } => xcode::handle(action),
        Command::Backup { action } => backup::handle(action),
        Command::Tools { action } => tools::handle(action),
        Command::Monitor {
            threshold_gb,
            mount,
            watch,
            interval_secs,
        } => monitor::handle(threshold_gb, mount, watch, interval_secs),
    }
}
