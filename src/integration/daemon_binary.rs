//! Stable, oclnr-owned binary location for launchd agents, plus the preflight
//! that runs it.
//!
//! Plists used to bake whatever `which oclnr` found (on this machine a stale
//! `~/.local/bin/oclnr` that `just install` — i.e. `cargo install` into
//! `~/.cargo/bin` — never updates). Installing an agent now copies the
//! *running* binary to `~/.oclnr/bin/oclnr`, so the agent runs exactly the
//! build that generated its plist, and `cargo clean` of a checkout cannot
//! remove it out from under launchd.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::domain::daemon_preflight::{missing_flags, program_arguments, requirements};

/// Where agent binaries live: `~/.oclnr/bin/oclnr`.
pub fn installed_binary_path() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".oclnr/bin/oclnr")
}

/// Copies `source` to `dest` atomically (temp file + rename, mode 0755).
pub fn install_binary(source: &Path, dest: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = dest.parent().context("install destination has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".oclnr.tmp.{}", std::process::id()));
    std::fs::copy(source, &tmp)
        .with_context(|| format!("copying {} to {}", source.display(), tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp, dest)
        .with_context(|| format!("installing binary at {}", dest.display()))?;
    Ok(())
}

/// Installs the currently running executable at [`installed_binary_path`].
pub fn install_self() -> anyhow::Result<PathBuf> {
    let me = std::env::current_exe().context("resolving the running oclnr binary")?;
    let dest = installed_binary_path();
    if std::fs::canonicalize(&me).ok() != std::fs::canonicalize(&dest).ok() {
        install_binary(&me, &dest)?;
    }
    Ok(dest)
}

/// Runs the plist's binary with `<subcommand…> --help` and returns the flags
/// the plist passes that the binary does not accept. `Err` when the binary
/// cannot be run or rejects the subcommand path itself.
pub fn preflight_plist(plist_contents: &str) -> anyhow::Result<Vec<String>> {
    let args = program_arguments(plist_contents);
    let bin = args.first().context("plist has no ProgramArguments")?;
    let (sub, flags) = requirements(&args);
    let out = std::process::Command::new(bin)
        .args(&sub)
        .arg("--help")
        .output()
        .with_context(|| format!("running {bin} for preflight"))?;
    if !out.status.success() {
        anyhow::bail!(
            "{bin} {} --help failed: {}",
            sub.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(missing_flags(&String::from_utf8_lossy(&out.stdout), &flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist_for(bin: &Path, args: &[&str]) -> String {
        let mut s =
            format!("<dict><key>ProgramArguments</key><array><string>{}</string>", bin.display());
        for a in args {
            s.push_str(&format!("<string>{a}</string>"));
        }
        s + "</array></dict>"
    }

    /// A real executable standing in for an old oclnr build: its `monitor
    /// --help` knows `--threshold-gb` but not `--reclaim`.
    fn stale_binary(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("old-oclnr");
        std::fs::write(
            &p,
            "#!/bin/sh\n[ \"$1\" = monitor ] && [ \"$2\" = --help ] && \
             { echo '      --threshold-gb <GB>'; echo '  -h, --help'; exit 0; }\n\
             echo \"error: unrecognized subcommand '$1'\" >&2; exit 2\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn stale_binary_is_caught_before_launchd_crash_loops_it() {
        let dir = tempfile::tempdir().unwrap();
        let bin = stale_binary(dir.path());
        let ok = plist_for(&bin, &["monitor", "--threshold-gb", "20"]);
        assert!(preflight_plist(&ok).unwrap().is_empty());

        let stale = plist_for(&bin, &["monitor", "--threshold-gb", "20", "--reclaim", "snapshots"]);
        assert_eq!(preflight_plist(&stale).unwrap(), vec!["--reclaim".to_string()]);

        // Unknown subcommand is an error, not "nothing missing".
        let wrong = plist_for(&bin, &["autoclean", "run", "--yes"]);
        assert!(preflight_plist(&wrong).is_err());
    }

    #[test]
    fn install_binary_copies_executable_atomically() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let src = stale_binary(dir.path());
        let dest = dir.path().join("nested/bin/oclnr");
        install_binary(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(&dest).unwrap());
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!dest.parent().unwrap().read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".oclnr.tmp")));
    }
}
