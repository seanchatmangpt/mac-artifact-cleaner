//! CLI Nouns module registering subcommands.

pub mod artifact;
pub mod audit;
pub mod delete;
pub mod doctor;
pub mod exclusion;
pub mod ocel;
pub mod plan;
pub mod privacy;
pub mod receipt;
pub mod snapshot;
pub mod tool_roots;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "oclnr",
    about = "Pentecost: macOS developer disk auditor and cleanup utility"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
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
}

pub fn handle_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
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
    }
}
