//! Developer toolchain and package manager scan noun.

use clap::Subcommand;

use crate::integration::git_health::scan_git_repos;
use crate::integration::progress::human_bytes as format_bytes;
use crate::integration::toolchain::{
    list_npm_global_packages, list_pip_packages, list_rust_toolchains, npm_available,
    pip_available, rustup_available,
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
}

pub fn handle(action: ToolsAction) -> anyhow::Result<()> {
    match action {
        ToolsAction::Rustup => handle_rustup(),
        ToolsAction::Npm => handle_npm(),
        ToolsAction::Pip => handle_pip(),
        ToolsAction::Git { path } => handle_git(path),
    }
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
    let root = path
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

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
            println!(
                "    dangling worktrees ({}):",
                repo.dangling_worktrees.len()
            );
            for wt in &repo.dangling_worktrees {
                println!("      {wt}  [missing]");
            }
        }
    }
    Ok(())
}
