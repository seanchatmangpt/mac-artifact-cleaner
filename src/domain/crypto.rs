//! Content-addressing helpers: BLAKE3 hashing used to produce
//! tamper-evident manifests for deletion receipts.
//!
//! **Domain purity**: this module only hashes already-read bytes handed to it
//! by the caller. Reading file contents from disk is the integration layer's
//! job (`crate::integration::fs::generate_manifest`), which streams the file
//! and delegates the actual digest computation back to [`hash_bytes`].

use blake3;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

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

/// Computes an HMAC-SHA256 over `data`, keyed with `secret`, and returns the
/// hex-encoded MAC.
///
/// Unlike [`hash_bytes`], this is **not** forgeable by anyone who can only
/// read/write the data being signed (e.g. a plan file on disk): reproducing
/// the output additionally requires the caller-supplied `secret`, which must
/// come from outside anything an attacker can observe (an environment
/// variable or a machine-local key file — see
/// `integration::config::approval_secret`).
///
/// ```
/// use osx_clnr::domain::crypto::hmac_sha256_hex;
///
/// // Positive: deterministic for the same secret and data.
/// let a = hmac_sha256_hex(b"secret-key", b"plan content");
/// let b = hmac_sha256_hex(b"secret-key", b"plan content");
/// assert_eq!(a, b);
///
/// // Refusal: a different secret produces a completely different MAC, even
/// // though the signed data is identical — this is exactly what makes the
/// // signature unforgeable by someone who only knows the data.
/// let forged = hmac_sha256_hex(b"guessed-wrong-key", b"plan content");
/// assert_ne!(a, forged);
/// ```
pub fn hmac_sha256_hex(secret: &[u8], data: &[u8]) -> String {
    // `new_from_slice` never fails for HMAC (any key length is valid; short
    // keys are zero-padded, long keys are hashed down internally).
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any length");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Verifies that `expected_hex` is the correct HMAC-SHA256 of `data` under
/// `secret`, using a constant-time comparison (via `hmac`'s `verify_slice`)
/// so the check does not leak timing information about how much of the MAC
/// matched.
///
/// ```
/// use osx_clnr::domain::crypto::{hmac_sha256_hex, verify_hmac_sha256};
///
/// let secret = b"real-secret";
/// let mac = hmac_sha256_hex(secret, b"plan content");
///
/// // Positive: the real secret verifies its own signature.
/// assert!(verify_hmac_sha256(secret, b"plan content", &mac));
///
/// // Refusal: an attacker who doesn't know the secret, and hand-computes a
/// // plain hash or guesses a key, is rejected.
/// assert!(!verify_hmac_sha256(b"guessed-key", b"plan content", &mac));
///
/// // Refusal: tampering with the signed data after signing is caught too.
/// assert!(!verify_hmac_sha256(secret, b"tampered content", &mac));
///
/// // Refusal: a malformed (non-hex) signature is rejected, not panicked on.
/// assert!(!verify_hmac_sha256(secret, b"plan content", "not-valid-hex"));
/// ```
pub fn verify_hmac_sha256(secret: &[u8], data: &[u8], expected_hex: &str) -> bool {
    let Ok(expected_bytes) = hex::decode(expected_hex) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret) else {
        return false;
    };
    mac.update(data);
    mac.verify_slice(&expected_bytes).is_ok()
}
