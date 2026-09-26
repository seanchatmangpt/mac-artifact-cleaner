//! Stamps the commit the binary was built from into `OCLNR_BUILD_SHA`, so
//! receipt projections (`domain::r_projection`) can pin `identity.subject_sha`
//! to a real commit. Falls back to "unknown" (which projections refuse to
//! certify) when git is unavailable.

fn main() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(true);
    println!("cargo:rustc-env=OCLNR_BUILD_SHA={sha}");
    println!("cargo:rustc-env=OCLNR_BUILD_DIRTY={dirty}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
