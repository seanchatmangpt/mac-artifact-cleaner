//! Rust toolchain and language package manager analysis.

use std::process::Command;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustToolchain {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainScanResult {
    pub toolchains: Vec<RustToolchain>,
    pub rustup_home_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmGlobalPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipPackage {
    pub name: String,
    pub version: String,
}

/// Returns true if `rustup` is available on PATH.
pub fn rustup_available() -> bool {
    which::which("rustup").is_ok()
}

/// Lists installed Rust toolchains and the size of ~/.rustup.
pub fn list_rust_toolchains() -> Result<ToolchainScanResult> {
    let output = Command::new("rustup").args(["toolchain", "list"]).output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let toolchains: Vec<RustToolchain> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let is_default = line.contains("(default)");
            let name = line.trim().trim_end_matches("(default)").trim().to_string();
            RustToolchain { name, is_default }
        })
        .collect();

    let rustup_home_bytes = rustup_home_size();

    Ok(ToolchainScanResult { toolchains, rustup_home_bytes })
}

fn rustup_home_size() -> u64 {
    // Try $RUSTUP_HOME first, then fall back to ~/.rustup
    let rustup_home = std::env::var("RUSTUP_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".rustup")));

    let Some(path) = rustup_home else {
        return 0;
    };

    if !path.exists() {
        return 0;
    }

    let output = Command::new("du").args(["-sk", path.to_str().unwrap_or("")]).output().ok();

    if let Some(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(kb_str) = text.split_whitespace().next() {
            if let Ok(kb) = kb_str.parse::<u64>() {
                return kb * 1024;
            }
        }
    }
    0
}

/// Returns true if `npm` is available on PATH.
pub fn npm_available() -> bool {
    which::which("npm").is_ok()
}

/// Lists globally installed npm packages.
pub fn list_npm_global_packages() -> Result<Vec<NpmGlobalPackage>> {
    let output = Command::new("npm").args(["list", "-g", "--depth", "0", "--json"]).output()?;

    // `npm list` can exit non-zero even on success (e.g. peer dep warnings) but
    // still emit valid JSON on stdout; only treat empty stdout as a hard failure.
    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("npm list -g failed: {}", stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("failed to parse npm list -g --json output: {}", e))?;

    let deps = match v.get("dependencies").and_then(|d| d.as_object()) {
        Some(d) => d,
        None => return Ok(vec![]),
    };

    let packages = deps
        .iter()
        .map(|(name, info)| {
            let version =
                info.get("version").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            NpmGlobalPackage { name: name.clone(), version }
        })
        .collect();

    Ok(packages)
}

/// Returns true if `pip3` or `pip` is available on PATH.
pub fn pip_available() -> bool {
    which::which("pip3").is_ok() || which::which("pip").is_ok()
}

/// Lists globally installed pip packages.
pub fn list_pip_packages() -> Result<Vec<PipPackage>> {
    // Try pip3 first, fall back to pip
    let output = Command::new("pip3")
        .args(["list", "--format", "json"])
        .output()
        .or_else(|_| Command::new("pip").args(["list", "--format", "json"]).output());

    let output = output.map_err(|e| anyhow::anyhow!("failed to spawn pip3/pip: {}", e))?;

    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("pip list failed: {}", stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("failed to parse pip list --format json output: {}", e))?;

    let packages = entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.to_string();
            let version = entry.get("version")?.as_str()?.to_string();
            Some(PipPackage { name, version })
        })
        .collect();

    Ok(packages)
}
