//! Audit CLI noun implementation.

use clap::Subcommand;
use dashmap::DashMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::domain::artifact::{ArgsSnapshot, Candidate};
use crate::domain::audit::Stats;
use crate::domain::ocel::build_disk_audit_ocel;
use crate::domain::tool_roots::{
    build_tool_root_defs, build_tool_root_report, ToolRootAcc, ToolRootReport,
};
use crate::integration::fs::scan_root;
use crate::integration::progress::human_bytes;
use crate::integration::progress::ProgressReporter;

#[derive(Subcommand, Debug)]
pub enum AuditAction {
    /// Run a full disk audit and display results
    Run {
        /// Roots to scan (defaults to home directory)
        #[arg(long)]
        root: Vec<PathBuf>,
        /// Include dependencies (e.g. node_modules)
        #[arg(long)]
        deps: bool,
        /// Include aggressive build files
        #[arg(long)]
        aggressive: bool,
        /// Include major tool-root analysis
        #[arg(long)]
        tool_roots: bool,
        /// Verbose trace output
        #[arg(long)]
        verbose: bool,
        /// Path to write OCEL v2 event log
        #[arg(long)]
        ocel_output: Option<PathBuf>,
    },
    /// Run a full disk audit and present a premium user-friendly analytics summary
    Summarize {
        /// Roots to scan (defaults to home directory)
        #[arg(long)]
        root: Vec<PathBuf>,
        /// Include dependencies (e.g. node_modules)
        #[arg(long)]
        deps: bool,
        /// Include aggressive build files
        #[arg(long)]
        aggressive: bool,
        /// Include major tool-root analysis
        #[arg(long)]
        tool_roots: bool,
    },
}

pub fn handle(action: AuditAction) -> anyhow::Result<()> {
    match action {
        AuditAction::Run {
            root,
            deps,
            aggressive,
            tool_roots,
            verbose,
            ocel_output,
        } => {
            let roots = if root.is_empty() {
                vec![dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?]
            } else {
                root
            };

            let (stats, candidates, tool_reports) =
                run_audit_scan(&roots, deps, aggressive, tool_roots, verbose)?;

            println!("\n==================================================");
            println!("               DISK AUDIT RUN                     ");
            println!("==================================================");
            println!(
                "  Files analyzed:      {}",
                stats.files_seen.load(Ordering::Relaxed)
            );
            println!(
                "  Directories walked:  {}",
                stats.dirs_seen.load(Ordering::Relaxed)
            );
            println!(
                "  Total size scanned:  {}",
                human_bytes(stats.bytes_seen.load(Ordering::Relaxed))
            );
            println!(
                "  Projects detected:   {}",
                stats.projects_seen.load(Ordering::Relaxed)
            );
            println!(
                "  Skipped paths:       {}",
                stats.pruned_dirs.load(Ordering::Relaxed)
            );
            println!(
                "  Errors encountered:  {}",
                stats.errors.load(Ordering::Relaxed)
            );
            println!("==================================================");

            println!("\nFound {} Deletion Candidates:", candidates.len());
            if candidates.is_empty() {
                println!("  No candidates found.");
            } else {
                for c in &candidates {
                    println!("  • {} ({})", c.path.display(), c.reason);
                }
            }

            if tool_roots && !tool_reports.is_empty() {
                println!("\nTool Root Caches Audited (>= 0 MB):");
                for r in &tool_reports {
                    println!(
                        "  • {} - {} (recs: {})",
                        format_path_for_display(&r.path),
                        r.human,
                        r.recommendation
                    );
                }
            }

            if let Some(o_path) = ocel_output {
                let log = build_disk_audit_ocel(&roots, &candidates, &tool_reports, &stats);
                let serialized = serde_json::to_string_pretty(&log)?;
                std::fs::write(&o_path, serialized)?;
                println!("\nWrote OCEL v2 log to: {}", o_path.display());
            }
        }
        AuditAction::Summarize {
            root,
            deps,
            aggressive,
            tool_roots,
        } => {
            let roots = if root.is_empty() {
                vec![dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?]
            } else {
                root
            };

            let (stats, candidates, tool_reports) = run_audit_scan(
                &roots, deps, aggressive, tool_roots, false, // non-verbose for summary
            )?;

            print_premium_audit_summary(&stats, &candidates, &tool_reports);
        }
    }
    Ok(())
}

fn run_audit_scan(
    roots: &[PathBuf],
    deps: bool,
    aggressive: bool,
    tool_roots_enabled: bool,
    verbose: bool,
) -> anyhow::Result<(Arc<Stats>, Vec<Candidate>, Vec<ToolRootReport>)> {
    let args = ArgsSnapshot {
        deps,
        aggressive,
        verbose,
        tool_roots: tool_roots_enabled,
    };

    let candidates = Arc::new(Mutex::new(BTreeSet::<Candidate>::new()));
    let stats = Arc::new(Stats::default());
    *stats.phase.lock().unwrap() = "scanning disk".to_string();

    let reporter = ProgressReporter::start("Auditing disk".to_string(), stats.clone());

    let tool_defs = build_tool_root_defs();
    let tool_accs = Arc::new(DashMap::new());
    if tool_roots_enabled {
        for def in &tool_defs {
            tool_accs.insert(def.path.clone(), ToolRootAcc::default());
        }
    }

    for r in roots {
        scan_root(
            r,
            &args,
            candidates.clone(),
            stats.clone(),
            &tool_defs,
            tool_accs.clone(),
        )?;
    }

    reporter.finish("✅ Disk audit complete!");

    let cand_list: Vec<Candidate> = candidates.lock().unwrap().iter().cloned().collect();

    let tool_reports = if tool_roots_enabled {
        build_tool_root_report(&tool_defs, &tool_accs, 0)
    } else {
        Vec::new()
    };

    Ok((stats, cand_list, tool_reports))
}

fn format_path_for_display(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.starts_with(home_str.as_ref()) {
            return path.replacen(home_str.as_ref(), "~", 1);
        }
    }
    path.to_string()
}

fn print_premium_audit_summary(
    stats: &Stats,
    candidates: &[Candidate],
    tool_reports: &[ToolRootReport],
) {
    let files = stats.files_seen.load(Ordering::Relaxed);
    let dirs = stats.dirs_seen.load(Ordering::Relaxed);
    let bytes = stats.bytes_seen.load(Ordering::Relaxed);
    let projects = stats.projects_seen.load(Ordering::Relaxed);
    let errors = stats.errors.load(Ordering::Relaxed);

    let mut total_cand_bytes = 0u64;
    for c in candidates {
        // Simple file size check, or default if directory (could estimate, but estimate_size is too slow to run twice,
        // so we use meta length if it's a file, or a conservative default/not shown if dir)
        if let Ok(meta) = std::fs::metadata(&c.path) {
            if meta.is_file() {
                total_cand_bytes += meta.len();
            }
        }
    }

    let mut total_tool_bytes = 0u64;
    let mut actionable_tools = 0;
    for r in tool_reports {
        total_tool_bytes += r.bytes;
        if r.recommendation != "low_priority" && r.recommendation != "keep" {
            actionable_tools += 1;
        }
    }

    // Modern Terminal Dashboard Styling
    println!("\n\x1b[1m\x1b[36m┌────────────────────────────────────────────────────────┐\x1b[0m");
    println!("\x1b[1m\x1b[36m│             macOS Developer Disk Audit Summary         │\x1b[0m");
    println!("\x1b[1m\x1b[36m└────────────────────────────────────────────────────────┘\x1b[0m");

    println!("\n\x1b[1m\x1b[34m [1] FILESYSTEM METRICS\x1b[0m");
    println!("  • Total Files Scanned  : \x1b[1m{}\x1b[0m", files);
    println!("  • Directories Walked   : \x1b[1m{}\x1b[0m", dirs);
    println!(
        "  • Total Volume Size    : \x1b[1m\x1b[32m{}\x1b[0m",
        human_bytes(bytes)
    );
    println!("  • Projects Detected    : \x1b[1m{}\x1b[0m", projects);
    if errors > 0 {
        println!(
            "  • Errors Encountered   : \x1b[1m\x1b[31m{}\x1b[0m",
            errors
        );
    }

    println!("\n\x1b[1m\x1b[34m [2] DEVELOPER ARTIFACTS (CLEANUP CANDIDATES)\x1b[0m");
    println!(
        "  • Candidates Found     : \x1b[1m{}\x1b[0m",
        candidates.len()
    );
    if total_cand_bytes > 0 {
        println!(
            "  • Total Reclaimable    : \x1b[1m\x1b[32m{}\x1b[0m (direct files)",
            human_bytes(total_cand_bytes)
        );
    }

    if !candidates.is_empty() {
        println!("\n  Top Deletion Candidates:");
        println!("  ┌──────────────────────────────────────────────┬──────────────┬──────────────────────┐");
        println!("  │ Candidate Path                               │ Size/Status  │ Reason               │");
        println!("  ├──────────────────────────────────────────────┼──────────────┼──────────────────────┤");
        for c in candidates.iter().take(10) {
            let path_disp = format_path_for_display(&c.path.to_string_lossy());
            let path_truncated = if path_disp.len() > 44 {
                format!("...{}", &path_disp[path_disp.len() - 41..])
            } else {
                format!("{:<44}", path_disp)
            };

            let size_str = if let Ok(meta) = std::fs::metadata(&c.path) {
                if meta.is_file() {
                    human_bytes(meta.len())
                } else {
                    "DIR".to_string()
                }
            } else {
                "UNKNOWN".to_string()
            };

            let reason_disp = if c.reason.len() > 20 {
                format!("{:<20}", &c.reason[..20])
            } else {
                format!("{:<20}", c.reason)
            };

            println!(
                "  │ {:<44} │ {:<12} │ {:<20} │",
                path_truncated, size_str, reason_disp
            );
        }
        if candidates.len() > 10 {
            println!(
                "  │ ... and {} more candidates                                                 │",
                candidates.len() - 10
            );
        }
        println!("  └──────────────────────────────────────────────┴──────────────┴──────────────────────┘");
    }

    if !tool_reports.is_empty() {
        println!("\n\x1b[1m\x1b[34m [3] ROOT-LEVEL TOOL CACHES (TOOL ROOTS)\x1b[0m");
        println!(
            "  • Tool Roots Found     : \x1b[1m{}\x1b[0m",
            tool_reports.len()
        );
        println!(
            "  • Total Tool Cache Size: \x1b[1m\x1b[32m{}\x1b[0m",
            human_bytes(total_tool_bytes)
        );
        println!(
            "  • Actionable/Review    : \x1b[1m\x1b[33m{}\x1b[0m",
            actionable_tools
        );

        println!("\n  Top Tool Roots:");
        println!("  ┌──────────────────────────────────────────────┬──────────────┬──────────────────────┐");
        println!("  │ Cache Path                                   │ Size         │ Recommendation       │");
        println!("  ├──────────────────────────────────────────────┼──────────────┼──────────────────────┤");
        for r in tool_reports.iter().take(10) {
            let path_disp = format_path_for_display(&r.path);
            let path_truncated = if path_disp.len() > 44 {
                format!("...{}", &path_disp[path_disp.len() - 41..])
            } else {
                format!("{:<44}", path_disp)
            };

            let rec_color = match r.recommendation.as_str() {
                "cleanup_candidate" => "\x1b[31m", // Red
                "keep" => "\x1b[32m",              // Green
                "review" | "review_with_tool" | "review_high_value" => "\x1b[33m", // Yellow
                _ => "",
            };

            let rec_disp = format!("{}{}\x1b[0m", rec_color, r.recommendation);
            let padding_len = 20 - r.recommendation.len();
            let padding = " ".repeat(padding_len);

            println!(
                "  │ {:<44} │ {:<12} │ {}{} │",
                path_truncated, r.human, rec_disp, padding
            );
        }
        if tool_reports.len() > 10 {
            println!(
                "  │ ... and {} more tool roots                                                  │",
                tool_reports.len() - 10
            );
        }
        println!("  └──────────────────────────────────────────────┴──────────────┴──────────────────────┘");
    }

    println!("\n\x1b[1m\x1b[35m [4] ACTIONABLE INSIGHTS\x1b[0m");
    let total_potential = total_tool_bytes + total_cand_bytes;
    if total_potential > 0 {
        println!("  \x1b[1m💡 Reclaimable space:\x1b[0m You could reclaim up to \x1b[1m\x1b[32m{}\x1b[0m from cache & build targets.", human_bytes(total_potential));
    }
    println!("  \x1b[1m👉 Action:\x1b[0m Run `mac-artifact-cleaner plan build --output plan.json` to prepare a safe deletion plan.");
    println!();
}
