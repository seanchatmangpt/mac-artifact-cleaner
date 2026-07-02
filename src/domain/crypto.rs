//! Content-addressing helpers: BLAKE3 file hashing used to produce
//! tamper-evident manifests for deletion receipts.

use std::{
    fs::File,
    io::{Read, Result},
    path::Path,
};

use blake3;

/// Computes the BLAKE3 hex digest of a file's contents, streaming it in
/// 64 KiB chunks so large files don't need to be loaded into memory at once.
///
/// ```
/// use osx_clnr::domain::crypto::generate_manifest;
/// use std::io::Write;
///
/// let mut file = tempfile::NamedTempFile::new().unwrap();
/// write!(file, "hello world").unwrap();
///
/// let digest = generate_manifest(file.path()).unwrap();
/// assert_eq!(digest.len(), 64); // BLAKE3 hex digest is 32 bytes = 64 hex chars
///
/// // Refusal: a missing file yields an I/O error, not a panic.
/// assert!(generate_manifest(std::path::Path::new("/nonexistent/file")).is_err());
/// ```
pub fn generate_manifest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 65536];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}
