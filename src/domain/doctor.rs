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

/// A single detected domain-purity violation (forbidden OS/fs/process call
/// found inside `src/domain/**`).
#[derive(Debug, Clone)]
pub struct PurityViolation {
    pub file_path: String,
    pub line_number: usize,
    pub matched_pattern: String,
}

/// Report returned by `diagnose_domain_purity`
#[derive(Debug, Clone)]
pub struct DomainPurityReport {
    pub files_scanned: usize,
    pub violations: Vec<PurityViolation>,
}

/// Forbidden patterns that indicate a domain-purity violation: direct
/// filesystem, process, or other OS-call access from `src/domain/**`.
const FORBIDDEN_DOMAIN_PATTERNS: &[&str] = &[
    "std::fs::",
    "use std::fs",
    "fs::File",
    "fs::read",
    "fs::write",
    "fs::create",
    "fs::remove",
    "fs::metadata",
    "fs::DirEntry",
    "std::process::Command",
    "use std::process",
    "process::Command",
    "std::net::",
    "use std::net",
    "sled::Db",
    "sled::",
];

/// Scans a single domain file's content for forbidden OS-call patterns,
/// skipping lines carrying an explicit `// doctor-allow: domain-purity`
/// justification comment.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::scan_domain_purity;
///
/// // Positive case: no violations
/// let clean = scan_domain_purity("plan.rs", "pub fn build() -> u32 { 1 }");
/// assert!(clean.is_empty());
///
/// // Negative case: a std::fs import is flagged
/// let dirty = scan_domain_purity("crypto.rs", "use std::fs::File;\n");
/// assert_eq!(dirty.len(), 1);
/// assert_eq!(dirty[0].line_number, 1);
///
/// // Refusal case: explicitly allow-listed line is not flagged
/// let allowed = scan_domain_purity(
///     "ocl.rs",
///     "use std::fs::File; // doctor-allow: domain-purity\n",
/// );
/// assert!(allowed.is_empty());
/// ```
pub fn scan_domain_purity(file_path: &str, content: &str) -> Vec<PurityViolation> {
    let mut violations = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        // Skip comments and doc-comments: they reference these patterns in
        // prose (e.g. explaining what the integration layer does) without
        // actually invoking them.
        if trimmed.starts_with("//") {
            continue;
        }
        // Skip string-literal pattern definitions (this module's own
        // allowlist tables reference the pattern text without using it).
        if trimmed.starts_with('"') || trimmed.contains("\": &[&str]") {
            continue;
        }
        if line.contains("doctor-allow: domain-purity") {
            continue;
        }
        for pattern in FORBIDDEN_DOMAIN_PATTERNS {
            if line.contains(pattern) {
                violations.push(PurityViolation {
                    file_path: file_path.to_string(),
                    line_number: i + 1,
                    matched_pattern: (*pattern).to_string(),
                });
                break;
            }
        }
    }

    violations
}

/// Diagnoses `src/domain/**` for violations of the hard domain-purity
/// constraint: zero `std::fs`, `std::process`, or other OS calls.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::diagnose_domain_purity;
///
/// // Positive case: clean domain files pass
/// let files = vec![("plan.rs".to_string(), "pub fn build() {}".to_string())];
/// let report = diagnose_domain_purity(&files);
/// assert!(report.violations.is_empty());
///
/// // Negative case: std::fs usage is caught
/// let dirty = vec![("crypto.rs".to_string(), "use std::fs::File;".to_string())];
/// let report = diagnose_domain_purity(&dirty);
/// assert_eq!(report.violations.len(), 1);
/// assert_eq!(report.violations[0].file_path, "crypto.rs");
/// ```
pub fn diagnose_domain_purity(files: &[(String, String)]) -> DomainPurityReport {
    let mut violations = Vec::new();
    for (file_path, content) in files {
        violations.extend(scan_domain_purity(file_path, content));
    }
    DomainPurityReport { files_scanned: files.len(), violations }
}

/// A single detected scanner/deleter policy violation.
#[derive(Debug, Clone)]
pub struct PolicyViolation {
    pub file_path: String,
    pub line_number: usize,
    pub matched_pattern: String,
}

/// Report returned by `diagnose_scan_delete_separation`
#[derive(Debug, Clone)]
pub struct ScanDeleteSeparationReport {
    pub files_scanned: usize,
    pub violations: Vec<PolicyViolation>,
    pub plan_file_param_found: bool,
}

/// Function-call patterns that indicate the deleter is invoking a live
/// filesystem scan, violating "the scanner cannot delete; the deleter
/// cannot scan".
const FORBIDDEN_SCAN_CALLS: &[&str] = &[
    "scan_root(",
    "audit_run(",
    "run_audit(",
    "scan_filesystem(",
    "walk_filesystem(",
    "audit_scan(",
];

/// Scans delete-path file content for forbidden calls into scanning entry
/// points, skipping explicitly allow-listed lines.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::scan_for_forbidden_scan_calls;
///
/// // Positive case: no violations
/// let clean = scan_for_forbidden_scan_calls("delete.rs", "pub fn delete_execute() {}");
/// assert!(clean.is_empty());
///
/// // Negative case: a scan call from the delete path is flagged
/// let dirty = scan_for_forbidden_scan_calls("delete.rs", "let x = scan_root(path);\n");
/// assert_eq!(dirty.len(), 1);
///
/// // Refusal case: explicitly allow-listed line is not flagged
/// let allowed = scan_for_forbidden_scan_calls(
///     "delete.rs",
///     "let x = scan_root(path); // doctor-allow: scan-delete-separation\n",
/// );
/// assert!(allowed.is_empty());
/// ```
pub fn scan_for_forbidden_scan_calls(file_path: &str, content: &str) -> Vec<PolicyViolation> {
    let mut violations = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if line.contains("doctor-allow: scan-delete-separation") {
            continue;
        }
        for pattern in FORBIDDEN_SCAN_CALLS {
            if line.contains(pattern) {
                violations.push(PolicyViolation {
                    file_path: file_path.to_string(),
                    line_number: i + 1,
                    matched_pattern: (*pattern).to_string(),
                });
                break;
            }
        }
    }

    violations
}

/// Diagnoses the "scanner cannot delete; deleter cannot scan" invariant by
/// checking the delete-execute code paths for forbidden calls into scan/audit
/// entry points, and verifying the delete path reads from a plan-file
/// parameter (`plan_file`) rather than performing a live directory walk.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::diagnose_scan_delete_separation;
///
/// // Positive case: clean delete path, reads from plan_file
/// let files = vec![(
///     "delete.rs".to_string(),
///     "pub fn delete_execute(plan_file: &str) {}".to_string(),
/// )];
/// let report = diagnose_scan_delete_separation(&files);
/// assert!(report.violations.is_empty());
/// assert!(report.plan_file_param_found);
///
/// // Negative case: delete path calls a scan entry point
/// let dirty = vec![(
///     "delete.rs".to_string(),
///     "pub fn delete_execute() { scan_root(root); }".to_string(),
/// )];
/// let report = diagnose_scan_delete_separation(&dirty);
/// assert_eq!(report.violations.len(), 1);
///
/// // Refusal case: no delete files supplied still reports plan_file as not found
/// let report = diagnose_scan_delete_separation(&[]);
/// assert!(!report.plan_file_param_found);
/// ```
pub fn diagnose_scan_delete_separation(files: &[(String, String)]) -> ScanDeleteSeparationReport {
    let mut violations = Vec::new();
    let mut plan_file_param_found = false;

    for (file_path, content) in files {
        violations.extend(scan_for_forbidden_scan_calls(file_path, content));
        if content.contains("plan_file") || content.contains("plan_path") {
            plan_file_param_found = true;
        }
    }

    ScanDeleteSeparationReport { files_scanned: files.len(), violations, plan_file_param_found }
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

/// Facts gathered (integration/noun side) about one launchd job of the
/// unattended autonomy stack. Pure data — no I/O happens here.
#[derive(Debug, Clone)]
pub struct DaemonJobFacts {
    /// launchd label, e.g. `com.oclnr.autoclean`.
    pub label: String,
    /// Whether the job's plist exists under `~/Library/LaunchAgents`.
    pub plist_exists: bool,
    /// The binary path baked into the plist's `ProgramArguments`, if it
    /// could be parsed.
    pub plist_binary: Option<String>,
    /// Whether that baked path exists on disk right now (a moved or deleted
    /// binary leaves a job that fails silently on every fire).
    pub binary_exists: bool,
    /// Whether launchd currently has the job loaded.
    pub launchd_loaded: bool,
}

/// Adjudicated finding for one job: empty `issues` means healthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonJobFinding {
    pub label: String,
    pub issues: Vec<String>,
}

/// Report returned by [`diagnose_daemon_health`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonHealthReport {
    pub jobs: Vec<DaemonJobFinding>,
    /// Consecutive autoclean failures parsed from the run log (0 if none).
    pub consecutive_failures: usize,
    /// Non-blocking advisories (e.g. last run was refused/skipped and needs
    /// human review, or the stack has never run).
    pub advisories: Vec<String>,
}

impl DaemonHealthReport {
    /// Total blocking issues across all jobs plus the failure streak.
    pub fn total_issues(&self) -> usize {
        let job_issues: usize = self.jobs.iter().map(|j| j.issues.len()).sum();
        job_issues + if self.consecutive_failures >= 2 { 1 } else { 0 }
    }
}

/// Adjudicates the autonomy stack's health from gathered facts.
///
/// A job is healthy when its plist exists, the binary baked into that plist
/// still exists, and launchd has the job loaded. An installed job with a
/// missing binary or an unloaded plist is a blocking issue (the job fails
/// silently forever otherwise); a job that is simply not installed is an
/// advisory, not an issue — installing the monitor is optional. A run
/// history with ≥2 consecutive autoclean failures is one blocking issue;
/// a last run of `refused`/`skipped` is an advisory (it needs human review
/// but the pipeline behaved correctly).
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::doctor::{diagnose_daemon_health, DaemonJobFacts};
///
/// // Positive: installed, binary present, loaded — no issues.
/// let healthy = vec![DaemonJobFacts {
///     label: "com.oclnr.autoclean".into(),
///     plist_exists: true,
///     plist_binary: Some("/usr/local/bin/oclnr".into()),
///     binary_exists: true,
///     launchd_loaded: true,
/// }];
/// let report = diagnose_daemon_health(healthy, 0, None);
/// assert_eq!(report.total_issues(), 0);
/// assert!(report.advisories.is_empty());
///
/// // Refusal: a loaded job whose baked binary vanished is silently dead —
/// // one blocking issue; and a 3-failure streak adds the standing issue.
/// let broken = vec![DaemonJobFacts {
///     label: "com.oclnr.autoclean".into(),
///     plist_exists: true,
///     plist_binary: Some("/usr/local/bin/oclnr".into()),
///     binary_exists: false,
///     launchd_loaded: true,
/// }];
/// let report = diagnose_daemon_health(broken, 3, None);
/// assert_eq!(report.total_issues(), 2);
/// assert!(report.jobs[0].issues.iter().any(|i| i.contains("does not exist")));
///
/// // Advisory, not issue: a refused last run needs review but behaved.
/// let report = diagnose_daemon_health(Vec::new(), 0, Some("refused".into()));
/// assert_eq!(report.total_issues(), 0);
/// assert!(report.advisories.iter().any(|a| a.contains("REFUSED")));
/// ```
pub fn diagnose_daemon_health(
    jobs: Vec<DaemonJobFacts>,
    consecutive_failures: usize,
    last_outcome: Option<String>,
) -> DaemonHealthReport {
    let mut findings = Vec::new();
    let mut advisories = Vec::new();

    for job in jobs {
        let mut issues = Vec::new();
        if !job.plist_exists {
            advisories.push(format!("{}: not installed (no plist)", job.label));
        } else {
            if !job.launchd_loaded {
                issues.push("plist exists but the job is not loaded with launchd".to_string());
            }
            match &job.plist_binary {
                Some(binary) if !job.binary_exists => {
                    issues.push(format!(
                        "configured binary '{binary}' does not exist — the job fails \
                         silently on every fire"
                    ));
                }
                Some(_) => {}
                None => issues.push("plist ProgramArguments could not be parsed".to_string()),
            }
        }
        findings.push(DaemonJobFinding { label: job.label, issues });
    }

    if consecutive_failures >= 2 {
        advisories.push(format!(
            "{consecutive_failures} consecutive autoclean failures — run the stages \
             manually to see the error"
        ));
    }
    match last_outcome.as_deref() {
        Some("refused") => advisories
            .push("last autoclean run was REFUSED (cap backstop) — review the plan".to_string()),
        Some("skipped") => advisories.push(
            "last autoclean run was SKIPPED (unknown-reversibility items) — review the plan"
                .to_string(),
        ),
        _ => {}
    }

    DaemonHealthReport { jobs: findings, consecutive_failures, advisories }
}
