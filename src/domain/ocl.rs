use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OclArtifact {
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
    pub blake3_hash: Option<String>,
    pub reason: String,
    pub last_seen: u64,
}

pub struct OclDatabase {
    db: Db,
}

impl OclDatabase {
    pub fn new(path: &Path) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn insert_artifact(&self, artifact: &OclArtifact) -> Result<()> {
        let key = artifact.path.to_string_lossy().as_bytes().to_vec();
        let value = serde_json::to_vec(artifact)?;
        self.db.insert(key, value)?;
        Ok(())
    }

    pub fn get_artifact(&self, path: &PathBuf) -> Result<Option<OclArtifact>> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        if let Some(v) = self.db.get(key)? {
            let artifact: OclArtifact = serde_json::from_slice(&v)?;
            Ok(Some(artifact))
        } else {
            Ok(None)
        }
    }

    pub fn list_all_artifacts(&self) -> Result<Vec<OclArtifact>> {
        let mut results = Vec::new();
        for item in self.db.iter() {
            let (_, v) = item?;
            let artifact: OclArtifact = serde_json::from_slice(&v)?;
            results.push(artifact);
        }
        Ok(results)
    }

    pub fn remove_artifact(&self, path: &PathBuf) -> Result<()> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        self.db.remove(key)?;
        Ok(())
    }
}
