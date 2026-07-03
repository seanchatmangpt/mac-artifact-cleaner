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

/// Subprocess runner
pub struct OclnrRunner {
    oclnr_path: PathBuf,
}

impl OclnrRunner {
    /// Create new runner, attempting to locate oclnr binary
    #[allow(clippy::result_large_err)]
    pub fn new() -> Result<Self, ErrorResponse> {
        let oclnr_path = which::which("oclnr")
            .or_else(|_| {
                // Try relative path for development
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|mut p| {
                        p.push("oclnr");
                        p
                    })
                    .ok_or_else(|| "oclnr not found")
            })
            .map_err(|e| {
                ErrorResponse::new(
                    ErrorCode::SubprocessFailed,
                    format!("Could not locate oclnr binary: {}", e),
                )
                .with_suggestions(vec![
                    "Install oclnr: cargo install --path .".to_string(),
                    "Or ensure oclnr is in PATH".to_string(),
                ])
            })?;

        Ok(Self { oclnr_path })
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
        if ignore_recent_hours > 0 {
            cmd.arg("--ignore-recent-hours").arg(ignore_recent_hours.to_string());
        }
        if tool_roots {
            cmd.arg("--tool-roots");
        }

        // Write OCEL output to disk-audit.jsonocel
        cmd.arg("--ocel-output").arg(workspace.join("disk-audit.jsonocel"));

        self.run_command(cmd, "oclnr audit run")
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

        self.run_command(cmd, "oclnr delete execute")
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
}
