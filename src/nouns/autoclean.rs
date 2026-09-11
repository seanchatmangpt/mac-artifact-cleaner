//! Autoclean: unattended, safe, recurring cleanup pipeline for the launchd
//! `daemon install --mode autoclean` job (see `daemon.rs`). Orchestrates the
//! existing, individually-tested CLI subcommands as subprocesses of the
//! currently-running binary — the same trusted pattern the MCP server uses
//! (`src/mcp/subprocess.rs`) — rather than duplicating their logic in a new
//! code path.
//!
//! Safety posture, strictly more conservative than an interactive session:
//! - Every stage is a real, unmodified `oclnr` subcommand: `plan build`,
//!   `plan approve`, `delete execute`, `receipt verify`.
//! - `plan build` never nominates a wholesale `~/Library/Caches` (see
//!   `domain::artifact::global_cache_candidates`) and never touches
//!   Docker/Colima (no detector exists for either, and the project's own
//!   safety hook independently blocks raw `colima`/`docker prune` commands
//!   for any interactive session — this job has no path to them at all).
//! - A hard `--max-reclaim-gb` cap refuses to approve/execute any single
//!   run's plan larger than the cap, regardless of what the scanner finds —
//!   a backstop against a detection bug nominating something huge.
//! - A plan containing any Unknown/Irreversible-reversibility item is never
//!   approved unattended (unlike an interactive session, which can pass
//!   `--acknowledge-unknown-reversibility` after a human looks) — it is
//!   skipped and logged for manual review instead.
//! - `--ignore-recent-hours` defaults to 24h, so anything touched in the
//!   last day (active work) is left alone.
//! - Every run's plan/receipt files and a one-line summary are appended to
//!   `~/Library/Logs/oclnr/autoclean.log`, so a run nobody watched is still
//!   fully auditable afterward.

use std::{path::PathBuf, process::Command};

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum AutocleanAction {
    /// Run one safe cleanup pass: build a plan, approve it if (and only if)
    /// it passes the safety cap and reversibility check, execute, verify.
    Run {
        /// Hard cap in GB — a plan claiming more than this is refused
        /// (logged, not deleted) rather than approved.
        #[arg(long, default_value_t = 50.0)]
        max_reclaim_gb: f64,
        /// Ignore projects modified within the specified number of hours
        /// (default: 24h — never touch anything from today's active work).
        #[arg(long, default_value_t = 24)]
        ignore_recent_hours: u64,
        /// Required: this command can delete files.
        #[arg(long)]
        yes: bool,
    },
}

pub fn handle(action: AutocleanAction) -> anyhow::Result<()> {
    match action {
        AutocleanAction::Run { max_reclaim_gb, ignore_recent_hours, yes } => {
            run(max_reclaim_gb, ignore_recent_hours, yes)
        }
    }
}

/// Pure safety-cap check: does `total_bytes` exceed the `max_reclaim_gb`
/// cap? Extracted as a standalone function so the cap boundary is
/// unit-testable without a real filesystem scan or subprocess.
///
/// # Examples
///
/// ```
/// use osx_clnr::nouns::autoclean::exceeds_cap;
///
/// // Positive case: comfortably under the cap.
/// assert!(!exceeds_cap(10 * 1024 * 1024 * 1024, 50.0));
///
/// // Refusal case: exactly at the cap is NOT a refusal (`>`, not `>=`) —
/// // an exact-cap plan is still within budget.
/// assert!(!exceeds_cap(50 * 1024 * 1024 * 1024, 50.0));
///
/// // Positive case: one byte over the cap is refused.
/// assert!(exceeds_cap(50 * 1024 * 1024 * 1024 + 1, 50.0));
/// ```
pub fn exceeds_cap(total_bytes: u64, max_reclaim_gb: f64) -> bool {
    let cap_bytes = (max_reclaim_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    total_bytes > cap_bytes
}

fn log_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("no home directory"))?
        .join("Library/Logs/oclnr");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn append_log(line: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let dir = log_dir()?;
    let mut f =
        std::fs::OpenOptions::new().create(true).append(true).open(dir.join("autoclean.log"))?;
    writeln!(f, "{}", line)?;
    Ok(())
}

fn run(max_reclaim_gb: f64, ignore_recent_hours: u64, yes: bool) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!(
            "Refusing to run without --yes (this can delete files). The launchd job always \
             passes --yes; pass it explicitly to test manually."
        );
    }

    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = log_dir()?;
    let plan_file = dir.join(format!("{ts}-plan.json"));
    let receipt_file = dir.join(format!("{ts}-receipt.jsonocel"));
    // Resolve the binary's own stable path once, up front — every stage
    // below re-execs *this* binary, so it must be installed somewhere
    // outside any path it might scan/delete (e.g. `/usr/local/bin`, never
    // a project `target/` dir it could nominate for cleanup itself).
    let exe = std::env::current_exe()?;

    println!("[autoclean {ts}] building plan (ignore_recent_hours={ignore_recent_hours})...");
    let build = Command::new(&exe)
        .args([
            "plan",
            "build",
            "--deps",
            "--aggressive",
            "--ignore-recent-hours",
            &ignore_recent_hours.to_string(),
            "--include-global-caches",
            "--output",
        ])
        .arg(&plan_file)
        .output()?;
    if !build.status.success() {
        let msg = format!(
            "[autoclean {ts}] plan build FAILED: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        anyhow::bail!("plan build failed");
    }

    let plan_content = std::fs::read_to_string(&plan_file)?;
    let plan: crate::domain::plan::DeletionPlan = serde_json::from_str(&plan_content)?;
    let total_bytes: u64 = plan.items.iter().map(|i| i.bytes).sum();

    if plan.items.is_empty() {
        let msg = format!("[autoclean {ts}] nothing to clean this run.");
        println!("{msg}");
        append_log(&msg)?;
        return Ok(());
    }

    if exceeds_cap(total_bytes, max_reclaim_gb) {
        let msg = format!(
            "[autoclean {ts}] REFUSED: plan claims {} which exceeds the {} GB safety cap — not \
             approving. Review manually: oclnr plan inspect --plan {}",
            crate::integration::progress::human_bytes(total_bytes),
            max_reclaim_gb,
            plan_file.display()
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        return Ok(());
    }

    // Never proceed past an Unknown/Irreversible item unattended — an
    // autonomous job must be strictly more conservative than an interactive
    // session (which can pass --acknowledge-unknown-reversibility after a
    // human actually looks). Skip and log instead of overriding it here.
    let non_reversible_count = plan
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.reversibility,
                crate::domain::dcm::Reversibility::Unknown
                    | crate::domain::dcm::Reversibility::Irreversible
            )
        })
        .count();
    if non_reversible_count > 0 {
        let msg = format!(
            "[autoclean {ts}] SKIPPED: plan contains {non_reversible_count} unknown/irreversible-\
             reversibility item(s) — autoclean never overrides that unattended. Review manually: \
             oclnr plan inspect --plan {}",
            plan_file.display()
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        return Ok(());
    }

    println!(
        "[autoclean {ts}] approving plan ({} items, {})...",
        plan.items.len(),
        crate::integration::progress::human_bytes(total_bytes)
    );
    let approve = Command::new(&exe)
        .args(["plan", "approve", "--plan"])
        .arg(&plan_file)
        .args([
            "--approver",
            "oclnr-autoclean",
            "--reason",
            "recurring unattended safe cleanup (launchd, under safety cap, all-reversible)",
            "--yes",
        ])
        .output()?;
    if !approve.status.success() {
        let msg = format!(
            "[autoclean {ts}] plan approve FAILED: {}",
            String::from_utf8_lossy(&approve.stderr)
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        anyhow::bail!("plan approve failed");
    }

    println!("[autoclean {ts}] executing...");
    let execute = Command::new(&exe)
        .args(["delete", "execute", "--plan"])
        .arg(&plan_file)
        .args(["--receipt"])
        .arg(&receipt_file)
        .arg("--yes")
        .output()?;
    // `delete execute`'s exit code now reflects this session's
    // space-verification-shortfall fix (see `nouns::delete`): it exits 0
    // even when local APFS/Time Machine snapshots are retaining freed
    // blocks. A nonzero exit here is therefore a *real* failure worth
    // treating as one, not that prior false negative.
    let exec_stdout = String::from_utf8_lossy(&execute.stdout).to_string();
    if !execute.status.success() {
        let msg = format!(
            "[autoclean {ts}] delete execute FAILED (exit {:?}): {}\n{}",
            execute.status.code(),
            String::from_utf8_lossy(&execute.stderr),
            exec_stdout
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        anyhow::bail!("delete execute failed");
    }

    let verify = Command::new(&exe)
        .args(["receipt", "verify", "--receipt"])
        .arg(&receipt_file)
        .args(["--plan"])
        .arg(&plan_file)
        .output()?;

    let msg = format!(
        "[autoclean {ts}] done. plan={} receipt={} verify_exit={:?}",
        plan_file.display(),
        receipt_file.display(),
        verify.status.code()
    );
    println!("{msg}");
    append_log(&msg)?;

    // Default posture for every cleaning run: local Time Machine snapshots
    // are thinned to only the single most recent one. File deletion alone
    // often shows no visible free-space gain until snapshots pinning the
    // deleted blocks are cleared too (the user's standing disk-cleanup
    // rule) — so this runs unconditionally after a successful delete, not
    // as an opt-in extra step. Deliberately non-fatal: it only ever
    // touches dated `com.apple.TimeMachine.*` local snapshots (see
    // `parse_snapshot_date` — OS-update snapshots never match and are left
    // alone), and a failure here must never make an already-successful,
    // already-verified file cleanup report as failed.
    let snapshot_receipt = dir.join(format!("{ts}-snapshot-receipt.jsonocel"));
    let snapshot_thin = Command::new(&exe)
        .args(["snapshot", "delete", "--which", "keep-latest", "--receipt"])
        .arg(&snapshot_receipt)
        .output();
    match snapshot_thin {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let summary_line = stdout
                .lines()
                .find(|l| l.starts_with("Deleted "))
                .unwrap_or("snapshot thin: no summary line")
                .to_string();
            let msg = format!("[autoclean {ts}] snapshot keep-latest: {summary_line}");
            println!("{msg}");
            append_log(&msg)?;
        }
        Ok(out) => {
            let msg = format!(
                "[autoclean {ts}] snapshot keep-latest FAILED (non-fatal, file cleanup already \
                 succeeded): {}",
                String::from_utf8_lossy(&out.stderr)
            );
            eprintln!("{msg}");
            append_log(&msg)?;
        }
        Err(e) => {
            let msg = format!(
                "[autoclean {ts}] snapshot keep-latest could not be spawned (non-fatal): {e}"
            );
            eprintln!("{msg}");
            append_log(&msg)?;
        }
    }

    Ok(())
}
