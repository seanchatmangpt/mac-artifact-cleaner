//! Time Machine Exclusion CLI noun implementation.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{
    domain::{ocel::build_exclusion_plan_ocel, plan::DeletionPlan},
    integration::tmutil::{apply_exclusions_script, write_tm_exclusions_script},
};

#[derive(Subcommand, Debug)]
pub enum ExclusionAction {
    /// Generate a Time Machine exclusion script from a deletion plan
    Plan {
        /// Path to the deletion plan
        #[arg(short, long)]
        from: PathBuf,
        /// Output path for the bash script
        #[arg(short, long)]
        output: PathBuf,
        /// Path to write OCEL v2 JSON evidence
        #[arg(long)]
        ocel: Option<PathBuf>,
    },
    /// Apply exclusions by running the generated script
    Apply {
        /// Path to the exclusion script
        #[arg(short, long)]
        from: PathBuf,
    },
}

pub fn handle(action: ExclusionAction) -> anyhow::Result<()> {
    match action {
        ExclusionAction::Plan { from, output, ocel } => {
            let content = std::fs::read_to_string(&from)?;
            let plan: DeletionPlan = serde_json::from_str(&content)?;

            let candidates = crate::domain::plan::extract_exclusion_candidates(&plan);

            println!(
                "Generating Time Machine exclusion plan for {} directories...",
                candidates.len()
            );
            write_tm_exclusions_script(&output, &candidates)?;
            println!("Wrote Time Machine exclusion script to: {}", output.display());

            if let Some(o_path) = ocel {
                let ocel_log =
                    build_exclusion_plan_ocel(&output.display().to_string(), candidates.len());
                let serialized = serde_json::to_string_pretty(&ocel_log)?;
                std::fs::write(&o_path, serialized)?;
                println!("Wrote exclusion plan OCEL v2 log to: {}", o_path.display());
            }
        }
        ExclusionAction::Apply { from } => {
            println!("Applying Time Machine exclusions from: {}", from.display());
            apply_exclusions_script(&from)?;
            println!("Successfully applied Time Machine exclusions.");
        }
    }
    Ok(())
}
