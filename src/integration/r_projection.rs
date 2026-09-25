//! Writes the `R = receipt(A)` projection (`domain::r_projection`) next to a
//! native receipt as `<stem>.r.json`, filling the invocation context the
//! native receipt does not carry: build commit, command line, cwd, and the
//! sha256 of the native file's exact on-disk bytes.

use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};

use crate::domain::r_projection::{ProjectionContext, RReceipt};

/// Commit this binary was built from (see `build.rs`).
pub const BUILD_SHA: &str = env!("OCLNR_BUILD_SHA");
/// Whether that build had uncommitted tracked changes (see `build.rs`).
pub const BUILD_DIRTY: &str = env!("OCLNR_BUILD_DIRTY");

/// Path of the projection for a native receipt: `x.jsonocel` → `x.r.json`.
pub fn projection_path(native: &Path) -> PathBuf {
    native.with_extension("r.json")
}

/// Builds the invocation context for `native` (which must already be written:
/// its bytes are hashed so the projection pins the exact native receipt).
pub fn context_for(
    native: &Path,
    actor: &str,
    grant: &str,
    work_order_id: &str,
    exit: i32,
) -> anyhow::Result<ProjectionContext> {
    let bytes = std::fs::read(native)
        .with_context(|| format!("reading native receipt {}", native.display()))?;
    let native_path = std::fs::canonicalize(native).unwrap_or_else(|_| native.to_path_buf());
    Ok(ProjectionContext {
        repo: env!("CARGO_MANIFEST_DIR").to_string(),
        build_sha: BUILD_SHA.to_string(),
        build_dirty: BUILD_DIRTY != "false",
        actor: actor.to_string(),
        grant: grant.to_string(),
        cmd: std::env::args().collect::<Vec<_>>().join(" "),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into()),
        exit,
        native_sha256: hex::encode(Sha256::digest(&bytes)),
        native_path: native_path.display().to_string(),
        work_order_id: work_order_id.to_string(),
    })
}

/// Serializes and writes `r` to [`projection_path`]`(native)`.
pub fn write_projection(native: &Path, r: &RReceipt) -> anyhow::Result<PathBuf> {
    let out = projection_path(native);
    std::fs::write(&out, serde_json::to_vec_pretty(r)?)
        .with_context(|| format!("writing receipt projection {}", out.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{r_projection::project_snapshot_thin, time::SnapshotThinReceipt};

    #[test]
    fn projection_pins_native_bytes_and_lands_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("thin-receipt.json");
        let receipt = SnapshotThinReceipt::new("/".into(), 5, 0, vec!["s1".into()], vec![]);
        let bytes = serde_json::to_vec_pretty(&receipt).unwrap();
        std::fs::write(&native, &bytes).unwrap();

        let ctx = context_for(&native, "test", "test-grant", "wo-test", 0).unwrap();
        assert_eq!(ctx.native_sha256, hex::encode(Sha256::digest(&bytes)));
        let out = write_projection(&native, &project_snapshot_thin(&receipt, &ctx)).unwrap();
        assert_eq!(out, dir.path().join("thin-receipt.r.json"));

        let back: RReceipt = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(back.replay.commands[0].output_sha256, ctx.native_sha256);
        assert_eq!(back.identity.subject_sha, BUILD_SHA);
        assert_eq!(back.work_order_id, "wo-test");
        assert_eq!(back.provider_execution_id, format!("oclnr:sha256:{}", ctx.native_sha256));
    }

    #[test]
    fn missing_native_receipt_is_an_error_not_a_blank_hash() {
        let dir = tempfile::tempdir().unwrap();
        assert!(context_for(&dir.path().join("absent.json"), "a", "g", "wo", 0).is_err());
    }
}
