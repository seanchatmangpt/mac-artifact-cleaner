//! Plan CLI noun implementation.

use clap::Subcommand;
use dashmap::DashMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::artifact::{ArgsSnapshot, Candidate};
use crate::domain::audit::Stats;
use crate::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use crate::domain::tool_roots::build_tool_root_defs;
use crate::integration::fs::scan_root;
use crate::integration::progress::ProgressReporter;

#[derive(Subcommand, Debug)]
pub enum PlanAction {
    /// Build a new dry-run deletion plan
    Build {
        /// Roots to scan (defaults to home directory)
        #[arg(long)]
        root: Vec<PathBuf>,
        /// Include dependencies (e.g. node_modules)
        #[arg(long)]
        deps: bool,
        /// Include aggressive build files
        #[arg(long)]
        aggressive: bool,
        /// Ignore projects modified within the specified number of hours
        #[arg(long, default_value_t = 168)]
        ignore_recent_hours: u64,
        /// Plan file output destination
        #[arg(short, long)]
        output: PathBuf,
        /// Verbose trace output
        #[arg(long)]
        verbose: bool,
    },
    /// Inspect a built deletion plan
    Inspect {
        /// Path to the deletion plan
        #[arg(short, long)]
        plan: PathBuf,
    },
}

use crate::integration::progress::human_bytes;
use std::sync::atomic::Ordering;

pub fn handle(action: PlanAction) -> anyhow::Result<()> {
    match action {
        PlanAction::Build {
            root,
            deps,
            aggressive,
            ignore_recent_hours,
            output,
            verbose,
        } => {
            let roots = if root.is_empty() {
                crate::nouns::default_scan_roots()?
            } else {
                root
            };

            let args = ArgsSnapshot {
                deps,
                aggressive,
                verbose,
                tool_roots: false,
                ignore_recent_hours,
            };

            let candidates = Arc::new(Mutex::new(BTreeSet::<Candidate>::new()));
            let stats = Arc::new(Stats::default());
            *stats.phase.lock().unwrap() = "scanning files".to_string();

            let reporter = ProgressReporter::start("Scanning for plan".to_string(), stats.clone());

            let tool_defs = build_tool_root_defs();
            let tool_accs = Arc::new(DashMap::new());

            for r in &roots {
                scan_root(
                    r,
                    &args,
                    candidates.clone(),
                    stats.clone(),
                    &tool_defs,
                    tool_accs.clone(),
                )?;
            }

            reporter.finish("✅ Scan complete!");

            let files = stats.files_seen.load(Ordering::Relaxed);
            let dirs = stats.dirs_seen.load(Ordering::Relaxed);
            let bytes = stats.bytes_seen.load(Ordering::Relaxed);
            let projects = stats.projects_seen.load(Ordering::Relaxed);
            let skipped = stats.pruned_dirs.load(Ordering::Relaxed);
            let errors = stats.errors.load(Ordering::Relaxed);

            println!("\n==================================================");
            println!("               SCAN SUMMARY               ");
            println!("==================================================");
            println!("  Files analyzed:      {}", files);
            println!("  Directories walked:  {}", dirs);
            println!("  Total size scanned:  {}", human_bytes(bytes));
            println!("  Projects detected:   {}", projects);
            println!("  Skipped paths:       {} (OS/caches/barriers)", skipped);
            println!("  Errors encountered:  {}", errors);
            println!("==================================================");

            let lock = candidates.lock().unwrap();
            let mut items = Vec::new();
            for c in lock.iter() {
                let kind = if c.path.is_file() {
                    PlanItemKind::File
                } else {
                    PlanItemKind::Dir
                };
                items.push(PlanItem {
                    path: c.path.clone(),
                    kind,
                    reason: c.reason.clone(),
                });
            }

            let plan = DeletionPlan::new(roots, deps, aggressive, items, vec![]);
            let serialized = serde_json::to_string_pretty(&plan)?;
            std::fs::write(&output, serialized)?;

            println!("\n✨ Success: Wrote deletion plan to: {}", output.display());
            println!("   Total deletion items: {}", plan.items.len());

            let err_lock = stats.error_details.lock().unwrap();
            if !err_lock.is_empty() {
                println!("\n==================================================");
                println!("               TRAVERSAL ERRORS                   ");
                println!("==================================================");
                let display_limit = 10;
                for (path, err_msg) in err_lock.iter().take(display_limit) {
                    println!("  ❌ {}: {}", path.display(), err_msg);
                }
                if err_lock.len() > display_limit {
                    println!("  ... and {} more errors.", err_lock.len() - display_limit);
                }
                println!("==================================================");
            }
        }
        PlanAction::Inspect { plan } => {
            let content = std::fs::read_to_string(&plan)?;
            let plan_data: DeletionPlan = serde_json::from_str(&content)?;
            println!("\n==================================================");
            println!("            DELETION PLAN INSPECTION              ");
            println!("==================================================");
            println!("  Plan File:   {}", plan.display());
            println!("  Version:     {}", plan_data.version);
            println!(
                "  Created:     {}",
                chrono::DateTime::from_timestamp(plan_data.created_unix as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| plan_data.created_unix.to_string())
            );
            println!("  Roots:       {:?}", plan_data.roots);
            println!(
                "  Flags:       deps={}, aggressive={}",
                plan_data.deps, plan_data.aggressive
            );
            println!("  Total Items: {}", plan_data.items.len());
            println!("==================================================");
            println!("\nScheduled Deletions:");
            if plan_data.items.is_empty() {
                println!("  (No items scheduled)");
            } else {
                for item in &plan_data.items {
                    println!(
                        "  • [{:?}] {} - {}",
                        item.kind,
                        item.path.display(),
                        item.reason
                    );
                }
            }
            println!("==================================================");
        }
    }
    Ok(())
}
