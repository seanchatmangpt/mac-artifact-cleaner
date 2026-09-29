//! Real-filesystem tests for the read-only launchd orphan scan. Plists are
//! real files in a tempdir, converted by the real `plutil`; no doubles.

use std::{fs, path::Path};

use osx_clnr::{
    domain::launchd_orphans::{find_orphans, summarize},
    integration::launchd_scan::scan_launchd_dirs,
};

fn plist(dir: &Path, name: &str, label: &str, program_key: &str, program: &str) {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>{label}</string>\n{program_key}\n\
         <string>{program}</string></dict></plist>\n"
    );
    fs::write(dir.join(name), body).unwrap();
}

#[test]
fn scanner_reports_missing_absolute_program_only() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    plist(d, "ok.plist", "t.ok", "<key>Program</key>", "/bin/ls");
    plist(d, "gone.plist", "t.gone", "<key>Program</key>", "/nonexistent/oclnr-test/bin");
    plist(d, "relative.plist", "t.rel", "<key>Program</key>", "ls");
    // ProgramArguments form with the first element missing.
    let args =
        "<key>ProgramArguments</key><array><string>/nonexistent/oclnr-test/args</string></array>";
    fs::write(
        d.join("args.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
             <key>Label</key><string>t.args</string>{args}</dict></plist>"
        ),
    )
    .unwrap();
    // No program key at all.
    fs::write(
        d.join("noprog.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
         <key>Label</key><string>t.noprog</string></dict></plist>",
    )
    .unwrap();
    // Malformed.
    fs::write(d.join("bad.plist"), "this is not a plist").unwrap();
    // Non-plist file ignored.
    fs::write(d.join("notes.txt"), "x").unwrap();

    let missing = d.join("does-not-exist");
    let scan = scan_launchd_dirs(&[d.to_path_buf(), missing.clone()]);

    assert_eq!(scan.facts.len(), 5, "facts: {:?}", scan.facts);
    assert_eq!(scan.errors.len(), 1);
    assert!(scan.errors[0].path.ends_with("bad.plist"));
    assert_eq!(scan.missing_dirs, vec![missing.display().to_string()]);

    let orphans = find_orphans(&scan.facts);
    let labels: Vec<_> = orphans.iter().map(|o| o.label.clone().unwrap()).collect();
    assert_eq!(labels, ["t.args", "t.gone"]);

    let s = summarize(&scan.facts);
    assert_eq!((s.total, s.ok, s.orphans, s.unknown), (5, 1, 2, 2));

    // Read-only: every fixture file is still present.
    for n in ["ok", "gone", "relative", "args", "noprog", "bad"] {
        assert!(d.join(format!("{n}.plist")).exists());
    }
}
