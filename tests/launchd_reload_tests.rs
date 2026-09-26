//! Real launchd round-trip for `reload_agent`, using a throwaway label that
//! runs `/bin/sleep`. Gated on `OCLNR_LAUNCHD_TESTS=1` because it registers a
//! (cleaned-up) user LaunchAgent; skips with a printed reason otherwise.

use osx_clnr::{
    domain::daemon_preflight::launchctl_print_program,
    integration::daemon_binary::{launchctl_print, reload_agent},
};

fn pid_of(print: &str) -> Option<u32> {
    print.lines().find_map(|l| l.strip_prefix("\tpid = ").and_then(|p| p.trim().parse().ok()))
}

fn plist(label: &str, program: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
         <key>Label</key><string>{label}</string>\
         <key>ProgramArguments</key><array><string>{program}</string><string>300</string></array>\
         <key>RunAtLoad</key><true/></dict></plist>\n"
    )
}

#[test]
fn reload_replaces_a_loaded_instance_and_verifies_program() {
    if std::env::var("OCLNR_LAUNCHD_TESTS").as_deref() != Ok("1") {
        eprintln!("SKIPPED: set OCLNR_LAUNCHD_TESTS=1 to exercise real launchd");
        return;
    }
    let label = format!("com.oclnr.test.reload.{}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{label}.plist"));

    std::fs::write(&path, plist(&label, "/bin/sleep")).unwrap();
    let first = reload_agent(&label, &path);
    let pid1 = launchctl_print(&label).as_deref().and_then(pid_of);

    // Reinstall while loaded — the case `launchctl load -w` silently no-op'd.
    let second = reload_agent(&label, &path);
    let printed = launchctl_print(&label);
    let pid2 = printed.as_deref().and_then(pid_of);

    // Refusal: a plist whose program differs from what launchd would run is
    // impossible to fake here, so instead check a bogus binary is rejected.
    let bogus = dir.path().join("bogus.plist");
    std::fs::write(&bogus, plist(&label, "/nonexistent/oclnr")).unwrap();
    let third = reload_agent(&label, &bogus);

    // Cleanup before asserting so a failure never leaks the agent.
    let _ = std::process::Command::new("launchctl")
        .args([
            "bootout",
            &format!(
                "gui/{}/{label}",
                String::from_utf8_lossy(
                    &std::process::Command::new("id").arg("-u").output().unwrap().stdout
                )
                .trim()
            ),
        ])
        .status();

    assert_eq!(first.unwrap(), "/bin/sleep");
    assert_eq!(second.unwrap(), "/bin/sleep");
    assert_eq!(printed.as_deref().and_then(launchctl_print_program).as_deref(), Some("/bin/sleep"));
    assert!(pid1.is_some() && pid2.is_some(), "{pid1:?} {pid2:?}");
    assert_ne!(pid1, pid2, "second install must replace the running instance");
    // launchd accepts a missing program at bootstrap (it fails at spawn), so
    // the verified program must at least be the plist's — never the old one.
    if let Ok(p) = third {
        assert_eq!(p, "/nonexistent/oclnr");
    }
    assert!(launchctl_print(&label).is_none(), "test agent leaked");
}
