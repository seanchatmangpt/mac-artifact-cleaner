use crate::domain::crypto::generate_manifest;
use crate::domain::ocl::{OclArtifact, OclDatabase};
use crate::domain::time::system_time_to_unix;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

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

        // In a real daemon, we wouldn't forget the watcher, but keep it in a loop
        // For this implementation, we ensure it persists in this thread context.
        std::mem::forget(watcher);

        Ok(())
    }
}
