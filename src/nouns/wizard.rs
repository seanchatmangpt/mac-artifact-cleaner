//! Wizard CLI noun implementation for interactive maintenance.

use crate::nouns::{delete, exclusion, plan, snapshot};
use colored::*;
use dialoguer::Confirm;
use std::path::PathBuf;

pub fn handle() -> anyhow::Result<()> {
    println!(
        "{}",
        "====================================================="
            .blue()
            .bold()
    );
    println!(
        "{}",
        "     osx-clnr Interactive Maintenance Wizard         "
            .blue()
            .bold()
    );
    println!(
        "{}",
        "====================================================="
            .blue()
            .bold()
    );

    let plan_file = PathBuf::from("wizard-plan.json");
    let exclusion_script = PathBuf::from("wizard-tm-exclusions.sh");
    let receipt_file = PathBuf::from("wizard-receipt.json");

    println!(
        "\n{}",
        "[1/4] Scanning disk & building deletion plan...".blue()
    );
    plan::handle(plan::PlanAction::Build {
        root: vec![], // defaults to home dir + /tmp
        deps: true,
        aggressive: false,
        ignore_recent_hours: 168,
        output: plan_file.clone(),
        include_global_caches: false,
        verbose: false,
    })?;

    // Read plan to show count
    let content = std::fs::read_to_string(&plan_file)?;
    let plan_data: crate::domain::plan::DeletionPlan = serde_json::from_str(&content)?;

    if plan_data.items.is_empty() {
        println!(
            "\n{}",
            "🎉 No items to clean. You are already optimized!".green()
        );
        return Ok(());
    }

    println!(
        "\nFound {} items scheduled for deletion.",
        plan_data.items.len().to_string().yellow()
    );

    let proceed = Confirm::new()
        .with_prompt("Do you want to proceed with exclusions and deletion?")
        .default(true)
        .interact()
        .unwrap_or(true);

    if !proceed {
        println!("{}", "Maintenance aborted by user.".red());
        return Ok(());
    }

    println!(
        "\n{}",
        "[2/4] Generating & applying Time Machine exclusions...".blue()
    );
    exclusion::handle(exclusion::ExclusionAction::Plan {
        from: plan_file.clone(),
        output: exclusion_script.clone(),
        ocel: None,
    })?;

    exclusion::handle(exclusion::ExclusionAction::Apply {
        from: exclusion_script.clone(),
    })?;

    println!(
        "\n{}",
        "[3/4] Executing strictly from authorized plan...".blue()
    );
    delete::handle(delete::DeleteAction::Execute {
        plan: plan_file.clone(),
        receipt: receipt_file.clone(),
    })?;

    println!("\n{}", "[4/4] APFS Snapshot Check...".blue());
    println!("If space hasn't freed up, it may be pinned by Time Machine snapshots.");
    let thin_snapshots = Confirm::new()
        .with_prompt("Would you like to immediately thin local APFS snapshots to reclaim space?")
        .default(false)
        .interact()
        .unwrap_or(false);

    if thin_snapshots {
        snapshot::handle(snapshot::SnapshotAction::Thin {
            mount: "/".to_string(),
            bytes: "100GB".to_string(), // Request a large amount to clear old ones
            receipt: None,
            ocel: None,
        })?;
    }

    println!(
        "\n{}",
        "====================================================="
            .green()
            .bold()
    );
    println!(
        "{}",
        "🎉 Maintenance complete! You are safely optimized.    "
            .green()
            .bold()
    );
    println!(
        "{}",
        "====================================================="
            .green()
            .bold()
    );

    // Cleanup temp files if desired, but we can leave them as receipts
    let cleanup_artifacts = Confirm::new()
        .with_prompt("Clean up temporary wizard artifact files (plan, script)?")
        .default(true)
        .interact()
        .unwrap_or(true);

    if cleanup_artifacts {
        let _ = std::fs::remove_file(&plan_file);
        let _ = std::fs::remove_file(&exclusion_script);
        println!("Cleaned up wizard artifacts.");
    }

    Ok(())
}
