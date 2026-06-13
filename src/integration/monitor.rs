use notify::{RecommendedWatcher, RecursiveMode, Watcher, Result, Event};
use std::path::Path;
use std::sync::Arc;
use crate::domain::ocl::OclDatabase;

pub struct FsMonitor {
    db: Arc<OclDatabase>,
}

impl FsMonitor {
    pub fn new(db: Arc<OclDatabase>) -> Self {
        Self { db }
    }

    pub fn watch(&self, path: &Path) -> Result<()> {
        let db = self.db.clone();
        
        let mut watcher = RecommendedWatcher::new(move |res: Result<Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    // Update index here
                    let _ = db.insert_artifact(&path, "pending_rehash");
                }
            }
        }, notify::Config::default())?;

        watcher.watch(path, RecursiveMode::Recursive)?;
        
        // Keep watcher alive - this needs to be a long-running process
        std::mem::forget(watcher);
        
        Ok(())
    }
}
