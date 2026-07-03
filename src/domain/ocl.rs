//! Artifact ledger record type.
//!
//! **Domain purity**: this module only defines the inert [`OclArtifact`] DTO
//! recording a scanned artifact's path, size, hash, and last-seen time. The
//! sled-backed ledger that persists these records between scans,
//! `OclDatabase`, lives in `crate::integration::ocl_store` — sled is an
//! OS-backed KV store, not domain logic.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A single artifact record tracked in the local ledger.
///
/// ```
/// use osx_clnr::domain::ocl::OclArtifact;
/// use std::path::PathBuf;
///
/// // Positive: constructing a record is plain data, no I/O involved.
/// let artifact = OclArtifact {
///     path: PathBuf::from("/tmp/proj/target"),
///     size: 1024,
///     modified: 0,
///     blake3_hash: None,
///     reason: "rust build artifact".to_string(),
///     last_seen: 0,
/// };
/// assert_eq!(artifact.size, 1024);
///
/// // Negative: two records with different paths are not equal in content.
/// let other = OclArtifact { path: PathBuf::from("/tmp/proj/other"), ..artifact.clone() };
/// assert_ne!(artifact.path, other.path);
/// ```
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OclArtifact {
    pub path: PathBuf,
    pub size: u64,
    pub modified: u64,
    pub blake3_hash: Option<String>,
    pub reason: String,
    pub last_seen: u64,
}
