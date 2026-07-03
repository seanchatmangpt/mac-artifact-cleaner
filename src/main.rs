//! osx-clnr CLI binary entrypoint.

// NOTE on the removed `github`-only dispatch path (see git history for the
// prior version of this function):
//
// This used to special-case `oclnr github <subcommand>` by routing it
// through `clap_noun_verb::run()` instead of the normal
// `Cli::parse()` -> `osx_clnr::nouns::handle_cli()` flow used by every
// other noun. `clap_noun_verb::run()` builds its own, entirely separate
// argv parser from a global registry of `#[clap_noun_verb_macros::verb(...)]`
// -annotated functions (see `src/nouns/github.rs`), so it never goes
// through `nouns::Cli` (the clap-derived struct with the global `--policy`
// flag) at all. That meant `oclnr github <subcommand> --policy <path>`
// silently ignored `--policy`: the policy was never loaded and
// `nouns::POLICY` was never populated for that invocation.
//
// Investigation showed this bypass was unnecessary: `nouns::Command` already
// has a full `Github { action: github::GithubAction }` variant (a normal
// `#[derive(Subcommand)]`), and `nouns::handle_cli()` already dispatches it
// via `github::handle(action)`, which calls the exact same underlying
// functions (`github_scan`, `github_plan`, `github_delete`,
// `github_receipt`) that the `#[verb(...)]` macros register with
// `clap_noun_verb`. There is no passthrough/raw-args need here — GithubAction
// is a fully modeled clap subcommand. So the second dispatch path was pure
// duplication that happened to skip policy loading; it is not needed for
// `oclnr github ...` to work, and removing it makes `--policy` apply
// uniformly across every noun.
fn main() -> anyhow::Result<()> {
    osx_clnr::nouns::handle_cli()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// Regression test for the bug where `oclnr github <subcommand>` bypassed
    /// the normal `Cli::parse()` flow and silently ignored `--policy`.
    ///
    /// Before the fix, `main.rs` special-cased `args[1] == "github"` and
    /// routed to `clap_noun_verb::run()`, which never reads `--policy` and
    /// never populates `nouns::POLICY`. A bogus `--policy` path would be
    /// silently ignored and the command would proceed (or fail for
    /// unrelated reasons, e.g. missing `gh` binary / auth), instead of
    /// erroring clearly about the missing/invalid policy file.
    ///
    /// After the fix, `github` goes through the same `Cli::parse()` ->
    /// `handle_cli()` path as every other noun, so a bad `--policy` value
    /// must surface as a policy-load error before any github-specific logic
    /// runs.
    #[test]
    fn github_subcommand_honors_global_policy_flag() {
        // `CARGO_BIN_EXE_oclnr` is only populated for integration tests under
        // `tests/`, not for unit tests compiled into the `oclnr` binary
        // itself. Locate the sibling `oclnr` binary relative to this test
        // binary's own path instead (both live in `target/<profile>/`).
        let mut bin_path = std::env::current_exe().expect("failed to get current exe path");
        bin_path.pop(); // deps/
        bin_path.pop(); // <profile>/
        bin_path.push(if cfg!(windows) { "oclnr.exe" } else { "oclnr" });
        assert!(
            bin_path.is_file(),
            "expected oclnr binary at {bin_path:?}; build it first with `cargo build --bin oclnr`"
        );

        let output = Command::new(bin_path)
            .args([
                "--policy",
                "/nonexistent/path/definitely-not-here/OCLNR.yaml",
                "github",
                "scan",
            ])
            .output()
            .expect("failed to run oclnr binary");

        assert!(
            !output.status.success(),
            "expected failure due to invalid --policy path, but command succeeded"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.to_lowercase().contains("policy")
                || stderr.to_lowercase().contains("no such file")
                || stderr.to_lowercase().contains("not found"),
            "expected a policy-related error in stderr, got: {stderr}"
        );
    }
}
