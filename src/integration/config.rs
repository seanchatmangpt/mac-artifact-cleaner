//! Policy loading via star-toml admission.
//!
//! The integration layer owns all filesystem access: this module reads
//! `osxclnr.toml` (or an explicit `--policy` path) and admits it through
//! `star_toml::TrustedLoader`, so the rest of the program only ever sees a
//! validated, normalized [`OclnrPolicy`]. Environment overrides use the
//! `OCLNR_` prefix (e.g. `OCLNR_RETENTION_HOURS=24`).

use std::path::Path;

use anyhow::{anyhow, Context};
use star_toml::loader::TrustedLoader;

use crate::domain::policy::OclnrPolicy;

/// Loads and admits the cleanup policy.
///
/// - `explicit`: a `--policy <path>` argument. The file must exist and admit
///   cleanly; failure is an error (the user asked for this exact policy).
/// - Otherwise `osxclnr.toml` in the current directory is layered in if
///   present; with no file at all, the built-in [`OclnrPolicy::default`] is
///   returned (after the same invariant check, so a broken default can never
///   ship silently).
pub fn load_policy(explicit: Option<&Path>) -> anyhow::Result<OclnrPolicy> {
    match explicit {
        Some(path) => {
            if !path.exists() {
                return Err(anyhow!("policy file not found: {}", path.display()));
            }
            admit_file(path)
        }
        None => {
            let default_path = Path::new("osxclnr.toml");
            if default_path.exists() {
                admit_file(default_path)
            } else {
                let policy = OclnrPolicy::default();
                let violations = policy.invariant_violations();
                if violations.is_empty() {
                    Ok(policy)
                } else {
                    Err(anyhow!("built-in default policy is invalid: {}", violations.join("; ")))
                }
            }
        }
    }
}

fn admit_file(path: &Path) -> anyhow::Result<OclnrPolicy> {
    TrustedLoader::new()
        .layer_file_if_exists(path)
        .env_prefix("OCLNR_")
        .load_admitted::<OclnrPolicy>()
        .map(|admitted| admitted.into_value())
        .with_context(|| format!("failed to admit policy from {}", path.display()))
}
