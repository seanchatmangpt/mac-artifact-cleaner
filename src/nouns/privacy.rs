//! Privacy CLI noun implementation.
//!
//! Provides CLI commands to scan the workspace for sensitive information leaks
//! and to redact files in-place.
//!
//! # Examples
//!
//! ```
//! use std::path::PathBuf;
//! use osx_clnr::nouns::privacy::{handle, PrivacyAction};
//!
//! // Negative/refusal case: Attempting to redact a non-existent file returns an error
//! let action = PrivacyAction::Redact {
//!     file: PathBuf::from("non_existent_file_12345.txt"),
//! };
//! assert!(handle(action).is_err());
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::Subcommand;

use crate::domain::{doctor::diagnose_privacy, redaction::redact_content};

#[derive(Subcommand, Debug, Clone)]
pub enum PrivacyAction {
    /// Scan the workspace using `diagnose_privacy` and report any unredacted paths/credentials or sensitive files.
    Scan,
    /// Load a file, apply `redact_content`, and save the redacted file in-place.
    Redact {
        /// Path to the file to redact in-place
        #[arg(short, long)]
        file: PathBuf,
    },
}

/// Handles the privacy actions.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use osx_clnr::nouns::privacy::{handle, PrivacyAction};
///
/// // Positive case: redact a temporary file
/// let temp_file = std::env::temp_dir().join("test_redact_doctest.txt");
/// std::fs::write(&temp_file, "my password is supersecret").unwrap();
/// let action = PrivacyAction::Redact {
///     file: temp_file.clone(),
/// };
/// assert!(handle(action).is_ok());
/// let content = std::fs::read_to_string(&temp_file).unwrap();
/// assert_eq!(content, "my password is [REDACTED]");
/// std::fs::remove_file(temp_file).unwrap();
///
/// // Negative/refusal case: attempting to redact a non-existent file returns an error
/// let action = PrivacyAction::Redact {
///     file: PathBuf::from("non_existent_file_12345.txt"),
/// };
/// assert!(handle(action).is_err());
/// ```
pub fn handle(action: PrivacyAction) -> anyhow::Result<()> {
    match action {
        PrivacyAction::Scan => {
            println!("Scanning workspace for privacy and sensitive leaks...");
            let workspace_root = Path::new(".");
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
                anyhow::bail!("Privacy scan failed. Found unredacted local paths.");
            }

            if !report.found_sensitive_files.is_empty() {
                anyhow::bail!("Privacy scan failed. Found sensitive files that should be cleaned or gitignored.");
            }

            println!("✅ Privacy scan passed! No local user profiles, credentials, or unredacted paths found.");
            Ok(())
        }
        PrivacyAction::Redact { file } => {
            if !file.exists() {
                anyhow::bail!("Error: File does not exist at {}", file.display());
            }
            if !file.is_file() {
                anyhow::bail!("Error: Path is not a file: {}", file.display());
            }

            println!("Reading file for redaction: {}", file.display());
            let content = fs::read_to_string(&file)?;
            let redacted = redact_content(&content);

            if content == redacted {
                println!("No sensitive patterns found. File remains unchanged.");
            } else {
                println!("Writing redacted content back to: {}", file.display());
                fs::write(&file, redacted)?;
                println!("✅ Redaction complete.");
            }
            Ok(())
        }
    }
}
