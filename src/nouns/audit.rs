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
use crate::integration::fs::{
    breakdown_sizes, find_cargo_target_dirs, find_large_files, force_remove_dir_all, scan_root,
};
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
        /// Ignore projects modified within the specified number of hours
        #[arg(long, default_value_t = 168)]
        ignore_recent_hours: u64,
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
        /// Ignore projects modified within the specified number of hours
        #[arg(long, default_value_t = 168)]
        ignore_recent_hours: u64,
        /// Include major tool-root analysis
        #[arg(long)]
        tool_roots: bool,
    },
    /// Run cargo clean on all Rust target/ directories under a root
    CargoClean {
        /// Root to search (default: home directory)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Dry-run: show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,
    },
    /// Find every file larger than a size threshold, sorted largest-first
    FindLarge {
        /// Root path to search (default: home directory)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Minimum file size in MB (default: 100)
        #[arg(long, default_value = "100")]
        min_mb: u64,
        /// Show top N results (default: 60)
        #[arg(long, default_value = "60")]
        top: usize,
    },
    /// Delete one or more cache/tool directories, handling macOS immutable flags
    CacheClean {
        /// Paths to remove (supports ~/ expansion)
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Show disk usage broken down by top-level directory (includes hidden dirs)
    Breakdown {
        /// Root to scan (defaults to home directory)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Show only the top N entries (default: 40)
        #[arg(long, default_value = "40")]
        top: usize,
        /// Hide entries smaller than this many MB (default: 0)
        #[arg(long, default_value = "0")]
        min_mb: u64,
    },
}

pub fn handle(action: AuditAction) -> anyhow::Result<()> {
    match action {
        AuditAction::Run {
            root,
            deps,
            aggressive,
            ignore_recent_hours,
            tool_roots,
            verbose,
            ocel_output,
        } => {
            let roots = if root.is_empty() {
                crate::nouns::default_scan_roots()?
            } else {
                root
            };

            let (stats, candidates, tool_reports) =
                run_audit_scan(&roots, deps, aggressive, ignore_recent_hours, tool_roots, verbose)?;

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
        AuditAction::CargoClean { root, dry_run } => {
            let search_root = match root {
                Some(p) => p,
                None => dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?,
            };

            eprintln!(
                "Scanning {} for Rust target/ directories…",
                search_root.display()
            );
            let targets = find_cargo_target_dirs(&search_root)?;

            if targets.is_empty() {
                println!("No Rust target/ directories found.");
                return Ok(());
            }

            let total: u64 = targets.iter().map(|(_, s)| s).sum();
            println!(
                "\n  Found {} target/ directories  ({}  physical)\n",
                targets.len(),
                human_bytes(total)
            );
            println!("  {:<65}  {:>10}", "Path", "Size");
            println!("  {}", "─".repeat(78));
            for (path, size) in &targets {
                let display = format_path_for_display(&path.to_string_lossy());
                let display = if display.len() > 65 {
                    format!("…{}", &display[display.len() - 64..])
                } else {
                    display
                };
                println!("  {:<65}  {:>10}", display, human_bytes(*size));
            }
            println!("  {}", "─".repeat(78));
            println!("  Total reclaimable: {}\n", human_bytes(total));

            if dry_run {
                println!("  (dry-run — nothing deleted)");
                return Ok(());
            }

            let mut freed = 0u64;
            let mut errors: Vec<String> = Vec::new();
            for (path, size) in &targets {
                let display = format_path_for_display(&path.to_string_lossy());
                eprint!("  del  {} … ", display);
                match force_remove_dir_all(path) {
                    Ok(()) => {
                        freed += size;
                        eprintln!("done  ({})", human_bytes(*size));
                    }
                    Err(e) => {
                        eprintln!("FAILED");
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }
            println!("\n  Freed: {}", human_bytes(freed));
            if !errors.is_empty() {
                eprintln!("\nErrors:");
                for e in &errors {
                    eprintln!("  {e}");
                }
            }
        }
        AuditAction::FindLarge { root, min_mb, top } => {
            let search_root = match root {
                Some(p) => p,
                None => dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?,
            };
            let min_bytes = min_mb * 1024 * 1024;

            eprint!(
                "Searching {} for files >= {} MB …\r",
                search_root.display(),
                min_mb
            );

            let results = find_large_files(&search_root, min_bytes, |files, found| {
                eprint!(
                    "  scanned {:>10} files  |  found {:>6} large files\r",
                    files, found
                );
            })?;

            // Clear progress line.
            eprintln!("{:80}", "");

            print_large_files(&results, top, min_mb);
        }
        AuditAction::CacheClean { paths } => {
            let mut total_freed: u64 = 0;
            let mut errors: Vec<String> = Vec::new();

            for raw in &paths {
                // Expand leading ~
                let path = if raw.starts_with("~") {
                    let home =
                        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?;
                    let stripped = raw.strip_prefix("~").unwrap_or(raw);
                    home.join(stripped.strip_prefix("/").unwrap_or(stripped))
                } else {
                    raw.clone()
                };

                if !path.exists() {
                    eprintln!("  skip  {} (not found)", path.display());
                    continue;
                }

                // Measure size before deletion for the report.
                let before: u64 = {
                    let mut builder = ignore::WalkBuilder::new(&path);
                    builder
                        .hidden(false)
                        .ignore(false)
                        .git_ignore(false)
                        .git_global(false)
                        .git_exclude(false)
                        .follow_links(false)
                        .same_file_system(true);
                    let mut sz = 0u64;
                    for e in builder.build().flatten() {
                        if let Ok(m) = e.metadata() {
                            if m.is_file() {
                                sz += m.len();
                            }
                        }
                    }
                    sz
                };

                eprint!("  del   {} ({}) … ", path.display(), human_bytes(before));
                match force_remove_dir_all(&path) {
                    Ok(()) => {
                        total_freed += before;
                        eprintln!("done");
                    }
                    Err(e) => {
                        eprintln!("FAILED");
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }

            println!("\nTotal freed: {}", human_bytes(total_freed));
            if !errors.is_empty() {
                eprintln!("\nErrors:");
                for e in &errors {
                    eprintln!("  {e}");
                }
            }
        }
        AuditAction::Breakdown { root, top, min_mb } => {
            let scan_root_path = match root {
                Some(p) => p,
                None => dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home dir not found"))?,
            };
            eprintln!(
                "Scanning {} for disk usage (including hidden dirs)…",
                scan_root_path.display()
            );
            let results = breakdown_sizes(&scan_root_path)?;
            print_breakdown(&scan_root_path, &results, top, min_mb);
        }
        AuditAction::Summarize {
            root,
            deps,
            aggressive,
            ignore_recent_hours,
            tool_roots,
        } => {
            let roots = if root.is_empty() {
                crate::nouns::default_scan_roots()?
            } else {
                root
            };

            let (stats, candidates, tool_reports) =
                run_audit_scan(&roots, deps, aggressive, ignore_recent_hours, tool_roots, false, // non-verbose for summary
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
    ignore_recent_hours: u64,
    tool_roots_enabled: bool,
    verbose: bool,
) -> anyhow::Result<(Arc<Stats>, Vec<Candidate>, Vec<ToolRootReport>)> {
    let args = ArgsSnapshot {
        deps,
        aggressive,
        verbose,
        tool_roots: tool_roots_enabled,
        ignore_recent_hours,
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
        crate::integration::fs::populate_tool_roots_metadata(&tool_defs, &tool_accs);
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

fn print_large_files(results: &[(PathBuf, u64)], top: usize, min_mb: u64) {
    let visible: Vec<_> = results.iter().take(top).collect();
    let total_large: u64 = results.iter().map(|(_, b)| b).sum();

    println!(
        "\n\x1b[1m\x1b[36m┌─────────────────────────────────────────────────────────────────────────────┐\x1b[0m"
    );
    println!(
        "\x1b[1m\x1b[36m│  Large Files  (>= {} MB)  —  {} files  —  {} total                \x1b[0m",
        min_mb,
        results.len(),
        human_bytes(total_large)
    );
    println!(
        "\x1b[1m\x1b[36m└─────────────────────────────────────────────────────────────────────────────┘\x1b[0m"
    );

    println!("\n  {:<10}  {}", "Size", "Path");
    println!("  {}", "─".repeat(78));

    for (path, bytes) in &visible {
        let display = format_path_for_display(&path.to_string_lossy());
        let display = if display.len() > 65 {
            format!("…{}", &display[display.len() - 64..])
        } else {
            display
        };

        let size_str = human_bytes(*bytes);
        let color = if *bytes >= 1024 * 1024 * 1024 {
            "\x1b[31m" // red for >= 1 GB
        } else if *bytes >= 500 * 1024 * 1024 {
            "\x1b[33m" // yellow for >= 500 MB
        } else {
            "\x1b[0m"
        };

        println!("  {}{:<10}\x1b[0m  {}", color, size_str, display);
    }

    println!("  {}", "─".repeat(78));
    if results.len() > top {
        println!(
            "  … {} more files not shown (pass --top {} to see all)\n",
            results.len() - top,
            results.len()
        );
    } else {
        println!();
    }
}

fn print_breakdown(root: &std::path::Path, results: &[(PathBuf, u64)], top: usize, min_mb: u64) {
    let min_bytes = min_mb * 1024 * 1024;
    let visible: Vec<_> = results
        .iter()
        .filter(|(_, b)| *b >= min_bytes)
        .take(top)
        .collect();

    let total_bytes: u64 = results.iter().map(|(_, b)| b).sum();
    let total_shown: u64 = visible.iter().map(|(_, b)| b).sum();

    println!(
        "\n\x1b[1m\x1b[36m┌─────────────────────────────────────────────────────────────┐\x1b[0m"
    );
    println!(
        "\x1b[1m\x1b[36m│  Disk Breakdown: {:<43}│\x1b[0m",
        format_path_for_display(&root.to_string_lossy())
    );
    println!(
        "\x1b[1m\x1b[36m└─────────────────────────────────────────────────────────────┘\x1b[0m"
    );
    println!(
        "  Total scanned: \x1b[1m\x1b[32m{}\x1b[0m across {} top-level entries\n",
        human_bytes(total_bytes),
        results.len()
    );

    println!(
        "  {:<50} {:>10}  {:>6}  {}",
        "Path", "Size", "% of ~", "Category"
    );
    println!("  {}", "─".repeat(80));

    for (path, bytes) in &visible {
        let pct = if total_bytes > 0 {
            (*bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };
        let display = format_path_for_display(&path.to_string_lossy());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display.clone());

        let category = categorize_dir(&name);
        let cat_color = match category {
            "tool cache" => "\x1b[33m",
            "build artifact" => "\x1b[31m",
            "data" => "\x1b[32m",
            _ => "\x1b[0m",
        };

        println!(
            "  {:<50} {:>10}  {:>5.1}%  {}{}  \x1b[0m",
            if display.len() > 50 {
                format!("…{}", &display[display.len() - 49..])
            } else {
                display
            },
            human_bytes(*bytes),
            pct,
            cat_color,
            category,
        );
    }

    println!("  {}", "─".repeat(80));
    println!(
        "  Shown: \x1b[1m{}\x1b[0m of total \x1b[1m{}\x1b[0m  ({} entries hidden)\n",
        human_bytes(total_shown),
        human_bytes(total_bytes),
        results.len() - visible.len(),
    );
    println!("  Legend: \x1b[33m■ tool cache\x1b[0m  \x1b[31m■ build artifact\x1b[0m  \x1b[32m■ data\x1b[0m  ■ project/other");
    println!();
}

fn categorize_dir(name: &str) -> &'static str {
    // Hidden dirs that are tool caches / runtimes
    let tool_caches = [
        ".ollama",
        ".cache",
        ".cargo",
        ".rustup",
        ".npm",
        ".pnpm-store",
        ".yarn",
        ".bun",
        ".deno",
        ".gradle",
        ".m2",
        ".pyenv",
        ".rbenv",
        ".asdf",
        ".sdkman",
        ".local",
        ".colima",
        ".docker",
        ".minikube",
        ".gemini",
        ".codeium",
        ".claude",
        ".codex",
        ".conda",
        ".venv",
        "miniconda3",
        ".multipass",
    ];
    if tool_caches.contains(&name) {
        return "tool cache";
    }
    // Well-known data dirs
    let data_dirs = [
        "Documents",
        "Downloads",
        "Desktop",
        "Pictures",
        "Movies",
        "Music",
        "Library",
        "Public",
    ];
    if data_dirs.contains(&name) {
        return "data";
    }
    "project/other"
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
    println!("  \x1b[1m👉 Action:\x1b[0m Run `osx-clnr plan build --output plan.json` to prepare a safe deletion plan.");
    println!();
}
