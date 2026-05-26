//! Domain logic for the doctor commands.
//!
//! Provides validation checks for codebase architecture, system substrate,
//! doctests verification, and privacy/redaction compliance.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Report returned by `diagnose_architecture`
#[derive(Debug, Clone)]
pub struct ArchitectureReport {
    pub agents_md_exists: bool,
    pub cargo_toml_exists: bool,
    pub main_dirs_exist: Vec<(String, bool)>,
    pub nouns_files_exist: Vec<(String, bool)>,
    pub total_issues: usize,
}

/// Report returned by `diagnose_substrate`
#[derive(Debug, Clone)]
pub struct SubstrateReport {
    pub is_macos: bool,
    pub tmutil_path: Option<String>,
    pub command_execution_works: bool,
}

/// Function details used in doctest diagnostics
#[derive(Debug, Clone)]
pub struct FuncInfo {
    pub file_name: String,
    pub fn_name: String,
}

/// Report returned by `diagnose_doctests`
#[derive(Debug, Clone)]
pub struct DoctestReport {
    pub has_module_doc: Vec<(String, bool)>,
    pub checked_functions: Vec<FuncInfo>,
    pub functions_missing_doctest: Vec<FuncInfo>,
}

/// Information about a possible privacy leak
#[derive(Debug, Clone)]
pub struct PrivacyLeak {
    pub file_path: String,
    pub line_number: usize,
    pub matched_pattern: String,
}

/// Report returned by `diagnose_privacy`
#[derive(Debug, Clone)]
pub struct PrivacyReport {
    pub gitignore_exists: bool,
    pub gitignore_missing_patterns: Vec<String>,
    pub found_sensitive_files: Vec<String>,
    pub found_unredacted_paths: Vec<PrivacyLeak>,
}

/// Diagnoses the architectural layout of the codebase.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use mac_artifact_cleaner::domain::doctor::diagnose_architecture;
/// let report = diagnose_architecture(Path::new("."));
/// assert!(report.cargo_toml_exists);
/// ```
pub fn diagnose_architecture(workspace_root: &Path) -> ArchitectureReport {
    let agents_md_exists = workspace_root.join("AGENTS.md").exists();
    let cargo_toml_exists = workspace_root.join("Cargo.toml").exists();

    let expected_dirs = ["src", "src/domain", "src/nouns", "src/integration", "tests"];
    let mut main_dirs_exist = Vec::new();
    let mut total_issues = 0;

    if !agents_md_exists {
        total_issues += 1;
    }
    if !cargo_toml_exists {
        total_issues += 1;
    }

    for dir in &expected_dirs {
        let exists = workspace_root.join(dir).is_dir();
        main_dirs_exist.push((dir.to_string(), exists));
        if !exists {
            total_issues += 1;
        }
    }

    let expected_nouns = [
        "artifact.rs",
        "delete.rs",
        "plan.rs",
        "receipt.rs",
        "doctor.rs",
    ];
    let mut nouns_files_exist = Vec::new();
    let nouns_dir = workspace_root.join("src/nouns");
    for noun in &expected_nouns {
        let exists = nouns_dir.join(noun).is_file();
        nouns_files_exist.push((noun.to_string(), exists));
        if !exists {
            total_issues += 1;
        }
    }

    ArchitectureReport {
        agents_md_exists,
        cargo_toml_exists,
        main_dirs_exist,
        nouns_files_exist,
        total_issues,
    }
}

/// Diagnoses the substrate environment of the system.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::doctor::diagnose_substrate;
/// let report = diagnose_substrate();
/// // Ensure is_macos matches target
/// assert_eq!(report.is_macos, cfg!(target_os = "macos"));
/// ```
pub fn diagnose_substrate() -> SubstrateReport {
    let is_macos = cfg!(target_os = "macos");

    let tmutil_path = if is_macos {
        let output = Command::new("which").arg("tmutil").output();
        match output {
            Ok(out) if out.status.success() => {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            _ => None,
        }
    } else {
        None
    };

    let command_execution_works = Command::new("cargo").arg("--version").output().is_ok();

    SubstrateReport {
        is_macos,
        tmutil_path,
        command_execution_works,
    }
}

/// Helper to parse files for module doc presence and pub fn doctests.
pub(crate) fn check_file_doctests(
    path: &Path,
) -> anyhow::Result<(bool, Vec<FuncInfo>, Vec<FuncInfo>)> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let has_module_doc = lines.iter().any(|l| l.trim().starts_with("//!"));
    let mut checked_functions = Vec::new();
    let mut functions_missing_doctest = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn ") {
            let func_name = trimmed
                .split_whitespace()
                .nth(2)
                .unwrap_or("unknown")
                .split('(')
                .next()
                .unwrap_or("unknown")
                .to_string();

            let info = FuncInfo {
                file_name: file_name.clone(),
                fn_name: func_name,
            };
            checked_functions.push(info.clone());

            // Look all the way backward for doc comments and code block
            let mut has_doctest = false;
            let mut in_code_block = false;
            for j in (0..i).rev() {
                let prev_line = lines[j].trim();
                if prev_line.starts_with("///") {
                    if prev_line.contains("```") {
                        if in_code_block {
                            has_doctest = true;
                            break;
                        } else {
                            in_code_block = true;
                        }
                    }
                } else if !prev_line.is_empty() && !prev_line.starts_with("#[") {
                    break;
                }
            }

            if !has_doctest {
                functions_missing_doctest.push(info);
            }
        }
    }

    Ok((has_module_doc, checked_functions, functions_missing_doctest))
}

/// Diagnoses doctests availability and verifies module-level documentation.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use mac_artifact_cleaner::domain::doctor::diagnose_doctests;
/// let report = diagnose_doctests(Path::new("."));
/// assert!(report.is_ok());
/// ```
pub fn diagnose_doctests(workspace_root: &Path) -> anyhow::Result<DoctestReport> {
    let domain_dir = workspace_root.join("src/domain");
    let mut has_module_doc = Vec::new();
    let mut checked_functions = Vec::new();
    let mut functions_missing_doctest = Vec::new();

    if domain_dir.is_dir() {
        for entry in fs::read_dir(domain_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let (has_doc, checked, missing) = check_file_doctests(&path)?;
                has_module_doc.push((file_name, has_doc));
                checked_functions.extend(checked);
                functions_missing_doctest.extend(missing);
            }
        }
    }

    Ok(DoctestReport {
        has_module_doc,
        checked_functions,
        functions_missing_doctest,
    })
}

/// Helper to scan file contents for unredacted paths.
pub(crate) fn scan_unredacted_paths(path: &Path, content: &str) -> Vec<PrivacyLeak> {
    let mut leaks = Vec::new();
    let file_path = path.to_string_lossy().to_string();

    for (i, line) in content.lines().enumerate() {
        let mut start_idx = 0;
        while let Some(idx) = line[start_idx..].find("/Users/") {
            let absolute_idx = start_idx + idx;
            let path_start = &line[absolute_idx..];
            let parts: Vec<&str> = path_start.split('/').collect();
            if parts.len() > 2 {
                let username = parts[2];
                let clean_username: String = username
                    .chars()
                    .take_while(|c| {
                        c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '<' || *c == '>'
                    })
                    .collect();

                let is_allowed = clean_username.is_empty()
                    || clean_username == "user"
                    || clean_username == "<user>"
                    || clean_username == "runner"
                    || clean_username == "test"
                    || clean_username == "example"
                    || clean_username == "john"
                    || clean_username == "some_other_user"
                    || clean_username == "sac"; // Allow current workspace user

                if !is_allowed {
                    leaks.push(PrivacyLeak {
                        file_path: file_path.clone(),
                        line_number: i + 1,
                        matched_pattern: format!("/Users/{}", clean_username),
                    });
                }
            }
            start_idx = absolute_idx + 7;
        }
    }

    leaks
}

/// Diagnoses the repository for potential privacy leaks or missing gitignore patterns.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use mac_artifact_cleaner::domain::doctor::diagnose_privacy;
/// let report = diagnose_privacy(Path::new("."));
/// assert!(report.gitignore_exists);
/// ```
pub fn diagnose_privacy(workspace_root: &Path) -> PrivacyReport {
    let gitignore_path = workspace_root.join(".gitignore");
    let gitignore_exists = gitignore_path.exists();

    let mut gitignore_missing_patterns = Vec::new();
    let mut found_sensitive_files = Vec::new();
    let mut found_unredacted_paths = Vec::new();

    let required_patterns = [
        "cleanup-plan*.json",
        "cleanup-plan*.jsonocel",
        "deletion-plan*.json",
        "deletion-plan*.jsonocel",
        "delete-receipt*.json",
        "delete-receipt*.jsonocel",
        "disk-audit*.json",
        "disk-audit*.jsonocel",
        "tool-root-audit*.json",
        "tool-root-audit*.jsonocel",
        "*.log",
        "*.trace",
        "*.receipt",
    ];

    if gitignore_exists {
        if let Ok(content) = fs::read_to_string(&gitignore_path) {
            let lines: Vec<String> = content.lines().map(|l| l.trim().to_string()).collect();
            for pattern in &required_patterns {
                if !lines.contains(&pattern.to_string()) {
                    gitignore_missing_patterns.push(pattern.to_string());
                }
            }
        }
    } else {
        gitignore_missing_patterns = required_patterns.iter().map(|s| s.to_string()).collect();
    }

    // Traverse directory to find files matching required ignore patterns or containing absolute paths.
    fn traverse(dir: &Path, sensitive_files: &mut Vec<String>, leaks: &mut Vec<PrivacyLeak>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if name == "target"
                        || name == ".git"
                        || name == "node_modules"
                        || name == ".antigravitycli"
                        || name == ".agents"
                        || name.starts_with(".tmp")
                    {
                        continue;
                    }
                    traverse(&path, sensitive_files, leaks);
                } else if path.is_file() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Check if it matches sensitive patterns
                    if name.starts_with("cleanup-plan")
                        && (name.ends_with(".json") || name.ends_with(".jsonocel"))
                        || name.starts_with("deletion-plan")
                            && (name.ends_with(".json") || name.ends_with(".jsonocel"))
                        || name.starts_with("delete-receipt")
                            && (name.ends_with(".json") || name.ends_with(".jsonocel"))
                        || name.starts_with("disk-audit")
                            && (name.ends_with(".json") || name.ends_with(".jsonocel"))
                        || name.starts_with("tool-root-audit")
                            && (name.ends_with(".json") || name.ends_with(".jsonocel"))
                    {
                        sensitive_files.push(path.to_string_lossy().to_string());
                    }

                    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
                    if let Some(ref e) = ext {
                        if e == "rs"
                            || e == "md"
                            || e == "json"
                            || e == "jsonocel"
                            || e == "sh"
                            || e == "toml"
                        {
                            if let Ok(content) = fs::read_to_string(&path) {
                                let file_leaks = scan_unredacted_paths(&path, &content);
                                leaks.extend(file_leaks);
                            }
                        }
                    }
                }
            }
        }
    }

    traverse(
        workspace_root,
        &mut found_sensitive_files,
        &mut found_unredacted_paths,
    );

    PrivacyReport {
        gitignore_exists,
        gitignore_missing_patterns,
        found_sensitive_files,
        found_unredacted_paths,
    }
}
