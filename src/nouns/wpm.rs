//! wasm4pm CLI integration noun.
//!
//! This module acts as a bridge between osx-clnr's OCEL generation and Dr. van der Aalst's
//! wasm4pm Process Mining engine, unlocking 25+ process intelligence use cases.

use clap::Subcommand;
use std::path::PathBuf;
use std::process::Command;

#[derive(Subcommand, Debug)]
pub enum WpmAction {
    /// Pillar 1: Process Discovery (The Bloat Cascade, Orphaned Toolchains)
    Discover {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Pillar 2: Conformance Checking (Gall Pipeline Verification, Adversarial Audit)
    Audit {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Pillar 3: Lean Six Sigma & Process Efficiency (Downtime Waste, Artifact Rework)
    Lean {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Pillar 3: Andon Oracle (Disk Spikes Alerting, Impossible Prefix Conformance)
    Oracle {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Pillar 4: Predictive Monitoring - Statistical Process Control (Cache SPC Bounds)
    Spc {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Pillar 4: Autonomic Optimization (RL Agent Disk Cleanup)
    Autoprocess {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Print instructions and queries for the 25 process mining use cases.
    UseCase {
        #[command(subcommand)]
        action: crate::nouns::wpm_use_cases::UseCaseAction,
    },
}

fn find_wpm_binary() -> anyhow::Result<String> {
    // 1. Check PATH
    if let Ok(path) = which::which("wpm") {
        return Ok(path.to_string_lossy().to_string());
    }
    
    // 2. Check local wasm4pm repo target/release
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?;
    let release_path = home.join("wasm4pm/target/release/wpm");
    if release_path.exists() {
        return Ok(release_path.to_string_lossy().to_string());
    }

    // 3. Check local wasm4pm repo target/debug
    let debug_path = home.join("wasm4pm/target/debug/wpm");
    if debug_path.exists() {
        return Ok(debug_path.to_string_lossy().to_string());
    }

    anyhow::bail!("Could not find 'wpm' binary in PATH or ~/wasm4pm/target/... Please install wasm4pm.")
}

fn run_wpm_cmd(args: &[&str]) -> anyhow::Result<()> {
    let wpm_bin = find_wpm_binary()?;
    println!("Executing: {} {}", wpm_bin, args.join(" "));
    
    let status = Command::new(wpm_bin)
        .args(args)
        .status()?;

    if !status.success() {
        anyhow::bail!("wpm command failed with status: {}", status);
    }
    Ok(())
}

pub fn handle(action: WpmAction) -> anyhow::Result<()> {
    match action {
        WpmAction::Discover { log } => {
            println!("🚀 Initiating Process Discovery via Object-Centric Petri Nets (OCPN)...");
            run_wpm_cmd(&["mining", "discover", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::Audit { log } => {
            println!("🛡️ Running Alignment-Based Conformance Checking and Adversarial Audit...");
            run_wpm_cmd(&["audit", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::Lean { log } => {
            println!("🏭 Analyzing Lean Six Sigma Process Waste and Efficiency...");
            run_wpm_cmd(&["lean", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::Oracle { log } => {
            println!("🔮 Consulting the Andon Oracle for Impossible Prefix Detection...");
            run_wpm_cmd(&["oracle", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::Spc { log } => {
            println!("📊 Calculating Statistical Process Control (SPC) Bounds...");
            run_wpm_cmd(&["spc", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::Autoprocess { log } => {
            println!("🤖 Bootstrapping Autonomic RL Agents for Optimization...");
            run_wpm_cmd(&["autoprocess", "--log", &log.to_string_lossy()])?;
        }
        WpmAction::UseCase { action } => {
            crate::nouns::wpm_use_cases::print_use_case_instructions(&action);
        }
    }
    Ok(())
}
