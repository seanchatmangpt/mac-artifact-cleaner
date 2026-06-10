//! OCEL v2 CLI noun implementation.

use crate::domain::ocel::{summarize_ocel_log, validate_ocel_log, OCEL};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum OcelAction {
    /// Validate the structure and referential integrity of an OCEL log
    Validate {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
    /// Summarize events and objects in the OCEL log
    Summarize {
        /// Path to the OCEL v2 log file
        #[arg(short, long)]
        log: PathBuf,
    },
}

pub fn handle(action: OcelAction) -> anyhow::Result<()> {
    match action {
        OcelAction::Validate { log } => {
            println!("Reading OCEL log from: {}", log.display());
            let content = std::fs::read_to_string(&log)?;
            let ocel_log: OCEL = serde_json::from_str(&content)?;

            println!("Validating OCEL log...");
            let report = validate_ocel_log(&ocel_log);
            if report.is_valid {
                println!("✅ OCEL log is structurally valid!");
            } else {
                println!("❌ OCEL log has validation errors:");
                for err in &report.errors {
                    println!("  - {}", err);
                }
                anyhow::bail!("OCEL validation failed");
            }
        }
        OcelAction::Summarize { log } => {
            println!("Reading OCEL log from: {}", log.display());
            let content = std::fs::read_to_string(&log)?;
            let ocel_log: OCEL = serde_json::from_str(&content)?;

            let summary = summarize_ocel_log(&ocel_log);
            println!("\n=== OCEL v2 Log Summary ===");
            println!("Total Events: {}", summary.total_events);
            println!("Total Objects: {}", summary.total_objects);

            println!("\nEvent Counts by Type:");
            let mut sorted_events: Vec<_> = summary.event_counts.iter().collect();
            sorted_events.sort_by(|a, b| b.1.cmp(a.1));
            for (etype, count) in sorted_events {
                println!("  - {}: {}", etype, count);
            }

            println!("\nObject Counts by Type:");
            let mut sorted_objects: Vec<_> = summary.object_counts.iter().collect();
            sorted_objects.sort_by(|a, b| b.1.cmp(a.1));
            for (otype, count) in sorted_objects {
                println!("  - {}: {}", otype, count);
            }

            if !summary.audit_stats.is_empty() {
                println!("\nDisk Audit Details:");
                for (idx, audit) in summary.audit_stats.iter().enumerate() {
                    println!("  Audit #{} (created at {}):", idx + 1, audit.created_at);
                    println!("    Files Seen:       {}", audit.files_seen);
                    println!("    Dirs Seen:        {}", audit.dirs_seen);
                    println!("    Bytes Seen:       {} bytes", audit.bytes_seen);
                    println!("    Projects Seen:    {}", audit.projects_seen);
                    println!("    Candidates Seen:  {}", audit.candidates_seen);
                    println!("    Pruned Dirs:      {}", audit.pruned_dirs);
                    println!("    Errors:           {}", audit.errors);
                }
            }
        }
    }
    Ok(())
}
