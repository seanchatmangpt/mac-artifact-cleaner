//! Integration layer for the doctor command.
//! All filesystem reads and process spawner commands live here.

use std::path::Path;
use std::process::Command;

/// Checks the existence of key files and directories in the workspace.
pub fn read_workspace_architecture(workspace_root: &Path) -> (bool, bool, Vec<(String, bool)>, Vec<(String, bool)>) {
    let agents_md_exists = workspace_root.join("AGENTS.md").exists();
    let cargo_toml_exists = workspace_root.join("Cargo.toml").exists();

    let expected_dirs = ["src", "src/domain", "src/nouns", "src/integration", "tests"];
    let mut main_dirs_exist = Vec::new();
    for dir in &expected_dirs {
        let exists = workspace_root.join(dir).is_dir();
        main_dirs_exist.push((dir.to_string(), exists));
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
    }

    (agents_md_exists, cargo_toml_exists, main_dirs_exist, nouns_files_exist)
}

/// Checks the substrate environment.
pub fn query_substrate_info() -> (Option<String>, bool) {
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

    (tmutil_path, command_execution_works)
}

/// Reads the file contents of all domain files for doctest checks.
pub fn read_doctest_files(workspace_root: &Path) -> Vec<(String, String)> {
    let domain_dir = workspace_root.join("src/domain");
    let mut out = Vec::new();

    if domain_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(domain_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        out.push((file_name, content));
                    }
                }
            }
        }
    }
    out
}

/// Reads all relevant repository files for privacy checking.
pub fn read_privacy_files(
    workspace_root: &Path,
) -> (bool, Option<String>, Vec<String>, Vec<(String, String)>) {
    let gitignore_path = workspace_root.join(".gitignore");
    let gitignore_exists = gitignore_path.exists();
    let gitignore_content = if gitignore_exists {
        std::fs::read_to_string(&gitignore_path).ok()
    } else {
        None
    };

    let mut found_sensitive_files = Vec::new();
    let mut files_to_scan = Vec::new();

    fn traverse(
        dir: &Path,
        sensitive_files: &mut Vec<String>,
        files_to_scan: &mut Vec<(String, String)>,
    ) {
        if let Ok(entries) = std::fs::read_dir(dir) {
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
                    traverse(&path, sensitive_files, files_to_scan);
                } else if path.is_file() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

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
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                files_to_scan.push((path.to_string_lossy().to_string(), content));
                            }
                        }
                    }
                }
            }
        }
    }

    traverse(workspace_root, &mut found_sensitive_files, &mut files_to_scan);

    (
        gitignore_exists,
        gitignore_content,
        found_sensitive_files,
        files_to_scan,
    )
}
