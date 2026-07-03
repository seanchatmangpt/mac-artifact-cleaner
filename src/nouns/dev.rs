//! Dev CLI noun implementation for emergency local cleanup.
//!
//! Provides an emergency lever to clean all builds and deps in a specific
//! directory (defaults to current directory) by disabling recency protections
//! and enabling aggressive/deps cleanup.

use std::path::PathBuf;

use colored::*;
use dialoguer::Confirm;

use crate::nouns::{delete, plan};

pub fn handle(path: Option<PathBuf>) -> anyhow::Result<()> {
    println!("{}", "=====================================================".red().bold());
    println!("{}", "     osx-clnr DEV: Emergency Local Cleanup Lever     ".red().bold());
    println!("{}", "=====================================================".red().bold());
    if let Some(ref p) = path {
        println!("Target directory: {}", p.display());
    } else {
        println!("Target directory: ~ (Whole Computer Deep Scan)");
    }

    let plan_file = PathBuf::from("dev-cleanup-plan.json");
    let receipt_file = PathBuf::from("dev-deletion-receipt.json");

    println!("\n{}", "[1/2] Scanning for ALL deps and builds (ignoring recency)...".blue());
    plan::handle(plan::PlanAction::Build {
        root: path.map(|p| vec![p]).unwrap_or_default(),
        deps: true,
        aggressive: true,
        ignore_recent_hours: 0, // EMERGENCY LEVER: Disable recency protection
        output: plan_file.clone(),
        include_global_caches: false,
        verbose: false,
    })?;

    // Read plan to show count
    let content = std::fs::read_to_string(&plan_file)?;
    let plan_data: crate::domain::plan::DeletionPlan = serde_json::from_str(&content)?;

    if plan_data.items.is_empty() {
        println!("\n{}", "🎉 No items to clean in this directory.".green());
        let _ = std::fs::remove_file(&plan_file);
        return Ok(());
    }

    println!(
        "\nFound {} items scheduled for emergency deletion.",
        plan_data.items.len().to_string().yellow()
    );

    let proceed = Confirm::new()
        .with_prompt("Do you want to proceed with emergency deletion?")
        .default(false)
        .interact()
        .unwrap_or(false);

    if !proceed {
        println!(
            "{}",
            "Emergency cleanup aborted by user. Plan saved to dev-cleanup-plan.json".yellow()
        );
        return Ok(());
    }

    println!("\n{}", "[2/2] Executing strictly from authorized plan...".blue());
    delete::handle(delete::DeleteAction::Execute {
        plan: plan_file.clone(),
        receipt: receipt_file.clone(),
        yes: true,
    })?;

    println!("\n{}", "=====================================================".green().bold());
    println!("{}", "🎉 Emergency cleanup complete!                        ".green().bold());
    println!("{}", "=====================================================".green().bold());

    let cleanup_artifacts = Confirm::new()
        .with_prompt("Clean up temporary dev artifact files (plan, receipt)?")
        .default(true)
        .interact()
        .unwrap_or(true);

    if cleanup_artifacts {
        let _ = std::fs::remove_file(&plan_file);
        let _ = std::fs::remove_file(&receipt_file);
        println!("Cleaned up dev artifacts.");
    }

    Ok(())
}
