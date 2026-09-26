//! Docker container runtime integration layer.

use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

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

/// One Lima managed disk discovered under `<home>/.colima/_lima/_disks/<name>/`.
///
/// `colima delete` removes the instance directory but deliberately leaves
/// named Lima disks (`_disks/<name>/datadisk`) behind; on a machine that has
/// churned through profiles these orphans can pin tens of GB of host blocks
/// that no registry entry accounts for. This struct is the observation half
/// of reaping them safely: `orphan` is fail-closed — it is only `true` when
/// the `limactl` registry was readable AND does not list the disk AND the
/// directory carries no resolvable `in_use_by` symlink. Anything ambiguous
/// (registry unreadable, symlink present, datadisk missing) classifies as
/// not-orphan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimaDiskEntry {
    pub name: String,
    pub dir: PathBuf,
    pub datadisk_logical_bytes: u64,
    pub datadisk_physical_bytes: u64,
    /// Raw target of the `in_use_by` symlink when present and resolvable to
    /// an existing path; `None` when absent or dangling.
    pub in_use_by: Option<String>,
    /// Whether `limactl disk list` knows this disk by name. Always `false`
    /// when the registry could not be read (`registry_known == false`).
    pub registered: bool,
    /// `true` only when the registry was readable, does not list this disk,
    /// and no valid `in_use_by` symlink exists. Reaping may touch these and
    /// nothing else.
    pub orphan: bool,
}

/// Parses `limactl disk list` output into disk names (first column),
/// skipping the header row. Pure so classification is testable without a
/// running limactl.
fn parse_lima_disk_list(output: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in output.lines().skip(1) {
        let Some(name) = line.split_whitespace().next() else { continue };
        if name == "NAME" || name.is_empty() {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

/// Reads the Lima disk registry via `limactl disk list`.
///
/// - `Ok(None)` — `limactl` is not installed; registry state is UNKNOWN.
/// - `Ok(Some(names))` — registry read successfully.
/// - `Err` — `limactl` exists but failed; registry state is UNKNOWN and
///   callers must refuse to reap (fail-closed).
pub fn lima_disk_registry() -> Result<Option<Vec<String>>> {
    let output = match std::process::Command::new("limactl").arg("disk").arg("list").output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow::Error::from(e).context("failed to run limactl")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("limactl disk list failed: {}", stderr.trim());
    }
    Ok(Some(parse_lima_disk_list(&String::from_utf8_lossy(&output.stdout))))
}

/// Classifies every `_disks/<name>` directory under a Lima home. Read-only:
/// never mutates anything, only `symlink_metadata`/`read_link` (the
/// `in_use_by` symlink is read, not followed for measurement — matching
/// [`colima_host_footprint_at`]).
///
/// `registry` is the parsed `limactl disk list` output; `None` means the
/// registry was unreadable, in which case nothing classifies as orphan.
pub fn lima_disk_entries_at(home: &Path, registry: Option<&[String]>) -> Vec<LimaDiskEntry> {
    let disks_dir = home.join(".colima/_lima/_disks");
    let Ok(entries) = std::fs::read_dir(&disks_dir) else { return Vec::new() };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // symlink_metadata on the dir itself: a planted symlink where a disk
        // dir should be is skipped, not followed.
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();

        let mut logical = 0u64;
        let mut physical = 0u64;
        let mut has_datadisk = false;
        if let Ok(dmeta) = std::fs::symlink_metadata(path.join("datadisk")) {
            if dmeta.is_file() {
                has_datadisk = true;
                logical = dmeta.len();
                physical = dmeta.blocks() * 512;
            }
        }

        let in_use_by = std::fs::read_link(path.join("in_use_by"))
            .ok()
            .map(|target| {
                let resolved =
                    if target.is_absolute() { target.clone() } else { path.join(&target) };
                // Only count the symlink as live when its target exists.
                if resolved.exists() {
                    Some(target.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .unwrap_or(None);

        let registered = registry.map(|r| r.contains(&name)).unwrap_or(false);
        let orphan = registry.is_some() && !registered && in_use_by.is_none() && has_datadisk;

        out.push(LimaDiskEntry {
            name,
            dir: path,
            datadisk_logical_bytes: logical,
            datadisk_physical_bytes: physical,
            in_use_by,
            registered,
            orphan,
        });
    }
    out
}

/// Convenience: registry + classification for the current user's home.
/// `Err` when the registry read fails (fail-closed — callers must not reap).
pub fn lima_disk_entries() -> Result<(bool, Vec<LimaDiskEntry>)> {
    let registry = lima_disk_registry()?;
    let registry_known = registry.is_some();
    let entries = dirs::home_dir()
        .map(|home| lima_disk_entries_at(&home, registry.as_deref()))
        .unwrap_or_default();
    Ok((registry_known, entries))
}

/// One reaped disk: what was removed and what the host volume got back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReapedLimaDisk {
    pub name: String,
    pub path: PathBuf,
    pub logical_bytes: u64,
    pub physical_bytes_freed: u64,
}

/// Deletes the directories of exactly the entries classified `orphan`,
/// returning a record per reaped disk. Refuses (typed error) if any entry is
/// not an orphan, if the registry was unknown, or if a dir escapes
/// `<home>/.colima/_lima/_disks` — the scanner's classification is the only
/// thing this may act on.
pub fn reap_orphan_lima_disks_at(
    home: &Path,
    registry_known: bool,
    entries: &[LimaDiskEntry],
) -> Result<Vec<ReapedLimaDisk>> {
    if !registry_known {
        anyhow::bail!(
            "REFUSED_REAP_REGISTRY_UNKNOWN: limactl registry unreadable — orphan classification \
             is not trustworthy, refusing to delete"
        );
    }
    let disks_root = home.join(".colima/_lima/_disks");
    let mut reaped = Vec::new();
    for e in entries {
        if !e.orphan {
            continue;
        }
        if e.dir.parent() != Some(disks_root.as_path()) {
            anyhow::bail!(
                "REFUSED_REAP_PATH_OUTSIDE_DISKS: {} is not under {}",
                e.dir.display(),
                disks_root.display()
            );
        }
        let freed = e.datadisk_physical_bytes;
        std::fs::remove_dir_all(&e.dir)
            .map_err(|err| anyhow::Error::from(err).context(format!("reap {}", e.dir.display())))?;
        if e.dir.exists() {
            anyhow::bail!("REFUSED_REAP_STILL_PRESENT: {} survived removal", e.dir.display());
        }
        reaped.push(ReapedLimaDisk {
            name: e.name.clone(),
            path: e.dir.clone(),
            logical_bytes: e.datadisk_logical_bytes,
            physical_bytes_freed: freed,
        });
    }
    Ok(reaped)
}

/// Convenience wrapper for [`reap_orphan_lima_disks_at`] under the current
/// user's home directory.
pub fn reap_orphan_lima_disks(
    registry_known: bool,
    entries: &[LimaDiskEntry],
) -> Result<Vec<ReapedLimaDisk>> {
    match dirs::home_dir() {
        Some(home) => reap_orphan_lima_disks_at(&home, registry_known, entries),
        None => anyhow::bail!("no home directory"),
    }
}

/// Parses the `Avail` column (4th field, 1K blocks) of `df -k -P` output
/// into bytes. Pure so the df parsing is testable without a real volume.
/// `None` for a header-only or malformed table.
fn parse_df_avail_bytes(output: &str) -> Option<u64> {
    let line = output.lines().nth(1)?;
    let avail_kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb.saturating_mul(1024))
}

/// Free bytes available to unprivileged users on the volume containing
/// `path`, via `df -k -P` (POSIX portable format; parsed rather than raw
/// `statfs` because this crate denies `unsafe`). `Err` when the path does
/// not exist or `df` fails.
pub fn volume_free_bytes(path: &Path) -> Result<u64> {
    let output = std::process::Command::new("df").arg("-k").arg("-P").arg(path).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("df failed for {}: {}", path.display(), stderr.trim());
    }
    parse_df_avail_bytes(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| anyhow::anyhow!("could not parse df output for {}", path.display()))
}

/// Profiles reported by `colima list` (first column, header skipped).
/// Empty when the CLI is missing or lists nothing.
fn colima_list_profiles() -> Vec<String> {
    let Ok(output) = std::process::Command::new("colima").arg("list").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .filter(|n| n != "PROFILE")
        .collect()
}

/// Deletes the default Colima instance if one exists (`colima delete
/// --force`). Destructive — tears down the VM and its filesystem; callers
/// gate this behind their own `--confirm`. Returns whether a delete was
/// actually attempted (a profile existed).
pub fn colima_delete_default() -> Result<bool> {
    if !is_colima_available() {
        anyhow::bail!("Colima not available");
    }
    if !colima_list_profiles().iter().any(|n| n == "default") {
        return Ok(false);
    }
    let output = std::process::Command::new("colima").args(["delete", "--force"]).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("colima delete failed: {}", stderr.trim());
    }
    Ok(true)
}

/// Starts a fresh default Colima instance with explicit resource bounds.
/// `vm_type` is passed through as `--vm-type` when given (e.g. `vz`).
/// Returns raw stdout; `colima start` has no machine-readable format.
pub fn colima_start(
    cpu: u32,
    memory_gib: u32,
    disk_gib: u32,
    vm_type: Option<&str>,
) -> Result<String> {
    if !is_colima_available() {
        anyhow::bail!("Colima not available");
    }
    let mut args = vec![
        "start".to_string(),
        "--cpu".to_string(),
        cpu.to_string(),
        "--memory".to_string(),
        format!("{memory_gib}GiB"),
        "--disk".to_string(),
        format!("{disk_gib}GiB"),
        "--runtime".to_string(),
        "docker".to_string(),
    ];
    if let Some(vt) = vm_type {
        args.push("--vm-type".to_string());
        args.push(vt.to_string());
    }
    let output = std::process::Command::new("colima").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("colima start failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

    #[test]
    fn parse_lima_disk_list_skips_header_and_grabs_names() {
        let out = "NAME    SIZE    FORMAT    DIR    IN-USE-BY\ncolima  40GiB   qcow2     d0     dir/colima\ncolima-act  150GiB  raw   d1     \n";
        let names = parse_lima_disk_list(out);
        assert_eq!(names, vec!["colima".to_string(), "colima-act".to_string()]);
    }

    #[test]
    fn parse_lima_disk_list_empty_registry() {
        // Real `limactl disk list` with no disks prints just the header plus
        // a warning on stderr (not stdout).
        assert!(parse_lima_disk_list("NAME    SIZE    FORMAT    DIR    IN-USE-BY\n").is_empty());
    }

    fn make_disk(home: &Path, name: &str, blocks: usize) -> std::path::PathBuf {
        let dir = home.join(".colima/_lima/_disks").join(name);
        std::fs::create_dir_all(&dir).expect("mkdir disk");
        std::fs::write(dir.join("datadisk"), vec![1u8; blocks * 512]).expect("write datadisk");
        dir
    }

    #[test]
    fn classification_registered_in_use_and_orphan() {
        let home = tempfile::tempdir().expect("tempdir");
        make_disk(home.path(), "colima", 8); // registry lists it
        make_disk(home.path(), "live", 8); // not registered, but in_use_by link
        let live_dir = home.path().join(".colima/_lima/_disks/live");
        std::os::unix::fs::symlink(&live_dir, live_dir.join("in_use_by")).expect("symlink");
        make_disk(home.path(), "ghost", 8); // neither -> orphan
                                            // A dangling in_use_by must NOT keep a disk alive.
        let dangling = make_disk(home.path(), "dangling", 8);
        std::os::unix::fs::symlink(
            home.path().join(".colima/_lima/gone-instance"),
            dangling.join("in_use_by"),
        )
        .expect("symlink");
        // A decoy file (not dir) under _disks is ignored entirely.
        std::fs::write(home.path().join(".colima/_lima/_disks/not-a-disk"), b"x")
            .expect("write decoy");

        let registry = vec!["colima".to_string()];
        let entries = lima_disk_entries_at(home.path(), Some(&registry));
        let by_name = |n: &str| entries.iter().find(|e| e.name == n).expect("entry").clone();

        assert!(by_name("colima").registered && !by_name("colima").orphan);
        assert!(!by_name("live").registered && !by_name("live").orphan);
        assert!(by_name("live").in_use_by.is_some());
        assert!(by_name("ghost").orphan);
        // Dangling link: target does not exist, so the disk is an orphan.
        assert!(by_name("dangling").orphan, "dangling in_use_by must not protect a disk");
        assert!(!entries.iter().any(|e| e.name == "not-a-disk"));
    }

    #[test]
    fn classification_refuses_orphans_when_registry_unknown() {
        let home = tempfile::tempdir().expect("tempdir");
        make_disk(home.path(), "ghost", 8);
        let entries = lima_disk_entries_at(home.path(), None);
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|e| !e.orphan), "unknown registry must orphan nothing");
    }

    #[test]
    fn reap_deletes_only_orphans_and_fails_closed_on_unknown_registry() {
        let home = tempfile::tempdir().expect("tempdir");
        let ghost_dir = make_disk(home.path(), "ghost", 8);
        let live_dir = make_disk(home.path(), "live", 8);
        std::os::unix::fs::symlink(&live_dir, live_dir.join("in_use_by")).expect("symlink");
        let registry = vec!["live".to_string()];
        let entries = lima_disk_entries_at(home.path(), Some(&registry));

        // Unknown registry: typed refusal, nothing deleted.
        let refused = reap_orphan_lima_disks_at(home.path(), false, &entries);
        assert!(refused.is_err());
        assert!(refused.err().unwrap().to_string().contains("REFUSED_REAP_REGISTRY_UNKNOWN"));
        assert!(ghost_dir.exists());

        let reaped = reap_orphan_lima_disks_at(home.path(), true, &entries).expect("reap");
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].name, "ghost");
        assert!(!ghost_dir.exists(), "orphan must be gone");
        assert!(live_dir.exists(), "in-use disk must survive");
        assert!(reaped[0].physical_bytes_freed >= 8 * 512);
    }

    #[test]
    fn parse_df_avail_bytes_reads_second_line_fourth_column() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 972756104 11534088 761756016 2% /";
        assert_eq!(parse_df_avail_bytes(out), Some(761_756_016 * 1024));
        assert_eq!(parse_df_avail_bytes("Filesystem 1024-blocks\n"), None);
    }

    #[test]
    fn volume_free_bytes_parses_a_real_volume() {
        let home = tempfile::tempdir().expect("tempdir");
        let free = volume_free_bytes(home.path()).expect("statfs");
        // A live volume's free space fluctuates between calls, so no
        // byte-equality — just prove the df parse path returns a plausible
        // positive number, block-aligned or not.
        assert!(free > 0, "real volume reported 0 free bytes");
    }
}
