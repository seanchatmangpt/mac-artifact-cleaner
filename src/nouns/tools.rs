//! Developer toolchain and package manager scan noun.

use clap::Subcommand;

use crate::integration::{
    git_health::scan_git_repos,
    progress::human_bytes as format_bytes,
    toolchain::{
        list_npm_global_packages, list_pip_packages, list_rust_toolchains, npm_available,
        pip_available, rustup_available,
    },
};

#[derive(Subcommand, Debug)]
pub enum ToolsAction {
    /// Scan installed Rust toolchains
    Rustup,
    /// Scan npm global packages
    Npm,
    /// Scan pip/pip3 global packages
    Pip,
    /// Scan git repositories for health issues
    Git {
        /// Root directory to scan (defaults to home)
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Read-only worktree standing: classify linked git worktrees
    /// (PRUNABLE / MERGED_CLEAN reclaimable; DIRTY / UNMERGED / DETACHED /
    /// LOCKED / UNKNOWN not) and report .git bloat. Never removes anything —
    /// removal is UNSUPPORTED (would go through plan/approve/delete).
    #[command(name = "git-worktrees", alias = "worktree-standing")]
    GitWorktrees {
        /// Root directory to scan (repeatable; defaults to home)
        #[arg(long = "root", value_name = "PATH")]
        roots: Vec<std::path::PathBuf>,
        /// Directory levels to descend when discovering repos
        #[arg(long, default_value = "4")]
        depth: u8,
        /// Write the full JSON report to this path
        #[arg(long, value_name = "FILE")]
        output: Option<std::path::PathBuf>,
        /// Number of bloated .git dirs to list
        #[arg(long, default_value = "10")]
        top: usize,
    },
}

pub fn handle(action: ToolsAction) -> anyhow::Result<()> {
    match action {
        ToolsAction::Rustup => handle_rustup(),
        ToolsAction::Npm => handle_npm(),
        ToolsAction::Pip => handle_pip(),
        ToolsAction::Git { path } => handle_git(path),
        ToolsAction::GitWorktrees { roots, depth, output, top } => {
            handle_git_worktrees(roots, depth, output, top)
        }
    }
}

fn handle_git_worktrees(
    roots: Vec<std::path::PathBuf>,
    depth: u8,
    output: Option<std::path::PathBuf>,
    top: usize,
) -> anyhow::Result<()> {
    use crate::{
        domain::git_worktree::WorktreeClass,
        integration::{fs::write_output_file, git_health::worktree_standing},
    };

    let roots = if roots.is_empty() {
        vec![dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory not found"))?]
    } else {
        roots
    };
    for r in &roots {
        if !r.is_dir() {
            anyhow::bail!("root is not a directory: {}", r.display());
        }
    }

    let report = worktree_standing(&roots, depth);
    let s = report.summary;

    println!("Git worktree standing (read-only; removal UNSUPPORTED)");
    println!(
        "  repos: {}   linked worktrees: {}   .git total: {}",
        s.repos,
        s.linked_worktrees,
        format_bytes(s.git_dir_bytes)
    );
    println!(
        "  reclaimable: {} worktrees, {}  (merged_clean {}, prunable {})",
        s.reclaimable_worktrees,
        format_bytes(s.reclaimable_bytes),
        s.merged_clean,
        s.prunable
    );
    println!(
        "  not reclaimable: dirty {}, unmerged {}, detached {}, locked {}, unknown {}",
        s.dirty, s.unmerged, s.detached, s.locked, s.unknown
    );

    let mut reclaimable: Vec<_> = report
        .repos
        .iter()
        .flat_map(|r| r.worktrees.iter())
        .filter(|w| w.verdict.reclaimable)
        .collect();
    reclaimable.sort_by_key(|w| std::cmp::Reverse(w.size_bytes));
    if !reclaimable.is_empty() {
        println!();
        println!("Reclaimable worktrees:");
        for w in &reclaimable {
            let tag = match w.verdict.class {
                WorktreeClass::Prunable => "PRUNABLE",
                _ => "MERGED_CLEAN",
            };
            println!(
                "  {:>10}  {:<12} {}  [{}] {}",
                format_bytes(w.size_bytes),
                tag,
                w.path,
                w.branch.as_deref().unwrap_or("-"),
                w.last_commit.as_deref().unwrap_or("?")
            );
        }
    }

    let mut bloated: Vec<_> = report.repos.iter().collect();
    bloated.sort_by_key(|r| std::cmp::Reverse(r.git_dir_bytes));
    if !bloated.is_empty() {
        println!();
        println!("Top .git dirs:");
        for r in bloated.iter().take(top) {
            let (loose, pack) = r
                .count_objects
                .map(|c| {
                    (
                        format!("{} ({})", c.loose_count, format_bytes(c.loose_bytes)),
                        format_bytes(c.pack_bytes),
                    )
                })
                .unwrap_or_else(|| ("?".into(), "?".into()));
            let gc = if r.gc_signals.is_empty() {
                "no".to_string()
            } else {
                format!("yes {:?}", r.gc_signals)
            };
            println!(
                "  {:>10}  {}  loose {}  pack {}  gc-would-help: {}",
                format_bytes(r.git_dir_bytes),
                r.git_dir,
                loose,
                pack,
                gc
            );
            println!(
                "              of which modules/ {}  worktrees/ {}",
                format_bytes(r.modules_bytes),
                format_bytes(r.worktrees_admin_bytes)
            );
            for e in &r.errors {
                println!("      error: {e}");
            }
        }
    }

    if let Some(path) = output {
        let json = serde_json::to_string_pretty(&report)?;
        let (outcome, _) = write_output_file(&path, &json, false, "worktree standing report")?;
        if outcome.is_written() {
            println!();
            println!("Report written: {}", path.display());
        }
    }
    Ok(())
}

fn handle_rustup() -> anyhow::Result<()> {
    if !rustup_available() {
        println!("rustup is not available on PATH.");
        return Ok(());
    }

    let result = list_rust_toolchains()?;

    println!("Rust Toolchains (rustup)");
    for tc in &result.toolchains {
        if tc.is_default {
            println!("  {} (default)", tc.name);
        } else {
            println!("  {}", tc.name);
        }
    }
    println!();
    println!("  ~/.rustup: {}", format_bytes(result.rustup_home_bytes));
    Ok(())
}

fn handle_npm() -> anyhow::Result<()> {
    if !npm_available() {
        println!("npm is not available on PATH.");
        return Ok(());
    }

    let packages = list_npm_global_packages()?;

    println!("npm global packages ({} installed)", packages.len());
    for pkg in &packages {
        println!("  {}@{}", pkg.name, pkg.version);
    }
    Ok(())
}

fn handle_pip() -> anyhow::Result<()> {
    if !pip_available() {
        println!("pip/pip3 is not available on PATH.");
        return Ok(());
    }

    let packages = list_pip_packages()?;

    println!("pip global packages ({} installed)", packages.len());
    let shown = packages.iter().take(20);
    for pkg in shown {
        println!("  {} {}", pkg.name, pkg.version);
    }
    if packages.len() > 20 {
        println!("  ... and {} more", packages.len() - 20);
    }
    Ok(())
}

fn handle_git(path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let root = path.or_else(dirs::home_dir).unwrap_or_else(|| std::path::PathBuf::from("."));

    println!("Scanning git repositories under: {}", root.display());
    println!();

    let repos = scan_git_repos(&root)?;

    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(());
    }

    println!("Found {} git repositories:", repos.len());
    for repo in &repos {
        println!();
        println!("  {}", repo.path.display());
        println!("    pack size:     {}", format_bytes(repo.pack_size_bytes));
        println!("    loose objects: {}", repo.loose_objects);
        if !repo.dangling_worktrees.is_empty() {
            println!("    dangling worktrees ({}):", repo.dangling_worktrees.len());
            for wt in &repo.dangling_worktrees {
                println!("      {wt}  [missing]");
            }
        }
    }
    Ok(())
}
