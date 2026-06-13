use sled::Db;
use std::path::{Path, PathBuf};
use anyhow::Result;

pub struct OclDatabase {
    db: Db,
}

impl OclDatabase {
    pub fn new(path: &Path) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn insert_artifact(&self, path: &PathBuf, hash: &str) -> Result<()> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        self.db.insert(key, hash.as_bytes())?;
        Ok(())
    }

    pub fn get_hash(&self, path: &PathBuf) -> Result<Option<String>> {
        let key = path.to_string_lossy().as_bytes().to_vec();
        Ok(self.db.get(key)?.map(|v| String::from_utf8_lossy(&v).to_string()))
    }
}
