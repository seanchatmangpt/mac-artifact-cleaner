//! Sled-backed local artifact ledger (`OclDatabase`) recording scanned
//! artifacts (path, size, hash, last-seen time) between scans.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::{Path, PathBuf};

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
    /// Opens (creating if absent) a sled-backed ledger at `path`.
    ///
    /// ```
    /// use osx_clnr::domain::ocl::OclDatabase;
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let db = OclDatabase::new(&dir.path().join("ledger.sled"));
    /// assert!(db.is_ok());
    /// ```
    pub fn new(path: &Path) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Inserts or overwrites an artifact record, keyed by its path.
    ///
    /// ```
    /// use osx_clnr::domain::ocl::{OclArtifact, OclDatabase};
    /// use std::path::PathBuf;
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let db = OclDatabase::new(&dir.path().join("ledger.sled")).unwrap();
    /// let artifact = OclArtifact {
    ///     path: PathBuf::from("/tmp/proj/target"),
    ///     size: 1024,
    ///     modified: 0,
    ///     blake3_hash: None,
    ///     reason: "rust build artifact".to_string(),
    ///     last_seen: 0,
    /// };
    /// assert!(db.insert_artifact(&artifact).is_ok());
    /// ```
    pub fn insert_artifact(&self, artifact: &OclArtifact) -> Result<()> {
        let key = artifact.path.to_string_lossy().as_bytes().to_vec();
        let value = serde_json::to_vec(artifact)?;
        self.db.insert(key, value)?;
        Ok(())
    }

    /// Looks up an artifact by path. Returns `Ok(None)` for a miss.
    ///
    /// ```
    /// use osx_clnr::domain::ocl::{OclArtifact, OclDatabase};
    /// use std::path::{Path, PathBuf};
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let db = OclDatabase::new(&dir.path().join("ledger.sled")).unwrap();
    /// let artifact = OclArtifact {
    ///     path: PathBuf::from("/tmp/proj/target"),
    ///     size: 1024,
    ///     modified: 0,
    ///     blake3_hash: None,
    ///     reason: "rust build artifact".to_string(),
    ///     last_seen: 0,
    /// };
    /// db.insert_artifact(&artifact).unwrap();
    ///
    /// let found = db.get_artifact(Path::new("/tmp/proj/target")).unwrap();
    /// assert_eq!(found.unwrap().size, 1024);
    ///
    /// // Refusal: unknown paths are a plain miss, not an error.
    /// assert!(db.get_artifact(Path::new("/tmp/proj/never-seen")).unwrap().is_none());
    /// ```
    pub fn get_artifact(&self, path: &std::path::Path) -> Result<Option<OclArtifact>> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        if let Some(v) = self.db.get(key)? {
            let artifact: OclArtifact = serde_json::from_slice(&v)?;
            Ok(Some(artifact))
        } else {
            Ok(None)
        }
    }

    /// Lists every artifact currently recorded in the ledger.
    ///
    /// ```
    /// use osx_clnr::domain::ocl::{OclArtifact, OclDatabase};
    /// use std::path::PathBuf;
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let db = OclDatabase::new(&dir.path().join("ledger.sled")).unwrap();
    /// assert_eq!(db.list_all_artifacts().unwrap().len(), 0);
    ///
    /// db.insert_artifact(&OclArtifact {
    ///     path: PathBuf::from("/tmp/proj/target"),
    ///     size: 1024,
    ///     modified: 0,
    ///     blake3_hash: None,
    ///     reason: "rust build artifact".to_string(),
    ///     last_seen: 0,
    /// })
    /// .unwrap();
    ///
    /// assert_eq!(db.list_all_artifacts().unwrap().len(), 1);
    /// ```
    pub fn list_all_artifacts(&self) -> Result<Vec<OclArtifact>> {
        let mut results = Vec::new();
        for item in self.db.iter() {
            let (_, v) = item?;
            let artifact: OclArtifact = serde_json::from_slice(&v)?;
            results.push(artifact);
        }
        Ok(results)
    }

    /// Removes an artifact record by path. Removing an absent key is a no-op.
    ///
    /// ```
    /// use osx_clnr::domain::ocl::{OclArtifact, OclDatabase};
    /// use std::path::{Path, PathBuf};
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let db = OclDatabase::new(&dir.path().join("ledger.sled")).unwrap();
    /// db.insert_artifact(&OclArtifact {
    ///     path: PathBuf::from("/tmp/proj/target"),
    ///     size: 1024,
    ///     modified: 0,
    ///     blake3_hash: None,
    ///     reason: "rust build artifact".to_string(),
    ///     last_seen: 0,
    /// })
    /// .unwrap();
    ///
    /// db.remove_artifact(Path::new("/tmp/proj/target")).unwrap();
    /// assert!(db.get_artifact(Path::new("/tmp/proj/target")).unwrap().is_none());
    ///
    /// // Refusal: removing a never-seen path does not error.
    /// assert!(db.remove_artifact(Path::new("/tmp/proj/never-seen")).is_ok());
    /// ```
    pub fn remove_artifact(&self, path: &std::path::Path) -> Result<()> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        self.db.remove(key)?;
        Ok(())
    }
}
