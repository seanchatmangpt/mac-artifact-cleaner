//! CLI Nouns module registering subcommands.

pub mod artifact;
pub mod audit;
pub mod completion;
pub mod delete;
pub mod doctor;
pub mod exclusion;
pub mod ocel;
pub mod plan;
pub mod privacy;
pub mod receipt;
pub mod snapshot;
pub mod tool_roots;
pub mod wizard;
pub mod wpm;
pub mod wpm_use_cases;

use clap::{Parser, Subcommand};
use std::path::{PathBuf, Path};
use std::sync::OnceLock;
use crate::domain::policy::OclnrPolicy;

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
    }
}

pub fn handle_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    let policy = if let Some(path) = cli.policy {
        OclnrPolicy::load_from_file(&path)?
    } else {
        let default_path = Path::new("OCLNR.toml");
        if default_path.exists() {
            OclnrPolicy::load_from_file(default_path)?
        } else {
            OclnrPolicy::default()
        }
    };
    
    POLICY.set(policy).map_err(|_| anyhow::anyhow!("Failed to set global policy"))?;
    
    match cli.command {
        Command::Wizard => wizard::handle(),
        Command::Completion { action } => completion::handle(action),
        Command::Audit { action } => audit::handle(action),
        Command::Artifact { action } => artifact::handle(action),
        Command::Plan { action } => plan::handle(action),
        Command::Delete { action } => delete::handle(action),
        Command::Receipt { action } => receipt::handle(action),
        Command::Doctor { action } => doctor::handle(action),
        Command::Snapshot { action } => snapshot::handle(action),
        Command::Exclusion { action } => exclusion::handle(action),
        Command::ToolRoots { action } => tool_roots::handle(action),
        Command::Ocel { action } => ocel::handle(action),
        Command::Privacy { action } => privacy::handle(action),
        Command::Wpm { action } => wpm::handle(action),
    }
}
