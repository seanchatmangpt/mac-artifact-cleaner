//! Tool Roots CLI noun implementation.

use clap::Subcommand;
use dashmap::DashMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::artifact::{ArgsSnapshot, Candidate};
use crate::domain::audit::Stats;
use crate::domain::ocel::build_tool_roots_ocel;
use crate::domain::tool_roots::{build_tool_root_defs, build_tool_root_report, ToolRootAcc};
use crate::integration::fs::scan_root;
use crate::integration::progress::ProgressReporter;

#[derive(Subcommand, Debug)]
pub enum ToolRootsAction {
    /// Audit and display root tool dependency caches
    Audit {
        /// Minimum size in MB to display (default: 0)
        #[arg(long, default_value_t = 0)]
        min_mb: u64,
        /// Path to write OCEL v2 event log
        #[arg(long)]
        ocel_output: Option<PathBuf>,
    },
    /// Print a high-level summary of tool roots
    Summarize {
        /// Minimum size in MB to summarize (default: 0)
        #[arg(long, default_value_t = 0)]
        min_mb: u64,
    },
}

pub fn handle(action: ToolRootsAction) -> anyhow::Result<()> {
    match action {
        ToolRootsAction::Audit {
            min_mb,
            ocel_output,
        } => {
            let reports = run_tool_roots_scan(min_mb)?;
            println!("Tool Root Audit Results (>= {} MB):", min_mb);
            for r in &reports {
                println!(
                    "{} - {} (files: {}, dirs: {})",
                    r.path, r.human, r.files, r.dirs
                );
                println!("  Category: {}", r.category);
                println!("  Recommendation: {}", r.recommendation);
                println!("  Rationale: {}", r.rationale);
                println!();
            }

            if let Some(o_path) = ocel_output {
                let log = build_tool_roots_ocel(&reports);
                let serialized = serde_json::to_string_pretty(&log)?;
                std::fs::write(&o_path, serialized)?;
                println!("Wrote OCEL v2 log to: {}", o_path.display());
            }
        }
        ToolRootsAction::Summarize { min_mb } => {
            let reports = run_tool_roots_scan(min_mb)?;
            print_premium_tool_roots_summary(&reports, min_mb);
        }
    }
    Ok(())
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

fn print_premium_tool_roots_summary(
    reports: &[crate::domain::tool_roots::ToolRootReport],
    min_mb: u64,
) {
    let mut total_bytes = 0u64;
    let mut actionable_tools = 0;
    for r in reports {
        total_bytes += r.bytes;
        if r.recommendation != "low_priority" && r.recommendation != "keep" {
            actionable_tools += 1;
        }
    }

    // Modern Terminal Dashboard Styling
    println!("\n\x1b[1m\x1b[36m┌────────────────────────────────────────────────────────┐\x1b[0m");
    println!("\x1b[1m\x1b[36m│            Developer Tool-Roots Cache Summary          │\x1b[0m");
    println!("\x1b[1m\x1b[36m└────────────────────────────────────────────────────────┘\x1b[0m");

    println!("\n\x1b[1m\x1b[34m [1] SUMMARY METRICS\x1b[0m");
    println!("  • Tool Roots Found    : \x1b[1m{}\x1b[0m", reports.len());
    println!(
        "  • Total Cache Size    : \x1b[1m\x1b[32m{}\x1b[0m",
        crate::domain::tool_roots::human_bytes(total_bytes)
    );
    println!(
        "  • Actionable Caches   : \x1b[1m\x1b[33m{}\x1b[0m (Require review/prune)",
        actionable_tools
    );

    if !reports.is_empty() {
        println!(
            "\n\x1b[1m\x1b[34m [2] TOOL ROOTS BREAKDOWN (>= {} MB)\x1b[0m",
            min_mb
        );
        println!("  ┌──────────────────────────────────────────────┬──────────────┬──────────────────────┐");
        println!("  │ Cache Path                                   │ Size         │ Recommendation       │");
        println!("  ├──────────────────────────────────────────────┼──────────────┼──────────────────────┤");
        for r in reports.iter().take(10) {
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
        if reports.len() > 10 {
            println!(
                "  │ ... and {} more tool roots                                                  │",
                reports.len() - 10
            );
        }
        println!("  └──────────────────────────────────────────────┴──────────────┴──────────────────────┘");
    }

    println!("\n\x1b[1m\x1b[35m [3] SUGGESTED COMMANDS\x1b[0m");
    println!("  \x1b[1m👉 Action:\x1b[0m Run `mac-artifact-cleaner tool-roots audit` to see full details of each tool cache.");
    println!();
}

fn run_tool_roots_scan(
    min_mb: u64,
) -> anyhow::Result<Vec<crate::domain::tool_roots::ToolRootReport>> {
    let tool_defs = build_tool_root_defs();
    let tool_accs = Arc::new(DashMap::new());

    for def in &tool_defs {
        tool_accs.insert(def.path.clone(), ToolRootAcc::default());
    }

    let args = ArgsSnapshot {
        deps: false,
        aggressive: false,
        verbose: false,
        tool_roots: true,
    };

    let stats = Arc::new(Stats::default());
    *stats.phase.lock().unwrap() = "scanning tool roots".to_string();

    let reporter = ProgressReporter::start("Auditing tool roots".to_string(), stats.clone());

    // Safety optimization: sweep only existing directories matching defined tool roots
    let candidates = Arc::new(Mutex::new(BTreeSet::<Candidate>::new()));
    for def in &tool_defs {
        if def.path.exists() {
            scan_root(
                &def.path,
                &args,
                candidates.clone(),
                stats.clone(),
                &tool_defs,
                tool_accs.clone(),
            )?;
        }
    }

    reporter.finish("Tool roots scan complete!");

    let min_bytes = min_mb * 1024 * 1024;
    let reports = build_tool_report_with_reverse_bytes(&tool_defs, &tool_accs, min_bytes);
    Ok(reports)
}

fn build_tool_report_with_reverse_bytes(
    defs: &[crate::domain::tool_roots::ToolRootDef],
    accs: &DashMap<PathBuf, ToolRootAcc>,
    min_bytes: u64,
) -> Vec<crate::domain::tool_roots::ToolRootReport> {
    let mut reports = build_tool_root_report(defs, accs, min_bytes);
    // Ensure sorting by reverse bytes matches final Clippy fixes
    reports.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    reports
}
