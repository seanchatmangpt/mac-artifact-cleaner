//! Docker container runtime integration layer.

use std::{os::unix::fs::MetadataExt, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::domain::docker_host::DockerHostFootprint;

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

/// Measures Docker Desktop's host-side sparse disk image(s) under a home
/// directory: `<home>/Library/Containers/com.docker.docker/Data/vms/*/data/`
/// — files named `Docker.raw` (and the equivalent `Docker.raw` sibling
/// `data.Docker.raw` some versions use), summed by logical size and physical
/// allocation (blocks × 512, the number that actually consumes host disk).
///
/// Returns `None` when no image file exists (Docker Desktop not installed,
/// or a different storage driver) — distinct from `Some(0..)`, which would
/// falsely claim an image was measured. Read-only: never touches the files,
/// only `symlink_metadata` (deliberately `symlink_` so a planted symlink is
/// counted as its own tiny inode, never followed into whatever it targets).
///
/// The physical-vs-VM-internal divergence this surfaces is the core of the
/// "cannot correctly clean up macOS" diagnosis: `docker system df` sees only
/// inside the VM; the host blocks are what a disk cleaner must account for.
pub fn docker_host_footprint_at(home: &Path) -> Option<DockerHostFootprint> {
    let vms_dir = home.join("Library/Containers/com.docker.docker/Data/vms");
    let entries = std::fs::read_dir(&vms_dir).ok()?;

    let mut fp = DockerHostFootprint::default();
    for entry in entries.flatten() {
        let data_dir = entry.path().join("data");
        let dir = match std::fs::read_dir(&data_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for f in dir.flatten() {
            let name = f.file_name();
            let name = name.to_string_lossy();
            if name != "Docker.raw" && name != "data.Docker.raw" {
                continue;
            }
            // symlink_metadata: never follow a planted symlink out of the
            // container dir while measuring.
            let Ok(meta) = std::fs::symlink_metadata(f.path()) else { continue };
            if !meta.is_file() {
                continue;
            }
            fp.docker_raw_count += 1;
            fp.docker_raw_logical_bytes = fp.docker_raw_logical_bytes.saturating_add(meta.len());
            fp.docker_raw_physical_bytes =
                fp.docker_raw_physical_bytes.saturating_add(meta.blocks() * 512);
        }
    }

    if fp.docker_raw_count == 0 {
        None
    } else {
        Some(fp)
    }
}

/// Measures Colima/Lima's host-side VM disk images under a home directory:
/// the shared data disk `<home>/.colima/_lima/_disks/<profile>/datadisk`
/// (where Docker images/volumes/build cache live) and each profile's root
/// disk `<home>/.colima/_lima/<profile>/{disk,diffdisk,basedisk}`.
///
/// Same contract as [`docker_host_footprint_at`]: `None` when no image file
/// exists, `symlink_metadata` only, never follows symlinks (Colima plants an
/// `in_use_by` symlink next to `datadisk`). Exists because the Docker Desktop
/// probe alone reported 12 GB physical on a machine whose Colima `datadisk`
/// pinned 53.6 GB of host blocks against 15 GB of VM-visible Docker usage.
pub fn colima_host_footprint_at(home: &Path) -> Option<DockerHostFootprint> {
    let lima = home.join(".colima/_lima");
    let mut fp = DockerHostFootprint::default();

    let mut measure = |path: &Path| {
        let Ok(meta) = std::fs::symlink_metadata(path) else { return };
        if !meta.is_file() {
            return;
        }
        fp.docker_raw_count += 1;
        fp.docker_raw_logical_bytes = fp.docker_raw_logical_bytes.saturating_add(meta.len());
        fp.docker_raw_physical_bytes =
            fp.docker_raw_physical_bytes.saturating_add(meta.blocks() * 512);
    };

    if let Ok(disks) = std::fs::read_dir(lima.join("_disks")) {
        for d in disks.flatten() {
            measure(&d.path().join("datadisk"));
        }
    }
    if let Ok(profiles) = std::fs::read_dir(&lima) {
        for p in profiles.flatten() {
            if p.file_name().to_string_lossy().starts_with('_') {
                continue;
            }
            for name in ["disk", "diffdisk", "basedisk"] {
                measure(&p.path().join(name));
            }
        }
    }

    if fp.docker_raw_count == 0 {
        None
    } else {
        Some(fp)
    }
}

/// Convenience wrapper for [`colima_host_footprint_at`] under the current
/// user's home directory.
pub fn colima_host_footprint() -> Option<DockerHostFootprint> {
    dirs::home_dir().and_then(|home| colima_host_footprint_at(&home))
}

/// Runs `fstrim -av` inside the Colima VM so the guest discards blocks its
/// filesystems no longer use, letting the host punch holes in the sparse
/// disk images. Data-preserving: only blocks the guest filesystem already
/// considers free are discarded — no image, container, or volume is removed.
/// Returns fstrim's stdout (per-mount "N bytes trimmed" lines).
pub fn colima_fstrim() -> Result<String> {
    if !is_colima_available() {
        anyhow::bail!("Colima not available");
    }
    let output = std::process::Command::new("colima")
        .args(["ssh", "--", "sudo", "fstrim", "-av"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("colima fstrim failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Convenience wrapper: measures Docker Desktop's sparse image under the
/// current user's home directory. `None` when there is no home dir or no
/// image file (see [`docker_host_footprint_at`]).
pub fn docker_host_footprint() -> Option<DockerHostFootprint> {
    dirs::home_dir().and_then(|home| docker_host_footprint_at(&home))
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

    #[test]
    fn host_footprint_measures_docker_raw_physical_and_logical() {
        let home = tempfile::tempdir().expect("tempdir");
        let vms = home.path().join("Library/Containers/com.docker.docker/Data/vms/0/data");
        std::fs::create_dir_all(&vms).expect("mkdir");
        std::fs::write(vms.join("Docker.raw"), vec![0u8; 4096]).expect("write raw");

        let fp = docker_host_footprint_at(home.path()).expect("footprint found");
        assert_eq!(fp.docker_raw_count, 1);
        assert_eq!(fp.docker_raw_logical_bytes, 4096);
        // Physical allocation of a real (non-sparse-in-test) file is at least
        // its logical size, rounded up to block boundaries.
        assert!(fp.docker_raw_physical_bytes >= 4096);
        assert!(fp.docker_raw_physical_bytes % 512 == 0);
    }

    #[test]
    fn colima_footprint_measures_datadisk_and_root_disk_not_symlinks() {
        let home = tempfile::tempdir().expect("tempdir");
        let lima = home.path().join(".colima/_lima");
        let disks = lima.join("_disks/colima");
        let profile = lima.join("colima");
        std::fs::create_dir_all(&disks).expect("mkdir disks");
        std::fs::create_dir_all(&profile).expect("mkdir profile");
        std::fs::create_dir_all(lima.join("_config")).expect("mkdir config");
        std::fs::write(disks.join("datadisk"), vec![1u8; 8192]).expect("write datadisk");
        std::fs::write(profile.join("disk"), vec![1u8; 4096]).expect("write disk");
        std::os::unix::fs::symlink(&profile, disks.join("in_use_by")).expect("symlink");
        // `_config` holds no disk and must not count.
        std::fs::write(lima.join("_config/disk"), vec![1u8; 4096]).expect("write decoy");

        let fp = colima_host_footprint_at(home.path()).expect("footprint found");
        assert_eq!(fp.docker_raw_count, 2);
        assert_eq!(fp.docker_raw_logical_bytes, 8192 + 4096);
        assert!(fp.docker_raw_physical_bytes >= 8192 + 4096);
    }

    #[test]
    fn colima_footprint_is_none_without_images() {
        let home = tempfile::tempdir().expect("tempdir");
        assert!(colima_host_footprint_at(home.path()).is_none());
        std::fs::create_dir_all(home.path().join(".colima/_lima/_disks/colima")).expect("mkdir");
        assert!(colima_host_footprint_at(home.path()).is_none());
    }

    #[test]
    fn host_footprint_is_none_without_docker_raw() {
        let home = tempfile::tempdir().expect("tempdir");
        // Home exists but has no Docker container dir at all.
        assert!(docker_host_footprint_at(home.path()).is_none());

        // A container dir whose vms/0/data exists but holds no Docker.raw
        // (e.g. Docker Desktop freshly reset) is also None, not Some(0).
        let vms = home.path().join("Library/Containers/com.docker.docker/Data/vms/0/data");
        std::fs::create_dir_all(&vms).expect("mkdir");
        std::fs::write(vms.join("unrelated.txt"), b"x").expect("write");
        assert!(docker_host_footprint_at(home.path()).is_none());
    }
}
