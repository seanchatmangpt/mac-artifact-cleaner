//! Content-addressing helpers: BLAKE3 hashing used to produce
//! tamper-evident manifests for deletion receipts.
//!
//! **Domain purity**: this module only hashes already-read bytes handed to it
//! by the caller. Reading file contents from disk is the integration layer's
//! job (`crate::integration::fs::generate_manifest`), which streams the file
//! and delegates the actual digest computation back to [`hash_bytes`].

use blake3;

/// Computes the BLAKE3 hex digest of `data`.
///
/// ```
/// use osx_clnr::domain::crypto::hash_bytes;
///
/// // Positive: hex digest is 32 bytes = 64 hex chars.
/// let digest = hash_bytes(b"hello world");
/// assert_eq!(digest.len(), 64);
///
/// // Negative: different content yields a different digest.
/// let other = hash_bytes(b"hello there");
/// assert_ne!(digest, other);
///
/// // Refusal: empty input still hashes deterministically rather than
/// // panicking or erroring — there is no invalid byte slice.
/// assert_eq!(hash_bytes(b"").len(), 64);
/// assert_eq!(hash_bytes(b""), hash_bytes(b""));
/// ```
pub fn hash_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
