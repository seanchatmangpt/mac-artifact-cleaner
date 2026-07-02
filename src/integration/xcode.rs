//! Xcode and CoreSimulator integration layer.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorRuntime {
    pub name: String,
    pub version: String,
    pub is_available: bool,
    pub path: Option<PathBuf>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorScanResult {
    pub runtimes: Vec<SimulatorRuntime>,
    pub total_bytes: u64,
    pub unavailable_bytes: u64,
}

/// Returns true if `xcrun` is available on PATH.
pub fn xcrun_available() -> bool {
    std::process::Command::new("which")
        .arg("xcrun")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Raw shape returned by `xcrun simctl list runtimes --json`.
#[derive(Debug, Deserialize)]
struct SimctlRuntimesOutput {
    runtimes: Vec<SimctlRuntime>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlRuntime {
    name: String,
    version: String,
    #[serde(default)]
    is_available: bool,
    bundle_path: Option<String>,
}

/// Run `xcrun simctl list runtimes --json` and parse the result.
pub fn list_simulator_runtimes() -> Result<SimulatorScanResult> {
    let output = std::process::Command::new("xcrun")
        .args(["simctl", "list", "runtimes", "--json"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("xcrun simctl failed: {}", stderr.trim());
    }

    let parsed: SimctlRuntimesOutput = serde_json::from_slice(&output.stdout)?;

    let mut runtimes = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut unavailable_bytes: u64 = 0;

    for rt in parsed.runtimes {
        let path = rt.bundle_path.as_deref().map(PathBuf::from);
        let size_bytes = path.as_deref().map(du_path).unwrap_or(0);

        total_bytes += size_bytes;
        if !rt.is_available {
            unavailable_bytes += size_bytes;
        }

        runtimes.push(SimulatorRuntime {
            name: rt.name,
            version: rt.version,
            is_available: rt.is_available,
            path,
            size_bytes,
        });
    }

    Ok(SimulatorScanResult { runtimes, total_bytes, unavailable_bytes })
}

/// Estimate directory (or file) size via `du -sk`.
pub fn du_path(path: &std::path::Path) -> u64 {
    let output = std::process::Command::new("du").args(["-sk", &path.to_string_lossy()]).output();
    if let Ok(out) = output {
        if let Ok(s) = String::from_utf8(out.stdout) {
            if let Some(kb) = s.split_whitespace().next() {
                if let Ok(n) = kb.parse::<u64>() {
                    return n * 1024;
                }
            }
        }
    }
    0
}
