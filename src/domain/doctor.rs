//! Domain logic for the doctor commands.
//!
//! Provides validation checks for codebase architecture, system substrate,
//! doctests verification, and privacy/redaction compliance.
//! All functions in this module are pure and do not perform any I/O.

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
/// use osx_clnr::domain::doctor::diagnose_architecture;
///
/// // Positive case: all required files and directories are present
/// let report = diagnose_architecture(true, true, vec![("src".to_string(), true)], vec![("artifact.rs".to_string(), true)]);
/// assert!(report.cargo_toml_exists);
/// assert_eq!(report.total_issues, 0);
///
/// // Negative case: Cargo.toml is missing
/// let report_missing = diagnose_architecture(true, false, vec![("src".to_string(), true)], vec![("artifact.rs".to_string(), true)]);
/// assert!(!report_missing.cargo_toml_exists);
/// assert_eq!(report_missing.total_issues, 1);
/// ```
pub fn diagnose_architecture(
    agents_md_exists: bool,
    cargo_toml_exists: bool,
    main_dirs_exist: Vec<(String, bool)>,
    nouns_files_exist: Vec<(String, bool)>,
) -> ArchitectureReport {
    let mut total_issues = 0;

    if !agents_md_exists {
        total_issues += 1;
    }
    if !cargo_toml_exists {
        total_issues += 1;
    }

    for (_, exists) in &main_dirs_exist {
        if !exists {
            total_issues += 1;
        }
    }

    for (_, exists) in &nouns_files_exist {
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
/// use osx_clnr::domain::doctor::diagnose_substrate;
///
/// // Positive case
/// let report = diagnose_substrate(true, Some("/usr/sbin/tmutil".to_string()), true);
/// assert!(report.is_macos);
/// assert!(report.command_execution_works);
/// ```
pub fn diagnose_substrate(
    is_macos: bool,
    tmutil_path: Option<String>,
    command_execution_works: bool,
) -> SubstrateReport {
    SubstrateReport { is_macos, tmutil_path, command_execution_works }
}

/// Helper to parse file contents for module doc presence and pub fn doctests.
pub(crate) fn check_file_doctests_content(
    file_name: &str,
    content: &str,
) -> (bool, Vec<FuncInfo>, Vec<FuncInfo>) {
    let lines: Vec<&str> = content.lines().collect();
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

            let info = FuncInfo { file_name: file_name.to_string(), fn_name: func_name };
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

    (has_module_doc, checked_functions, functions_missing_doctest)
}

/// Diagnoses doctests availability and verifies module-level documentation.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::diagnose_doctests;
///
/// // Positive case: check in-memory files
/// let files = vec![("artifact.rs".to_string(), "//! Module doc\n\n/// Test\n/// ```\n/// let x = 5;\n/// ```\npub fn foo() {}".to_string())];
/// let report = diagnose_doctests(&files);
/// assert_eq!(report.has_module_doc[0].1, true);
/// assert!(report.functions_missing_doctest.is_empty());
/// ```
pub fn diagnose_doctests(files: &[(String, String)]) -> DoctestReport {
    let mut has_module_doc = Vec::new();
    let mut checked_functions = Vec::new();
    let mut functions_missing_doctest = Vec::new();

    for (file_name, content) in files {
        let (has_doc, checked, missing) = check_file_doctests_content(file_name, content);
        has_module_doc.push((file_name.clone(), has_doc));
        checked_functions.extend(checked);
        functions_missing_doctest.extend(missing);
    }

    DoctestReport { has_module_doc, checked_functions, functions_missing_doctest }
}

/// Helper to scan file contents for unredacted paths.
pub(crate) fn scan_unredacted_paths(file_path: &str, content: &str) -> Vec<PrivacyLeak> {
    let mut leaks = Vec::new();

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
                    || clean_username == "some_other_user";
                // ADVERSARIAL PASS: Removed hardcoded bypass check for user "sac"

                if !is_allowed {
                    leaks.push(PrivacyLeak {
                        file_path: file_path.to_string(),
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
/// use osx_clnr::domain::doctor::diagnose_privacy;
///
/// // Positive case: verify clean environment
/// let report = diagnose_privacy(true, Some("cleanup-plan*.json\n*.log".to_string()), vec![], &[]);
/// assert!(report.gitignore_exists);
/// assert!(!report.gitignore_missing_patterns.is_empty());
/// ```
pub fn diagnose_privacy(
    gitignore_exists: bool,
    gitignore_content: Option<String>,
    found_sensitive_files: Vec<String>,
    files_to_scan: &[(String, String)],
) -> PrivacyReport {
    let mut gitignore_missing_patterns = Vec::new();
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
        if let Some(content) = gitignore_content {
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

    for (file_path, content) in files_to_scan {
        let file_leaks = scan_unredacted_paths(file_path, content);
        found_unredacted_paths.extend(file_leaks);
    }

    PrivacyReport {
        gitignore_exists,
        gitignore_missing_patterns,
        found_sensitive_files,
        found_unredacted_paths,
    }
}
