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
use crate::integration::fs::{physical_dir_size, scan_root, write_or_dump_on_full};
use crate::integration::progress::ProgressReporter;
use rayon::prelude::*;
use std::os::unix::fs::MetadataExt;

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
        /// Also nominate large user-level caches (~/Library/Caches, Xcode
        /// DerivedData, ~/.cache, cargo registry, npm/go caches) — off by default
        #[arg(long)]
        include_global_caches: bool,
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
            include_global_caches,
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
            let mut candidate_vec: Vec<Candidate> = lock.iter().cloned().collect();
            drop(lock);

            // Optionally nominate large user-level caches the per-project scanner
            // never reaches. Guarded by `is_macos_os_dir` and existence; the curated
            // allowlist itself is the safety boundary.
            if include_global_caches {
                if let Some(home) = dirs::home_dir() {
                    let known: std::collections::HashSet<_> =
                        candidate_vec.iter().map(|c| c.path.clone()).collect();
                    for (path, reason) in crate::domain::artifact::global_cache_candidates(&home) {
                        if path.exists()
                            && !crate::domain::artifact::is_macos_os_dir(&path)
                            && !known.contains(&path)
                        {
                            candidate_vec.push(Candidate { path, reason });
                        }
                    }
                }
            }

            // Size each candidate in parallel using physical allocation (blocks × 512),
            // so the plan shows reclaim impact and the receipt can prove bytes freed.
            // `physical_dir_size` (jwalk) is fast enough to run once per candidate here.
            let mut items: Vec<PlanItem> = candidate_vec
                .par_iter()
                .map(|c| {
                    let kind = if c.path.is_file() {
                        PlanItemKind::File
                    } else {
                        PlanItemKind::Dir
                    };
                    let bytes = match kind {
                        PlanItemKind::File => std::fs::symlink_metadata(&c.path)
                            .map(|m| m.blocks() * 512)
                            .unwrap_or(0),
                        PlanItemKind::Dir => physical_dir_size(&c.path),
                        PlanItemKind::GithubRepo
                        | PlanItemKind::GithubBranch
                        | PlanItemKind::GithubRun
                        | PlanItemKind::GithubRelease => 0,
                    };

                    PlanItem {
                        path: c.path.clone(),
                        kind,
                        reason: c.reason.clone(),
                        bytes,
                    }
                })
                .collect();

            // Largest reclaim first — both for the printed preview and so deletion
            // tackles the biggest wins before any I/O errors can interrupt it.
            items.sort_by(|a, b| b.bytes.cmp(&a.bytes));

            let plan_total: u64 = items.iter().map(|i| i.bytes).sum();

            let plan = DeletionPlan::new(roots, deps, aggressive, items, vec![]);
            let serialized = serde_json::to_string_pretty(&plan)?;
            write_or_dump_on_full(&output, &serialized, "deletion plan")?;

            println!("\n✨ Success: Wrote deletion plan to: {}", output.display());
            println!("   Total deletion items: {}", plan.items.len());
            println!("   Estimated reclaim:    {}", human_bytes(plan_total));
            let top_n = 10.min(plan.items.len());
            if top_n > 0 {
                println!("   Top {} items by size:", top_n);
                for item in plan.items.iter().take(top_n) {
                    println!(
                        "     {:>10}  {}",
                        human_bytes(item.bytes),
                        item.path.display()
                    );
                }
            }

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
                        "  • [{:?}] {:>10}  {} - {}",
                        item.kind,
                        human_bytes(item.bytes),
                        item.path.display(),
                        item.reason
                    );
                }
                let total: u64 = plan_data.items.iter().map(|i| i.bytes).sum();
                println!("  ── Estimated reclaim: {}", human_bytes(total));
            }
            println!("==================================================");
        }
    }
    Ok(())
}
