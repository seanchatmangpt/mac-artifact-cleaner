use std::{path::Path, sync::Arc, time::SystemTime};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result, Watcher};

use crate::{
    domain::{
        crypto::generate_manifest,
        ocl::{OclArtifact, OclDatabase},
        time::system_time_to_unix,
    },
    integration::{fs::volume_space, notify::notify_disk_pressure},
};

const BYTES_PER_GB: f64 = 1_073_741_824.0;

pub struct FsMonitor {
    db: Arc<OclDatabase>,
}

impl FsMonitor {
    pub fn new(db: Arc<OclDatabase>) -> Self {
        Self { db }
    }

    pub fn watch(&self, path: &Path) -> Result<()> {
        let db = self.db.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            for path in event.paths {
                                if let Ok(meta) = std::fs::metadata(&path) {
                                    let modified = meta
                                        .modified()
                                        .map(system_time_to_unix)
                                        .unwrap_or_default()
                                        as u64;

                                    let hash = if meta.is_file() {
                                        generate_manifest(&path).ok()
                                    } else {
                                        None
                                    };

                                    let artifact = OclArtifact {
                                        path: path.clone(),
                                        size: meta.len(),
                                        modified,
                                        blake3_hash: hash,
                                        reason: "auto_detected".to_string(),
                                        last_seen: system_time_to_unix(SystemTime::now()) as u64,
                                    };

                                    let _ = db.insert_artifact(&artifact);
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                let _ = db.remove_artifact(&path);
                            }
                        }
                        _ => {}
                    }
                }
            },
            notify::Config::default(),
        )?;

        watcher.watch(path, RecursiveMode::Recursive)?;

        // Keep watcher alive in a background thread so the watch persists
        std::thread::spawn(move || {
            let _watcher = watcher;
            loop {
                std::thread::park();
            }
        });

        Ok(())
    }
}

// ── Disk pressure watcher ────────────────────────────────────────────────────

/// Outcome of a single disk-pressure check against a volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiskPressureCheck {
    /// Free (available) space on the volume, in GB.
    pub free_gb: f64,
    /// Threshold the caller configured, in GB.
    pub threshold_gb: f64,
    /// True if `free_gb` is below `threshold_gb`.
    pub under_pressure: bool,
}

/// Queries free space on the volume containing `mount` and compares it to
/// `threshold_gb`. Pure I/O: performs a `statvfs(2)` call via
/// [`crate::integration::fs::volume_space`] and returns an inert result —
/// callers decide whether/how to act on `under_pressure`.
pub fn check_disk_pressure(mount: &Path, threshold_gb: f64) -> anyhow::Result<DiskPressureCheck> {
    let vs = volume_space(mount)?;
    let free_gb = vs.available as f64 / BYTES_PER_GB;
    Ok(DiskPressureCheck { free_gb, threshold_gb, under_pressure: free_gb < threshold_gb })
}

/// Runs [`check_disk_pressure`] and, if the volume is under pressure, fires a
/// desktop notification via [`crate::integration::notify::notify_disk_pressure`].
/// Returns the check outcome either way so callers can report it.
pub fn check_and_notify(mount: &Path, threshold_gb: f64) -> anyhow::Result<DiskPressureCheck> {
    let check = check_disk_pressure(mount, threshold_gb)?;
    if check.under_pressure {
        notify_disk_pressure(check.free_gb, threshold_gb)?;
    }
    Ok(check)
}
