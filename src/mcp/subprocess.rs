//! Subprocess runner for oclnr commands
//!
//! Spawns oclnr as a subprocess and handles output parsing.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::Value;

use super::error::{ErrorCode, ErrorResponse};

/// Result of subprocess execution
#[derive(Debug, Clone)]
pub struct SubprocessResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl SubprocessResult {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    pub fn to_error(&self, cmd: &str) -> ErrorResponse {
        ErrorResponse::subprocess_failed(cmd, &self.stderr)
    }
}

/// Resolve the `oclnr` binary path used by [`OclnrRunner::new`].
///
/// Priority order:
/// 1. `env_override` (from `OCLNR_BIN`), if it points at an existing file.
/// 2. A file named `oclnr` next to `current_exe`'s parent directory, if it
///    exists — i.e. co-located with the running `oclnr-mcp` binary.
/// 3. Whatever `which_lookup("oclnr")` returns (a `PATH` search).
///
/// Co-located resolution is checked *before* `PATH` so that a stale `oclnr`
/// earlier on `PATH` (e.g. an old install in `~/.cargo/bin`) can never
/// shadow the binary that was built/installed alongside `oclnr-mcp` itself.
fn resolve_oclnr_path(
    env_override: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    which_lookup: impl Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(p) = env_override {
        if p.is_file() {
            return Some(p);
        }
    }

    if let Some(colocated) = current_exe.and_then(|exe| exe.parent().map(|p| p.join("oclnr"))) {
        if colocated.is_file() {
            return Some(colocated);
        }
    }

    which_lookup("oclnr")
}

/// Subprocess runner
pub struct OclnrRunner {
    oclnr_path: PathBuf,
}

impl OclnrRunner {
    /// Create new runner, attempting to locate oclnr binary.
    ///
    /// Resolution order (most to least specific), so that a stale `oclnr`
    /// earlier on `PATH` can never silently shadow a co-located, freshly
    /// built binary:
    ///
    /// 1. `OCLNR_BIN` environment variable, if set — explicit pin for
    ///    deployments that want to bypass discovery entirely.
    /// 2. A binary named `oclnr` next to the currently running executable
    ///    (`std::env::current_exe()?.parent()/oclnr`) — this is where the
    ///    `oclnr-mcp` binary itself was built/installed, so it is the most
    ///    likely to be in sync with the code that spawns it.
    /// 3. `PATH` lookup via `which::which("oclnr")` — last resort, since a
    ///    PATH entry may point at an older, independently installed copy.
    #[allow(clippy::result_large_err)]
    pub fn new() -> Result<Self, ErrorResponse> {
        let oclnr_path = resolve_oclnr_path(
            std::env::var_os("OCLNR_BIN").map(PathBuf::from),
            std::env::current_exe().ok(),
            |name| which::which(name).ok(),
        )
        .ok_or_else(|| {
            ErrorResponse::new(
                ErrorCode::SubprocessFailed,
                "Could not locate oclnr binary: not found via OCLNR_BIN, co-located \
                 with current executable, or PATH"
                    .to_string(),
            )
            .with_suggestions(vec![
                "Install oclnr: cargo install --path .".to_string(),
                "Or ensure oclnr is in PATH".to_string(),
                "Or set OCLNR_BIN to an explicit binary path".to_string(),
            ])
        })?;

        Ok(Self { oclnr_path })
    }

    /// Build a runner that invokes a specific binary rather than resolving
    /// `oclnr` from `PATH`. Exists so integration tests can point the
    /// runner at a stub executable and inspect the exact argv this wrapper
    /// sends, without needing a real `oclnr` install or performing a live
    /// deletion.
    pub fn with_binary_path(oclnr_path: PathBuf) -> Self {
        Self { oclnr_path }
    }

    /// Run: oclnr audit run [roots...] [options]
    #[allow(clippy::result_large_err)]
    pub fn audit_run(
        &self,
        workspace: &PathBuf,
        roots: Vec<PathBuf>,
        include_deps: bool,
        include_aggressive: bool,
        ignore_recent_hours: u32,
        tool_roots: bool,
        all_filesystems: bool,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("audit")
            .arg("run")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add roots
        for root in roots {
            cmd.arg("--root").arg(root.to_string_lossy().to_string());
        }

        // Add options
        if include_deps {
            cmd.arg("--deps");
        }
        if include_aggressive {
            cmd.arg("--aggressive");
        }
        // Always forward the value: 0 is a meaningful override ("disable the
        // recency guard entirely"), not "unset". Gating this on `> 0` would
        // silently fall back to the CLI's own 168h default whenever a caller
        // asked to disable the guard.
        cmd.arg("--ignore-recent-hours").arg(ignore_recent_hours.to_string());
        if tool_roots {
            cmd.arg("--tool-roots");
        }
        if all_filesystems {
            cmd.arg("--all-filesystems");
        }

        // Write OCEL output to disk-audit.jsonocel
        cmd.arg("--ocel-output").arg(workspace.join("disk-audit.jsonocel"));

        self.run_command(cmd, "oclnr audit run")
    }

    /// Run: oclnr audit breakdown --root <root> --depth <depth> --top <top> --min-mb <min_mb> --json
    ///
    /// Unlike `audit_run`, this walks every byte under `root` (hidden dirs
    /// included, no artifact-specific pruning) instead of only surfacing
    /// deletion candidates — it's the tool for "where did the disk space
    /// actually go", not "what can I delete".
    #[allow(clippy::result_large_err)]
    pub fn audit_breakdown(
        &self,
        workspace: &PathBuf,
        root: &PathBuf,
        depth: u32,
        top: usize,
        min_mb: u64,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("audit")
            .arg("breakdown")
            .arg("--root")
            .arg(root)
            .arg("--depth")
            .arg(depth.to_string())
            .arg("--top")
            .arg(top.to_string())
            .arg("--min-mb")
            .arg(min_mb.to_string())
            .arg("--json")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr audit breakdown")
    }

    /// Run: oclnr plan build --root ... --output cleanup-plan.json [options]
    ///
    /// `plan build` re-scans (it does not read a saved audit file); the audit
    /// is used upstream to decide roots/flags and to gate the workflow state.
    #[allow(clippy::result_large_err)]
    pub fn plan_create(
        &self,
        workspace: &PathBuf,
        roots: Vec<PathBuf>,
        deps: bool,
        aggressive: bool,
        include_global_caches: bool,
        ignore_recent_hours: u32,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let output = workspace.join("cleanup-plan.json");
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("plan")
            .arg("build")
            .arg("--output")
            .arg(&output)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for root in roots {
            cmd.arg("--root").arg(root);
        }
        if deps {
            cmd.arg("--deps");
        }
        if aggressive {
            cmd.arg("--aggressive");
        }
        if include_global_caches {
            cmd.arg("--include-global-caches");
        }
        // Always forward: 0 is a meaningful override ("disable the recency
        // guard"), not "unset". See audit_run for the same rationale.
        cmd.arg("--ignore-recent-hours").arg(ignore_recent_hours.to_string());

        self.run_command(cmd, "oclnr plan build")
    }

    /// Run: oclnr delete execute --plan cleanup-plan.json --receipt <receipt> [--yes]
    ///
    /// The CLI verb is `execute` (not `run`), and `--receipt` is a required
    /// argument even for a dry-run preview (the CLI simply returns before
    /// writing it when `--yes` is absent).
    #[allow(clippy::result_large_err)]
    pub fn delete_run(
        &self,
        workspace: &PathBuf,
        plan_file: &PathBuf,
        receipt_file: &PathBuf,
        confirm: bool,
        max_concurrent: Option<usize>,
        timeout_secs: u32,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("delete")
            .arg("execute")
            .arg("--plan")
            .arg(plan_file)
            .arg("--receipt")
            .arg(receipt_file)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if confirm {
            cmd.arg("--yes");
        }
        if let Some(n) = max_concurrent {
            cmd.arg("--max-concurrent").arg(n.to_string());
        }

        self.run_command_with_timeout(cmd, "oclnr delete execute", timeout_secs)
    }

    /// Run: oclnr receipt verify [--receipt receipt-file]
    #[allow(clippy::result_large_err)]
    pub fn receipt_verify(
        &self,
        workspace: &PathBuf,
        receipt_file: Option<&PathBuf>,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("receipt")
            .arg("verify")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(receipt) = receipt_file {
            cmd.arg("--receipt").arg(receipt);
        }

        self.run_command(cmd, "oclnr receipt verify")
    }

    /// Run: oclnr receipt certify --receipt <receipt> [--out <out>]
    #[allow(clippy::result_large_err)]
    pub fn receipt_certify(
        &self,
        workspace: &PathBuf,
        receipt_file: &PathBuf,
        out_file: Option<&PathBuf>,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("receipt")
            .arg("certify")
            .arg("--receipt")
            .arg(receipt_file)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(out) = out_file {
            cmd.arg("--out").arg(out);
        }

        self.run_command(cmd, "oclnr receipt certify")
    }

    /// Run: oclnr emergency --mount <mount> [--yes] [--receipt <receipt>]
    #[allow(clippy::result_large_err)]
    pub fn emergency_reclaim(
        &self,
        workspace: &PathBuf,
        mount: &str,
        confirm: bool,
        receipt_file: Option<&PathBuf>,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("emergency")
            .arg("--mount")
            .arg(mount)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if confirm {
            cmd.arg("--yes");
        }
        if let Some(receipt) = receipt_file {
            cmd.arg("--receipt").arg(receipt);
        }

        self.run_command(cmd, "oclnr emergency")
    }

    /// Run: oclnr docker scan
    #[allow(clippy::result_large_err)]
    pub fn docker_scan(&self, workspace: &PathBuf) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("docker")
            .arg("scan")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr docker scan")
    }

    /// Run: oclnr docker plan
    #[allow(clippy::result_large_err)]
    pub fn docker_plan(&self, workspace: &PathBuf) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("docker")
            .arg("plan")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr docker plan")
    }

    /// Run: oclnr docker prune --confirm [--skip-colima]
    #[allow(clippy::result_large_err)]
    pub fn docker_prune(
        &self,
        workspace: &PathBuf,
        skip_colima: bool,
        receipt_file: Option<&PathBuf>,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("docker")
            .arg("prune")
            .arg("--confirm")
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if skip_colima {
            cmd.arg("--skip-colima");
        }
        if let Some(r) = receipt_file {
            cmd.arg("--receipt").arg(r);
        }

        self.run_command(cmd, "oclnr docker prune")
    }

    /// Run: oclnr snapshot audit --mount <mount>
    ///
    /// `snapshot audit` takes a single `--mount` flag (default `/`), not a
    /// list of positional roots — the first caller-supplied root (if any) is
    /// used as the mount point.
    #[allow(clippy::result_large_err)]
    pub fn snapshot_audit(
        &self,
        workspace: &PathBuf,
        roots: Vec<PathBuf>,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mount = roots
            .first()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("snapshot")
            .arg("audit")
            .arg("--mount")
            .arg(mount)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr snapshot audit")
    }

    /// Run: oclnr snapshot thin --mount <mount> --bytes <bytes> --receipt <receipt_file>
    #[allow(clippy::result_large_err)]
    pub fn snapshot_thin(
        &self,
        workspace: &PathBuf,
        mount: &str,
        bytes: &str,
        receipt_file: &PathBuf,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("snapshot")
            .arg("thin")
            .arg("--mount")
            .arg(mount)
            .arg("--bytes")
            .arg(bytes)
            .arg("--receipt")
            .arg(receipt_file)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr snapshot thin")
    }

    /// Run: oclnr snapshot delete --mount <mount> --which <which> --oldest-n <oldest_n> --receipt <receipt_file>
    #[allow(clippy::result_large_err)]
    pub fn snapshot_delete(
        &self,
        workspace: &PathBuf,
        mount: &str,
        which: &str,
        oldest_n: usize,
        receipt_file: &PathBuf,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = Command::new(&self.oclnr_path);
        cmd.arg("snapshot")
            .arg("delete")
            .arg("--mount")
            .arg(mount)
            .arg("--which")
            .arg(which)
            .arg("--oldest-n")
            .arg(oldest_n.to_string())
            .arg("--receipt")
            .arg(receipt_file)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.run_command(cmd, "oclnr snapshot delete")
    }

    /// Run: oclnr doctor architecture
    #[allow(clippy::result_large_err)]
    pub fn doctor_check(
        &self,
        workspace: &PathBuf,
        check: &str,
    ) -> Result<SubprocessResult, ErrorResponse> {
        let cmd = Command::new(&self.oclnr_path)
            .arg("doctor")
            .arg(check)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ErrorResponse::subprocess_failed("oclnr doctor", &e.to_string()))?;

        let output = cmd
            .wait_with_output()
            .map_err(|e| ErrorResponse::subprocess_failed("oclnr doctor", &e.to_string()))?;

        Ok(SubprocessResult {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Generic command runner
    #[allow(clippy::result_large_err)]
    fn run_command(&self, cmd: Command, name: &str) -> Result<SubprocessResult, ErrorResponse> {
        let mut cmd = cmd;
        let output =
            cmd.output().map_err(|e| ErrorResponse::subprocess_failed(name, &e.to_string()))?;

        Ok(SubprocessResult {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Like [`Self::run_command`], but kills the child and returns an error
    /// if it hasn't exited within `timeout_secs`.
    ///
    /// `Command::output()` (used by `run_command`) blocks indefinitely —
    /// there is no way to bound it. This spawns instead and polls
    /// `try_wait()`, so a real timeout previously advertised in the
    /// `delete` MCP tool's schema (`timeout_secs`) but never enforced now
    /// actually bounds how long a hung deletion subprocess can block.
    #[allow(clippy::result_large_err)]
    fn run_command_with_timeout(
        &self,
        mut cmd: Command,
        name: &str,
        timeout_secs: u32,
    ) -> Result<SubprocessResult, ErrorResponse> {
        use std::io::Read;

        let mut child =
            cmd.spawn().map_err(|e| ErrorResponse::subprocess_failed(name, &e.to_string()))?;

        // Drain stdout/stderr on background threads while we poll for exit
        // below — `try_wait()` alone doesn't read the pipes, and on macOS a
        // full ~64KB pipe buffer would otherwise deadlock the child against
        // an OS write() that never returns while we're just waiting.
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let stdout_thread = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut p) = stdout_pipe.take() {
                let _ = p.read_to_string(&mut buf);
            }
            buf
        });
        let stderr_thread = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut p) = stderr_pipe.take() {
                let _ = p.read_to_string(&mut buf);
            }
            buf
        });

        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        // Best-effort: if kill() itself fails (process
                        // already gone in a race), that's fine — we're
                        // about to report the timeout either way.
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(ErrorResponse::subprocess_failed(
                            name,
                            &format!("timed out after {timeout_secs}s and was killed"),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(ErrorResponse::subprocess_failed(name, &e.to_string()));
                }
            }
        };

        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        Ok(SubprocessResult { status: status.code().unwrap_or(-1), stdout, stderr })
    }
}

/// Parse JSON output from oclnr subprocess
#[allow(clippy::result_large_err)]
pub fn parse_json_output(output: &str) -> Result<Value, ErrorResponse> {
    serde_json::from_str(output).map_err(|e| {
        ErrorResponse::json_parse_error(&e.to_string()).with_context(serde_json::json!({
            "output_preview": &output[..std::cmp::min(200, output.len())]
        }))
    })
}

/// Parse JSONOCEL output from oclnr subprocess
#[allow(clippy::result_large_err)]
pub fn parse_jsonocel_output(output: &str) -> Result<Value, ErrorResponse> {
    // JSONOCEL is line-delimited JSON
    let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.is_empty() {
        return Ok(serde_json::json!({"objects": [], "events": []}));
    }

    let mut all_objects = Vec::new();
    let mut all_events = Vec::new();

    for line in lines {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if value.get("objectType").is_some() {
                all_objects.push(value);
            } else if value.get("eventType").is_some() {
                all_events.push(value);
            }
        }
    }

    Ok(serde_json::json!({
        "objects": all_objects,
        "events": all_events
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subprocess_result_success() {
        let result =
            SubprocessResult { status: 0, stdout: "success".to_string(), stderr: String::new() };
        assert!(result.success());
    }

    #[test]
    fn test_subprocess_result_failure() {
        let result =
            SubprocessResult { status: 1, stdout: String::new(), stderr: "error".to_string() };
        assert!(!result.success());
    }

    #[test]
    fn test_parse_json_output() {
        let output = r#"{"key": "value"}"#;
        let result = parse_json_output(output);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_json_output_error() {
        let output = "invalid json";
        let result = parse_json_output(output);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_jsonocel_output() {
        let output = r#"{"objectType": "test", "id": "1"}
{"eventType": "test", "id": "1"}"#;
        let result = parse_jsonocel_output(output);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["objects"].as_array().unwrap().len(), 1);
        assert_eq!(val["events"].as_array().unwrap().len(), 1);
    }

    /// Write a trivial executable stub at `path` that just echoes its own
    /// path when run, so tests can tell which stub actually got resolved.
    fn write_stub(path: &std::path::Path) {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "echo STUB:{}", path.display()).unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Regression test for the PATH-shadows-co-located-binary bug: when a
    /// stub exists both on a fake `PATH` directory and co-located with a
    /// fake `current_exe`, the co-located one must win.
    #[test]
    fn test_resolve_prefers_colocated_binary_over_path() {
        let tmp = std::env::temp_dir().join(format!(
            "oclnr_subprocess_resolve_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let exe_dir = tmp.join("exe_dir");
        let path_dir = tmp.join("path_dir");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();

        let colocated = exe_dir.join("oclnr");
        let on_path = path_dir.join("oclnr");
        write_stub(&colocated);
        write_stub(&on_path);

        // Fake `current_exe`: a file living in exe_dir (its parent is what
        // matters for co-location, the file itself need not be executable).
        let fake_current_exe = exe_dir.join("oclnr-mcp");
        std::fs::write(&fake_current_exe, b"fake").unwrap();

        let on_path_clone = on_path.clone();
        let resolved = resolve_oclnr_path(None, Some(fake_current_exe), move |name| {
            assert_eq!(name, "oclnr");
            Some(on_path_clone.clone())
        });

        assert_eq!(
            resolved,
            Some(colocated),
            "co-located binary must be preferred over a PATH (which::which) hit"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_resolve_falls_back_to_path_when_no_colocated_binary() {
        let tmp = std::env::temp_dir().join(format!(
            "oclnr_subprocess_resolve_test_fallback_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let exe_dir = tmp.join("exe_dir_empty");
        let path_dir = tmp.join("path_dir");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();

        let on_path = path_dir.join("oclnr");
        write_stub(&on_path);

        let fake_current_exe = exe_dir.join("oclnr-mcp");
        std::fs::write(&fake_current_exe, b"fake").unwrap();

        let on_path_clone = on_path.clone();
        let resolved =
            resolve_oclnr_path(None, Some(fake_current_exe), move |_| Some(on_path_clone.clone()));

        assert_eq!(resolved, Some(on_path));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_resolve_env_override_wins_over_everything() {
        let tmp = std::env::temp_dir().join(format!(
            "oclnr_subprocess_resolve_test_override_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let exe_dir = tmp.join("exe_dir");
        let override_dir = tmp.join("override_dir");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&override_dir).unwrap();

        let colocated = exe_dir.join("oclnr");
        let overridden = override_dir.join("oclnr-custom");
        write_stub(&colocated);
        write_stub(&overridden);

        let fake_current_exe = exe_dir.join("oclnr-mcp");
        std::fs::write(&fake_current_exe, b"fake").unwrap();

        let resolved = resolve_oclnr_path(Some(overridden.clone()), Some(fake_current_exe), |_| {
            panic!("which_lookup should not be called when OCLNR_BIN override is valid")
        });

        assert_eq!(resolved, Some(overridden));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
