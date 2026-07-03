//! Doctor CLI noun implementation.

use std::path::Path;

use clap::Subcommand;

use crate::domain::doctor::{
    diagnose_architecture, diagnose_doctests, diagnose_domain_purity, diagnose_privacy,
    diagnose_scan_delete_separation, diagnose_substrate,
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
    /// Assert src/domain/** has zero std::fs/std::process/OS calls
    DomainPurity,
    /// Assert the scanner-cannot-delete / deleter-cannot-scan invariant
    ScanDeleteSeparation,
}

pub fn handle(action: DoctorAction) -> anyhow::Result<()> {
    let workspace_root = Path::new(".");
    match action {
        DoctorAction::Architecture => {
            println!("Auditing architecture layout...");
            let (agents_md_exists, cargo_toml_exists, main_dirs_exist, nouns_files_exist) =
                crate::integration::doctor::read_workspace_architecture(workspace_root);

            let report = diagnose_architecture(
                agents_md_exists,
                cargo_toml_exists,
                main_dirs_exist,
                nouns_files_exist,
            );

            println!("- AGENTS.md exists: {}", report.agents_md_exists);
            println!("- Cargo.toml exists: {}", report.cargo_toml_exists);
            println!("\nDirectories check:");
            for (dir, exists) in &report.main_dirs_exist {
                println!("  {:<20} : {}", dir, if *exists { "Found" } else { "Missing" });
            }
            println!("\nNouns files check:");
            for (noun, exists) in &report.nouns_files_exist {
                println!("  {:<20} : {}", noun, if *exists { "Found" } else { "Missing" });
            }
            println!("\nTotal architecture issues: {}", report.total_issues);
            if report.total_issues > 0 {
                anyhow::bail!(
                    "Architecture check failed with {} issues.\n\nSuggestions:\n  - Review the directories/nouns checks above for missing files\n  - Ensure src/domain, src/integration, src/nouns, src/mcp exist as expected\n  - Re-run `oclnr doctor arch` after fixing",
                    report.total_issues
                );
            } else {
                println!("✅ Architecture check passed!");
            }
        }
        DoctorAction::Substrate => {
            println!("Auditing system substrate...");
            let is_macos = cfg!(target_os = "macos");
            let (tmutil_path, command_execution_works) =
                crate::integration::doctor::query_substrate_info();

            let report = diagnose_substrate(is_macos, tmutil_path, command_execution_works);

            println!("- Running on macOS: {}", report.is_macos);
            println!(
                "- tmutil executable: {}",
                report
                    .tmutil_path
                    .as_deref()
                    .unwrap_or("Not found (Time Machine capabilities might be limited)")
            );
            println!("- Command execution works: {}", report.command_execution_works);
            if !report.is_macos {
                println!("⚠️ Warning: This tool is designed primarily for macOS.");
            } else {
                println!("✅ Substrate check completed successfully!");
            }
        }
        DoctorAction::Doctests => {
            println!("Auditing module and function doctests...");
            let files = crate::integration::doctor::read_doctest_files(workspace_root);
            let report = diagnose_doctests(&files);

            println!("\nModule level docs found:");
            for (module, has_doc) in &report.has_module_doc {
                println!(
                    "  {:<25} : {}",
                    module,
                    if *has_doc { "Yes" } else { "No (//! missing)" }
                );
            }

            println!("\nChecked public functions count: {}", report.checked_functions.len());
            println!("Missing doctests: {}", report.functions_missing_doctest.len());

            if !report.functions_missing_doctest.is_empty() {
                println!("\nDetailed list of public functions missing doctests:");
                for info in &report.functions_missing_doctest {
                    println!("  - File: {}, Function: {}", info.file_name, info.fn_name);
                }
                anyhow::bail!(
                    "Doctests check failed. Public functions must contain doctests.\n\nSuggestions:\n  - Add doctests to the functions listed above (positive + negative + refusal cases)\n  - Re-run `oclnr doctor doc` to confirm"
                );
            } else {
                println!("✅ All public functions have valid doctests!");
            }
        }
        DoctorAction::DomainPurity => {
            println!("Auditing domain purity (src/domain/** must have zero std::fs/std::process/OS calls)...");
            let files = crate::integration::doctor::read_domain_files(workspace_root);
            let report = diagnose_domain_purity(&files);

            println!("- Files scanned: {}", report.files_scanned);

            if !report.violations.is_empty() {
                println!("\n❌ Domain-purity violations found:");
                for v in &report.violations {
                    println!("  - {}:{} -> {}", v.file_path, v.line_number, v.matched_pattern);
                }
                anyhow::bail!(
                    "Domain-purity check failed with {} violation(s). src/domain/** must have zero std::fs, std::process, or OS calls.",
                    report.violations.len()
                );
            } else {
                println!(
                    "✅ Domain-purity check passed! No forbidden OS calls found in src/domain/**."
                );
            }
        }
        DoctorAction::ScanDeleteSeparation => {
            println!("Auditing scanner/deleter separation (scanner cannot delete; deleter cannot scan)...");
            let files = crate::integration::doctor::read_delete_path_files(workspace_root);
            let report = diagnose_scan_delete_separation(&files);

            println!("- Files scanned: {}", report.files_scanned);
            println!(
                "- delete path reads from plan_file: {}",
                if report.plan_file_param_found { "Yes" } else { "No" }
            );

            let mut failed = false;
            if !report.violations.is_empty() {
                println!("\n❌ Forbidden scan/audit calls found in delete path:");
                for v in &report.violations {
                    println!("  - {}:{} -> {}", v.file_path, v.line_number, v.matched_pattern);
                }
                failed = true;
            }
            if !report.plan_file_param_found {
                println!("\n❌ Delete path does not appear to read from a plan_file parameter.");
                failed = true;
            }

            if failed {
                anyhow::bail!(
                    "Scan/delete separation check failed. The deleter must never scan; it must read exclusively from a previously-built plan file."
                );
            } else {
                println!("✅ Scan/delete separation check passed!");
            }
        }
        DoctorAction::Privacy => {
            println!("Auditing privacy constraints and sensitive leaks...");
            let (gitignore_exists, gitignore_content, found_sensitive_files, files_to_scan) =
                crate::integration::doctor::read_privacy_files(workspace_root);

            let report = diagnose_privacy(
                gitignore_exists,
                gitignore_content,
                found_sensitive_files,
                &files_to_scan,
            );

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
                anyhow::bail!(
                    "Privacy check failed. Found unredacted local paths.\n\nSuggestions:\n  - Run `oclnr privacy redact --file <path>` on each flagged file\n  - Re-run `oclnr doctor priv` to confirm"
                );
            } else {
                println!(
                    "✅ Privacy check passed! No local user profiles or unredacted paths found."
                );
            }
        }
    }
    Ok(())
}
