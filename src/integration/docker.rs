//! Docker container runtime integration layer.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerDiskUsage {
    pub images_count: u64,
    pub images_bytes: u64,
    pub containers_count: u64,
    pub containers_bytes: u64,
    pub volumes_count: u64,
    pub volumes_bytes: u64,
    pub build_cache_count: u64,
    pub build_cache_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerPrunePreview {
    pub images_reclaimable_bytes: u64,
    pub volumes_reclaimable_bytes: u64,
    pub build_cache_reclaimable_bytes: u64,
    pub total_reclaimable_bytes: u64,
}

/// Raw line from `docker system df --format json`.
///
/// `TotalCount` (not `Total`) is what real `docker system df --format json`
/// output actually names the field, and it is emitted as a JSON string (e.g.
/// `"7"`), not a number — a prior version of this struct assumed `Total: u64`
/// and silently failed `serde_json::from_str` on every real line, which
/// `run_df` swallowed into a `Warning:` and an empty result.
#[derive(Debug, Deserialize)]
struct DfLine {
    #[serde(rename = "Type")]
    type_name: String,
    #[serde(rename = "TotalCount")]
    total_count: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "Reclaimable")]
    reclaimable: String,
}

impl DfLine {
    fn total(&self) -> u64 {
        self.total_count.parse().unwrap_or(0)
    }
}

/// Returns `true` if `docker info` exits with status 0.
pub fn is_docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Parses a human-readable Docker size string into bytes.
///
/// Handles suffixes: `B`, `KB`, `MB`, `GB`, `TB`. The numeric part may be an
/// integer or a decimal (e.g. `"2.1GB"`). Strips trailing parenthetical
/// annotations such as `" (40%)"` so callers can pass raw Docker output directly.
/// Returns `0` for unrecognised input.
///
/// This is a thin alias over the single shared implementation in
/// [`crate::integration::progress::parse_human_size`].
///
/// # Examples
///
/// ```
/// use osx_clnr::integration::docker::parse_size_str;
/// assert_eq!(parse_size_str("0B"), 0);
/// assert_eq!(parse_size_str("1KB"), 1024);
/// assert_eq!(parse_size_str("1MB"), 1_048_576);
/// ```
pub fn parse_size_str(s: &str) -> u64 {
    crate::integration::progress::parse_human_size(s)
}

/// Runs `docker system df --format json` and returns parsed lines.
fn run_df() -> Result<Vec<DfLine>> {
    if !is_docker_available() {
        anyhow::bail!("Docker not available");
    }

    let output =
        std::process::Command::new("docker").args(["system", "df", "--format", "json"]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker system df failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = Vec::new();
    for raw in stdout.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match serde_json::from_str::<DfLine>(raw) {
            Ok(line) => lines.push(line),
            Err(e) => {
                eprintln!("Warning: could not parse docker df line {:?}: {}", raw, e);
            }
        }
    }
    Ok(lines)
}

/// Returns current Docker disk usage broken down by resource type.
///
/// Returns an error if Docker is unavailable or the command fails.
pub fn docker_disk_usage() -> Result<DockerDiskUsage> {
    let lines = run_df()?;

    let mut usage = DockerDiskUsage {
        images_count: 0,
        images_bytes: 0,
        containers_count: 0,
        containers_bytes: 0,
        volumes_count: 0,
        volumes_bytes: 0,
        build_cache_count: 0,
        build_cache_bytes: 0,
        total_bytes: 0,
    };

    for line in &lines {
        let bytes = parse_size_str(&line.size);
        match line.type_name.as_str() {
            "Images" => {
                usage.images_count = line.total();
                usage.images_bytes = bytes;
            }
            "Containers" => {
                usage.containers_count = line.total();
                usage.containers_bytes = bytes;
            }
            "Local Volumes" | "Volumes" => {
                usage.volumes_count = line.total();
                usage.volumes_bytes = bytes;
            }
            "Build Cache" => {
                usage.build_cache_count = line.total();
                usage.build_cache_bytes = bytes;
            }
            _ => {}
        }
    }

    usage.total_bytes = usage
        .images_bytes
        .saturating_add(usage.containers_bytes)
        .saturating_add(usage.volumes_bytes)
        .saturating_add(usage.build_cache_bytes);

    Ok(usage)
}

/// Returns what `docker system prune` would reclaim without executing it.
///
/// Parses the `Reclaimable` field from `docker system df --format json`.
/// Returns an error if Docker is unavailable or the command fails.
pub fn docker_prune_preview() -> Result<DockerPrunePreview> {
    let lines = run_df()?;

    let mut preview = DockerPrunePreview {
        images_reclaimable_bytes: 0,
        volumes_reclaimable_bytes: 0,
        build_cache_reclaimable_bytes: 0,
        total_reclaimable_bytes: 0,
    };

    for line in &lines {
        // Reclaimable looks like "2.1GB (40%)" — parse_size_str handles the annotation.
        let bytes = parse_size_str(&line.reclaimable);
        match line.type_name.as_str() {
            "Images" => preview.images_reclaimable_bytes = bytes,
            "Local Volumes" | "Volumes" => preview.volumes_reclaimable_bytes = bytes,
            "Build Cache" => preview.build_cache_reclaimable_bytes = bytes,
            _ => {}
        }
    }

    preview.total_reclaimable_bytes = preview
        .images_reclaimable_bytes
        .saturating_add(preview.volumes_reclaimable_bytes)
        .saturating_add(preview.build_cache_reclaimable_bytes);

    Ok(preview)
}

/// Result of executing `docker system prune -af --volumes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerPruneResult {
    pub before: DockerDiskUsage,
    pub after: DockerDiskUsage,
    pub reclaimed_bytes: u64,
    pub stdout: String,
}

/// Executes `docker system prune -af --volumes`, actually removing unused
/// images, stopped containers, unused networks, dangling build cache, and
/// (because `--volumes` is passed) unused local volumes. Destructive —
/// callers must gate this behind their own confirmation, same as
/// `delete::execute` and `snapshot::thin/delete` do at the CLI/MCP layer;
/// this function performs no confirmation of its own.
///
/// Returns before/after disk usage plus the delta actually reclaimed.
pub fn docker_system_prune() -> Result<DockerPruneResult> {
    if !is_docker_available() {
        anyhow::bail!("Docker not available");
    }

    let before = docker_disk_usage()?;

    let output = std::process::Command::new("docker")
        .args(["system", "prune", "-af", "--volumes"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker system prune failed: {}", stderr.trim());
    }

    let after = docker_disk_usage()?;
    let reclaimed_bytes = before.total_bytes.saturating_sub(after.total_bytes);

    Ok(DockerPruneResult {
        before,
        after,
        reclaimed_bytes,
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
    })
}

/// Returns `true` if the `colima` CLI is on `PATH`.
pub fn is_colima_available() -> bool {
    std::process::Command::new("colima")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Runs `colima prune`, which removes cached downloaded VM assets (old
/// Lima/QEMU images, stale layer downloads) without touching the running VM,
/// its disk, or any containers inside it — unlike `colima delete`, which
/// tears down the whole VM and is deliberately not exposed here. Returns raw
/// stdout since `colima prune` has no machine-readable output format.
pub fn colima_prune() -> Result<String> {
    if !is_colima_available() {
        anyhow::bail!("Colima not available");
    }

    let output = std::process::Command::new("colima").args(["prune", "--force"]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("colima prune failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_zero_bytes() {
        assert_eq!(parse_size_str("0B"), 0);
    }

    #[test]
    fn parse_kilobytes() {
        assert_eq!(parse_size_str("1KB"), 1_024);
    }

    #[test]
    fn parse_megabytes() {
        assert_eq!(parse_size_str("1MB"), 1_048_576);
    }

    #[test]
    fn parse_decimal_gigabytes() {
        let expected = (2.1_f64 * 1_073_741_824_f64) as u64;
        assert_eq!(parse_size_str("2.1GB"), expected);
    }

    #[test]
    fn parse_reclaimable_with_pct() {
        // Docker reclaimable strings include "(40%)" — must equal the plain value.
        assert_eq!(parse_size_str("2.1GB (40%)"), parse_size_str("2.1GB"));
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(parse_size_str(""), 0);
    }
}
