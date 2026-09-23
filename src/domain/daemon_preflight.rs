//! Launch-agent preflight: does the binary a plist points at actually accept
//! the command line the plist passes it?
//!
//! On 2026-09-22 `com.oclnr.pressure` was installed against `~/.local/bin/oclnr`
//! (resolved via `which`), a Sep 19 build that predated `monitor --reclaim`.
//! launchd crash-looped it ("unexpected argument '--reclaim'") from install
//! onward, and a trigger test run against `target/release` "witnessed" the
//! wrong binary. This module derives, from the plist itself, the subcommand
//! path and flags the job will pass, and checks them against that binary's
//! `--help` text. Pure: the integration layer runs the binary.

/// The `<string>` entries of a plist's `ProgramArguments` array, in order.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::daemon_preflight::program_arguments;
/// let plist = "<dict><key>Label</key><string>x</string>\
///   <key>ProgramArguments</key><array><string>/b/oclnr</string>\
///   <string>monitor</string><string>--watch</string></array>\
///   <key>RunAtLoad</key><true/></dict>";
/// assert_eq!(program_arguments(plist), vec!["/b/oclnr", "monitor", "--watch"]);
///
/// // Negative: no ProgramArguments key → empty, never the Label string.
/// assert!(program_arguments("<dict><key>Label</key><string>x</string></dict>").is_empty());
/// ```
pub fn program_arguments(plist: &str) -> Vec<String> {
    let Some(idx) = plist.find("<key>ProgramArguments</key>") else { return vec![] };
    let rest = &plist[idx..];
    let (Some(a), Some(b)) = (rest.find("<array>"), rest.find("</array>")) else { return vec![] };
    if b < a {
        return vec![];
    }
    let mut body = &rest[a + "<array>".len()..b];
    let mut out = Vec::new();
    while let Some(s) = body.find("<string>") {
        let start = s + "<string>".len();
        let Some(len) = body[start..].find("</string>") else { break };
        out.push(body[start..start + len].to_string());
        body = &body[start + len + "</string>".len()..];
    }
    out
}

/// Splits `args` (binary first) into the subcommand path — leading words
/// before the first `-`-prefixed argument — and the long flags passed.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::daemon_preflight::requirements;
/// let args: Vec<String> = ["/b/oclnr", "autoclean", "run", "--yes", "--max-reclaim-gb", "50"]
///     .iter().map(|s| s.to_string()).collect();
/// let (sub, flags) = requirements(&args);
/// assert_eq!(sub, vec!["autoclean", "run"]);
/// assert_eq!(flags, vec!["--yes", "--max-reclaim-gb"]);
///
/// // `--flag=value` contributes only the flag name.
/// let args: Vec<String> = ["/b", "monitor", "--reclaim=snapshots"].iter().map(|s| s.to_string()).collect();
/// assert_eq!(requirements(&args).1, vec!["--reclaim"]);
///
/// // Refusal boundary: a binary with no arguments requires nothing.
/// assert_eq!(requirements(&["/b".to_string()]), (vec![], vec![]));
/// ```
pub fn requirements(args: &[String]) -> (Vec<String>, Vec<String>) {
    let rest = args.get(1..).unwrap_or(&[]);
    let sub: Vec<String> = rest.iter().take_while(|a| !a.starts_with('-')).cloned().collect();
    let flags = rest
        .iter()
        .filter(|a| a.starts_with("--"))
        .map(|a| a.split('=').next().unwrap_or(a).to_string())
        .collect();
    (sub, flags)
}

/// Flags from `flags` that do not appear as whole tokens in `help`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::daemon_preflight::missing_flags;
/// let help = "Options:\n      --threshold-gb <GB>\n      --watch\n  -h, --help";
/// let want = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
///
/// // Positive: everything present.
/// assert!(missing_flags(help, &want(&["--watch", "--threshold-gb"])).is_empty());
///
/// // Negative: the stale-binary case — `--reclaim` unknown to this build.
/// assert_eq!(missing_flags(help, &want(&["--watch", "--reclaim"])), want(&["--reclaim"]));
///
/// // Refusal: a prefix is not a match (`--watch` ≠ `--watch-dir`).
/// assert_eq!(missing_flags("  --watch-dir <D>", &want(&["--watch"])), want(&["--watch"]));
/// ```
pub fn missing_flags(help: &str, flags: &[String]) -> Vec<String> {
    let tokens: std::collections::HashSet<&str> = help
        .split(|c: char| c.is_whitespace() || c == ',' || c == '=' || c == '[' || c == ']')
        .filter(|t| t.starts_with("--"))
        .collect();
    flags.iter().filter(|f| !tokens.contains(f.as_str())).cloned().collect()
}
