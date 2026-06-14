//! GitHub command integration layer.
//!
//! Exposes functions to execute `gh` CLI commands and parse their outputs,
//! abstracting standard system process execution behind a `CommandExecutor` trait.

use crate::domain::github::{GhRepo, GhRun, GhBranchListRef, GhCompareResponse, GhRelease};
use std::io;
use std::process::Output;

/// Abstract executor for external shell commands.
pub trait CommandExecutor: Send + Sync {
    /// Executes a program with the given arguments.
    fn execute(&self, program: &str, args: &[&str]) -> io::Result<Output>;
}

/// Production implementation of `CommandExecutor` running system processes.
pub struct RealCommandExecutor;

impl CommandExecutor for RealCommandExecutor {
    fn execute(&self, program: &str, args: &[&str]) -> io::Result<Output> {
        std::process::Command::new(program).args(args).output()
    }
}

/// Helper to run a `gh` subcommand and parse its JSON output.
fn run_gh_parsed<T: serde::de::DeserializeOwned>(
    executor: &dyn CommandExecutor,
    args: &[&str],
) -> anyhow::Result<T> {
    let output = executor.execute("gh", args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh command failed: {}", stderr.trim());
    }
    let val = serde_json::from_slice(&output.stdout)?;
    Ok(val)
}

/// Helper to run a `gh` subcommand and check its status.
fn run_gh_status(
    executor: &dyn CommandExecutor,
    args: &[&str],
) -> anyhow::Result<()> {
    let output = executor.execute("gh", args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh command failed: {}", stderr.trim());
    }
    Ok(())
}

/// Lists all repositories for the authenticated user.
pub fn list_repositories(executor: &dyn CommandExecutor) -> anyhow::Result<Vec<GhRepo>> {
    run_gh_parsed(
        executor,
        &[
            "repo",
            "list",
            "--limit",
            "1000",
            "--json",
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility",
        ],
    )
}

/// Lists workflow runs for a specific repository.
pub fn list_runs(executor: &dyn CommandExecutor, owner: &str, repo: &str) -> anyhow::Result<Vec<GhRun>> {
    let repo_arg = format!("{}/{}", owner, repo);
    run_gh_parsed(
        executor,
        &[
            "run",
            "list",
            "--repo",
            &repo_arg,
            "--limit",
            "100",
            "--json",
            "databaseId,number,name,headBranch,status,conclusion,createdAt,updatedAt",
        ],
    )
}

/// Lists branches for a specific repository.
pub fn list_branches(executor: &dyn CommandExecutor, owner: &str, repo: &str) -> anyhow::Result<Vec<GhBranchListRef>> {
    let api_path = format!("repos/{}/{}/branches", owner, repo);
    run_gh_parsed(executor, &["api", &api_path])
}

/// Compares a branch with a default branch in a repository.
pub fn compare_branch(
    executor: &dyn CommandExecutor,
    owner: &str,
    repo: &str,
    default_branch: &str,
    branch: &str,
) -> anyhow::Result<GhCompareResponse> {
    let api_path = format!("repos/{}/{}/compare/{}...{}", owner, repo, default_branch, branch);
    run_gh_parsed(executor, &["api", &api_path])
}

/// Lists releases in a repository.
pub fn list_releases(executor: &dyn CommandExecutor, owner: &str, repo: &str) -> anyhow::Result<Vec<GhRelease>> {
    let repo_arg = format!("{}/{}", owner, repo);
    run_gh_parsed(
        executor,
        &[
            "release",
            "list",
            "--repo",
            &repo_arg,
            "--limit",
            "100",
            "--json",
            "tagName,name,isDraft,isPrerelease,createdAt,publishedAt",
        ],
    )
}

/// Deletes a GitHub repository.
pub fn delete_repository(executor: &dyn CommandExecutor, owner: &str, repo: &str) -> anyhow::Result<()> {
    let repo_arg = format!("{}/{}", owner, repo);
    run_gh_status(executor, &["repo", "delete", &repo_arg, "--confirm"])
}

/// Deletes a git branch ref from a repository.
pub fn delete_branch(executor: &dyn CommandExecutor, owner: &str, repo: &str, branch: &str) -> anyhow::Result<()> {
    let api_path = format!("repos/{}/{}/git/refs/heads/{}", owner, repo, branch);
    run_gh_status(executor, &["api", "-X", "DELETE", &api_path])
}

/// Deletes a workflow run from a repository.
pub fn delete_run(executor: &dyn CommandExecutor, owner: &str, repo: &str, run_id: u64) -> anyhow::Result<()> {
    let repo_arg = format!("{}/{}", owner, repo);
    run_gh_status(executor, &["run", "delete", &run_id.to_string(), "--repo", &repo_arg])
}

/// Deletes a release and optionally its tag from a repository.
pub fn delete_release(executor: &dyn CommandExecutor, owner: &str, repo: &str, tag: &str) -> anyhow::Result<()> {
    let repo_arg = format!("{}/{}", owner, repo);
    run_gh_status(executor, &["release", "delete", tag, "--repo", &repo_arg, "--yes", "--cleanup-tag"])
}

pub struct MockCommandExecutor {
    pub calls: std::sync::Mutex<Vec<Vec<String>>>,
    pub responses: std::sync::Mutex<std::collections::HashMap<String, (i32, String, String)>>,
}

impl MockCommandExecutor {
    pub fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            responses: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn add_response(&self, program: &str, args: &[&str], status: i32, stdout: &str, stderr: &str) {
        let key = format!("{} {}", program, args.join(" "));
        self.responses.lock().unwrap().insert(
            key,
            (status, stdout.to_string(), stderr.to_string()),
        );
    }

    pub fn get_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandExecutor for MockCommandExecutor {
    fn execute(&self, program: &str, args: &[&str]) -> io::Result<Output> {
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().unwrap().push(
            std::iter::once(program.to_string()).chain(args_vec).collect()
        );

        let key = format!("{} {}", program, args.join(" "));
        if let Some((status, stdout, stderr)) = self.responses.lock().unwrap().get(&key) {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                Ok(Output {
                    status: std::process::ExitStatus::from_raw(*status),
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: stderr.as_bytes().to_vec(),
                })
            }
            #[cfg(not(unix))]
            {
                // Basic mock implementation for non-unix testing environments
                Ok(Output {
                    status: unsafe { std::mem::transmute(*status) },
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: stderr.as_bytes().to_vec(),
                })
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("No mock response registered for: {}", key),
            ))
        }
    }
}

