//! Doctor CLI noun implementation.

use clap::Subcommand;
use std::path::Path;

use crate::domain::doctor::{
    diagnose_architecture, diagnose_doctests, diagnose_privacy, diagnose_substrate,
};

#[derive(Subcommand, Debug)]
pub enum DoctorAction {
    /// Check standard architectural layout
    Architecture,
    /// Check macOS system substrate capabilities
    Substrate,
    /// Assert doc-test completeness for domain module functions
    Doctests,
    /// Assert privacy and redaction rule compliance
    Privacy,
}

pub fn handle(action: DoctorAction) -> anyhow::Result<()> {
    let workspace_root = Path::new(".");
    match action {
        DoctorAction::Architecture => {
            println!("Auditing architecture layout...");
            let report = diagnose_architecture(workspace_root);
            println!("- AGENTS.md exists: {}", report.agents_md_exists);
            println!("- Cargo.toml exists: {}", report.cargo_toml_exists);
            println!("\nDirectories check:");
            for (dir, exists) in &report.main_dirs_exist {
                println!(
                    "  {:<20} : {}",
                    dir,
                    if *exists { "Found" } else { "Missing" }
                );
            }
            println!("\nNouns files check:");
            for (noun, exists) in &report.nouns_files_exist {
                println!(
                    "  {:<20} : {}",
                    noun,
                    if *exists { "Found" } else { "Missing" }
                );
            }
            println!("\nTotal architecture issues: {}", report.total_issues);
            if report.total_issues > 0 {
                anyhow::bail!(
                    "Architecture check failed with {} issues.",
                    report.total_issues
                );
            } else {
                println!("✅ Architecture check passed!");
            }
        }
        DoctorAction::Substrate => {
            println!("Auditing system substrate...");
            let report = diagnose_substrate();
            println!("- Running on macOS: {}", report.is_macos);
            println!(
                "- tmutil executable: {}",
                report
                    .tmutil_path
                    .as_deref()
                    .unwrap_or("Not found (Time Machine capabilities might be limited)")
            );
            println!(
                "- Command execution works: {}",
                report.command_execution_works
            );
            if !report.is_macos {
                println!("⚠️ Warning: This tool is designed primarily for macOS.");
            } else {
                println!("✅ Substrate check completed successfully!");
            }
        }
        DoctorAction::Doctests => {
            println!("Auditing module and function doctests...");
            let report = diagnose_doctests(workspace_root)?;
            println!("\nModule level docs found:");
            for (module, has_doc) in &report.has_module_doc {
                println!(
                    "  {:<25} : {}",
                    module,
                    if *has_doc { "Yes" } else { "No (//! missing)" }
                );
            }

            println!(
                "\nChecked public functions count: {}",
                report.checked_functions.len()
            );
            println!(
                "Missing doctests: {}",
                report.functions_missing_doctest.len()
            );

            if !report.functions_missing_doctest.is_empty() {
                println!("\nDetailed list of public functions missing doctests:");
                for info in &report.functions_missing_doctest {
                    println!("  - File: {}, Function: {}", info.file_name, info.fn_name);
                }
                anyhow::bail!("Doctests check failed. Public functions must contain doctests.");
            } else {
                println!("✅ All public functions have valid doctests!");
            }
        }
        DoctorAction::Privacy => {
            println!("Auditing privacy constraints and sensitive leaks...");
            let report = diagnose_privacy(workspace_root);
            println!("- .gitignore exists: {}", report.gitignore_exists);

            if !report.gitignore_missing_patterns.is_empty() {
                println!("\n⚠️ Missing patterns in .gitignore:");
                for pattern in &report.gitignore_missing_patterns {
                    println!("  - {}", pattern);
                }
            } else {
                println!("- .gitignore contains all required patterns.");
            }

            if !report.found_sensitive_files.is_empty() {
                println!("\n⚠️ Sensitive/temporary files found in workspace (should be gitignored/cleaned):");
                for file in &report.found_sensitive_files {
                    println!("  - {}", file);
                }
            } else {
                println!("- No sensitive/temporary plan files found committed/stored.");
            }

            if !report.found_unredacted_paths.is_empty() {
                println!("\n❌ Unredacted local home paths found:");
                for leak in &report.found_unredacted_paths {
                    println!(
                        "  - {}:{} -> {}",
                        leak.file_path, leak.line_number, leak.matched_pattern
                    );
                }
                anyhow::bail!("Privacy check failed. Found unredacted local paths.");
            } else {
                println!(
                    "✅ Privacy check passed! No local user profiles or unredacted paths found."
                );
            }
        }
    }
    Ok(())
}
