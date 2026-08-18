//! Artifact CLI noun implementation.

use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};

use clap::Subcommand;
use dashmap::DashMap;

use crate::{
    domain::{
        artifact::{ArgsSnapshot, Candidate},
        audit::Stats,
        tool_roots::build_tool_root_defs,
    },
    integration::{
        fs::scan_root,
        progress::{human_bytes, ProgressReporter},
    },
};

#[derive(Subcommand, Debug)]
pub enum ArtifactAction {
    /// Scan and display candidates for deletion
    Scan {
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
        /// Verbose trace output
        #[arg(long)]
        verbose: bool,
    },
}

pub fn handle(action: ArtifactAction) -> anyhow::Result<()> {
    match action {
        ArtifactAction::Scan { root, deps, aggressive, ignore_recent_hours, verbose } => {
            let roots = if root.is_empty() { crate::nouns::default_scan_roots()? } else { root };

            let args = ArgsSnapshot {
                deps,
                aggressive,
                verbose,
                tool_roots: false,
                ignore_recent_hours,
                all_filesystems: false,
            };

            let candidates: Arc<DashMap<PathBuf, Candidate>> = Arc::new(DashMap::new());
            let stats = Arc::new(Stats::default());
            *stats.phase.lock().unwrap() = "scanning files".to_string();

            let reporter = ProgressReporter::start("Scanning artifacts".to_string(), stats.clone());

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
                    None,
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

            println!("\nFound {} Deletion Candidates:", candidates.len());
            if candidates.is_empty() {
                println!("  No candidates found.");
            } else {
                let mut sorted: Vec<Candidate> =
                    candidates.iter().map(|e| e.value().clone()).collect();
                sorted.sort();
                for c in &sorted {
                    println!("  • {} ({})", c.path.display(), c.reason);
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
    }
    Ok(())
}
