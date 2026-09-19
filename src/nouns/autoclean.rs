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
//! - A hard `--max-reclaim-gb` cap bounds how much any single run may delete:
//!   a plan larger than the cap is trimmed largest-first to fit (the remainder
//!   defers to the next scheduled run), and a plan whose *single largest item*
//!   exceeds the cap is refused outright — the backstop against a detection
//!   bug nominating something huge.
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

use crate::domain::ocel::{build_autoclean_run_ocel, AutocleanRunFacts};

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
    /// Show the standing of unattended runs, parsed from
    /// `~/Library/Logs/oclnr/autoclean.log`: last outcome, freed bytes,
    /// consecutive failures, and run counts. Read-only.
    Status,
}

pub fn handle(action: AutocleanAction) -> anyhow::Result<()> {
    match action {
        AutocleanAction::Run { max_reclaim_gb, ignore_recent_hours, yes } => {
            run(max_reclaim_gb, ignore_recent_hours, yes)
        }
        AutocleanAction::Status => print_status(),
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

/// Splits plan items into `(kept, deferred)` so the kept set's total never
/// exceeds `cap_bytes` — the cap as a *per-run budget*, not a veto.
///
/// Items are considered largest-first (defensively re-sorted; `plan build`
/// already emits them that way) so each run removes the biggest wins and
/// defers the remainder to the next run. An item that alone exceeds the cap
/// is always deferred, never partially kept — so a runaway nomination
/// (the detection-bug scenario the cap exists to backstop) still results in
/// an empty `kept` set and the caller's refuse-and-log path, never in a
/// silently-truncated deletion of something huge.
///
/// Pure: no filesystem access.
///
/// # Examples
///
/// ```
/// use osx_clnr::nouns::autoclean::trim_plan_items_to_cap;
/// use osx_clnr::domain::plan::{PlanItem, PlanItemKind};
/// use osx_clnr::domain::dcm::Reversibility;
/// use std::path::PathBuf;
///
/// let item = |gb: u64| PlanItem {
///     path: PathBuf::from(format!("/tmp/item-{gb}")),
///     kind: PlanItemKind::Dir,
///     reason: "rust target".to_string(),
///     bytes: gb * 1024 * 1024 * 1024,
///     reversibility: Reversibility::Reversible,
/// };
///
/// // 25+25+25 GB against a 50 GB budget: the first two fill it exactly, the
/// // third defers.
/// let (kept, deferred) = trim_plan_items_to_cap(vec![item(25), item(25), item(25)], 50.0);
/// assert_eq!(kept.len(), 2);
/// assert_eq!(deferred.len(), 1);
/// assert_eq!(deferred[0].bytes, 25 * 1024 * 1024 * 1024);
///
/// // A single item larger than the whole cap defers everything — the caller
/// // must refuse the run (empty kept set), never truncate the item.
/// let (kept, deferred) = trim_plan_items_to_cap(vec![item(80)], 50.0);
/// assert!(kept.is_empty());
/// assert_eq!(deferred.len(), 1);
///
/// // Largest-first even if the input arrives unsorted: 40+10 fill the 50 GB
/// // budget exactly (exact fit is within budget), 30 defers.
/// let (kept, deferred) = trim_plan_items_to_cap(vec![item(10), item(40), item(30)], 50.0);
/// assert_eq!(kept.iter().map(|i| i.bytes).sum::<u64>(), 50 * 1024 * 1024 * 1024);
/// assert_eq!(deferred.len(), 1);
/// ```
pub fn trim_plan_items_to_cap(
    items: Vec<crate::domain::plan::PlanItem>,
    max_reclaim_gb: f64,
) -> (Vec<crate::domain::plan::PlanItem>, Vec<crate::domain::plan::PlanItem>) {
    let cap_bytes = (max_reclaim_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    let mut sorted = items;
    sorted.sort_by_key(|i| std::cmp::Reverse(i.bytes));

    let mut kept: Vec<crate::domain::plan::PlanItem> = Vec::new();
    let mut deferred: Vec<crate::domain::plan::PlanItem> = Vec::new();
    let mut budget = cap_bytes;

    for item in sorted {
        if item.bytes <= budget {
            budget -= item.bytes;
            kept.push(item);
        } else {
            deferred.push(item);
        }
    }

    (kept, deferred)
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

// ── Standing: parsing autoclean.log into an inspectable outcome history ───────

/// Terminal outcome of one unattended run, as classified from its log lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocleanOutcome {
    /// Ran to completion (`done.` line present) and deleted something.
    Ok,
    /// Ran, plan was empty — nothing to clean.
    NothingToDo,
    /// Deliberately did not proceed (unknown/irreversible reversibility).
    Skipped,
    /// Deliberately refused (single item over the safety cap).
    Refused,
    /// A stage failed (plan build / approve / execute nonzero exit).
    Failed,
}

impl std::fmt::Display for AutocleanOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AutocleanOutcome::Ok => "ok",
            AutocleanOutcome::NothingToDo => "nothing-to-do",
            AutocleanOutcome::Skipped => "skipped",
            AutocleanOutcome::Refused => "refused",
            AutocleanOutcome::Failed => "FAILED",
        };
        write!(f, "{s}")
    }
}

/// One run's record, grouped from all log lines sharing its `[autoclean {ts}]`
/// tag. `detail` is the terminal line that decided the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocleanRunRecord {
    pub started_unix: i64,
    pub outcome: AutocleanOutcome,
    pub detail: String,
    /// Human reclaim figure captured from the `freed:` line, if the run
    /// reached (and reported) a measured deletion.
    pub freed: Option<String>,
}

/// The inspectable history of unattended runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutocleanStanding {
    pub runs_total: usize,
    pub last_run_unix: Option<i64>,
    pub last_outcome: Option<AutocleanOutcome>,
    pub last_detail: String,
    pub last_freed: Option<String>,
    /// Consecutive `Failed` runs counted back from the most recent run —
    /// the "this job is broken and nobody noticed" signal.
    pub consecutive_failures: usize,
    pub runs_last_7d: usize,
}

/// Parses one log timestamp tag (`20260918T041500Z`) to Unix seconds.
///
/// ```
/// use osx_clnr::nouns::autoclean::parse_log_ts;
///
/// assert_eq!(parse_log_ts("19700101T000000Z"), Some(0));
/// // Refusal: not a timestamp.
/// assert_eq!(parse_log_ts("nonsense"), None);
/// ```
pub fn parse_log_ts(tag: &str) -> Option<i64> {
    chrono::NaiveDateTime::parse_from_str(tag, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

/// Severity rank used to pick a run's terminal outcome when several
/// classified lines exist (a failed run has no `done.` line, but a
/// successful one has exactly one terminal marker; rank guards against
/// reordering anyway).
fn outcome_rank(line: &str) -> Option<(u8, AutocleanOutcome)> {
    // Snapshot-thin failures are explicitly non-fatal — the file cleanup
    // already succeeded — so they must not classify a run as Failed.
    if line.contains("FAILED") && !line.contains("non-fatal") {
        return Some((4, AutocleanOutcome::Failed));
    }
    if line.contains("REFUSED:") {
        return Some((3, AutocleanOutcome::Refused));
    }
    if line.contains("SKIPPED:") {
        return Some((2, AutocleanOutcome::Skipped));
    }
    if line.trim_start().starts_with("done.") {
        return Some((1, AutocleanOutcome::Ok));
    }
    if line.contains("nothing to clean") {
        return Some((1, AutocleanOutcome::NothingToDo));
    }
    None
}

/// Extracts the human reclaim figure from a `freed:` log line's text.
///
/// ```
/// use osx_clnr::nouns::autoclean::parse_freed;
///
/// assert_eq!(parse_freed("freed: 3.20 GB"), Some("3.20 GB".to_string()));
/// assert_eq!(parse_freed("freed: nothing"), Some("nothing".to_string()));
/// // Refusal: a line that carries no freed figure.
/// assert_eq!(parse_freed("done. plan=x receipt=y verify_exit=Some(0)"), None);
/// ```
pub fn parse_freed(line: &str) -> Option<String> {
    let rest = line.split("freed:").nth(1)?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// Folds `autoclean.log` lines (each record line shaped
/// `[autoclean {ts}] message`, embedded stderr/stdout continuation lines
/// without the prefix ignored) into the run history. Lines must be in
/// chronological order — the log is append-only, so they are.
///
/// ```
/// use osx_clnr::nouns::autoclean::{summarize_log, AutocleanOutcome};
///
/// let lines = [
///     "[autoclean 20260917T041500Z] done. plan=/l/1-plan.json receipt=/l/1-r.jsonocel verify_exit=Some(0)",
///     "[autoclean 20260917T041500Z] freed: 3.20 GB",
///     "[autoclean 20260918T041500Z] plan build FAILED: exit code 1",
///     "oclnr: error: something (unprefixed continuation line, ignored)",
/// ];
/// let now = osx_clnr::nouns::autoclean::parse_log_ts("20260918T041500Z").unwrap() + 86_400;
///
/// let s = summarize_log(&lines, now);
///
/// // Positive: both runs found, freed figure captured, failure streak = 1.
/// assert_eq!(s.runs_total, 2);
/// assert_eq!(s.last_outcome, Some(AutocleanOutcome::Failed));
/// assert_eq!(s.consecutive_failures, 1);
/// // The most recent *reported* reclaim survives even though the newest run
/// // failed before deleting anything.
/// assert_eq!(s.last_freed, Some("3.20 GB".to_string()));
/// assert_eq!(s.runs_last_7d, 2);
///
/// // Refusal: no history at all.
/// assert_eq!(summarize_log(&[], now).runs_total, 0);
/// ```
pub fn summarize_log(lines: &[&str], now_unix: i64) -> AutocleanStanding {
    let mut records: Vec<AutocleanRunRecord> = Vec::new();

    for line in lines {
        let Some(rest) = line.strip_prefix("[autoclean ") else { continue };
        let Some((tag, message)) = rest.split_once("] ") else { continue };
        let Some(started_unix) = parse_log_ts(tag) else { continue };

        let freed = parse_freed(message);
        let classified = outcome_rank(message);

        // Group by tag: the current record if the tag matches, else a new one.
        if records.last().map(|r| r.started_unix) == Some(started_unix) && freed.is_some() {
            records.last_mut().unwrap().freed = freed;
            continue;
        }
        match classified {
            Some((rank, outcome)) => {
                let replace_last = records.last().map(|r| r.started_unix) == Some(started_unix)
                    && outcome_rank(&records.last().unwrap().detail).map(|(r, _)| r) < Some(rank);
                if replace_last {
                    let r = records.last_mut().unwrap();
                    r.outcome = outcome;
                    r.detail = message.to_string();
                } else if !replace_last
                    && records.last().map(|r| r.started_unix) != Some(started_unix)
                {
                    records.push(AutocleanRunRecord {
                        started_unix,
                        outcome,
                        detail: message.to_string(),
                        freed,
                    });
                }
            }
            None => {
                // Non-terminal line (progress chatter, freed figure for a
                // fresh tag) — only open a record for it if this tag is new,
                // so a run whose terminal line is somehow missing still
                // appears in the count.
                if records.last().map(|r| r.started_unix) != Some(started_unix) {
                    records.push(AutocleanRunRecord {
                        started_unix,
                        outcome: AutocleanOutcome::Ok,
                        detail: message.to_string(),
                        freed,
                    });
                } else if freed.is_some() {
                    records.last_mut().unwrap().freed = freed;
                }
            }
        }
    }

    let mut standing = AutocleanStanding::default();
    standing.runs_total = records.len();
    let week_ago = now_unix.saturating_sub(7 * 86_400);
    standing.runs_last_7d = records.iter().filter(|r| r.started_unix >= week_ago).count();

    if let Some(last) = records.last() {
        standing.last_run_unix = Some(last.started_unix);
        standing.last_outcome = Some(last.outcome);
        standing.last_detail = last.detail.clone();
    }
    // Most recent *reported* reclaim, not necessarily the newest run's: a
    // failed run deleted nothing, and hiding the last known figure behind a
    // `None` would make the standing strictly less informative.
    standing.last_freed = records.iter().rev().find_map(|r| r.freed.clone());
    standing.consecutive_failures =
        records.iter().rev().take_while(|r| r.outcome == AutocleanOutcome::Failed).count();

    standing
}

/// Renders and prints `autoclean status` (read-only).
fn print_status() -> anyhow::Result<()> {
    let log = log_dir()?.join("autoclean.log");
    println!("Autoclean log: {}", log.display());

    let lines: Vec<String> = match std::fs::read_to_string(&log) {
        Ok(content) => content.lines().map(|l| l.to_string()).collect(),
        Err(_) => {
            println!("No runs recorded yet (no log file).");
            return Ok(());
        }
    };
    let borrowed: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let now = chrono::Utc::now().timestamp();
    let s = summarize_log(&borrowed, now);

    if s.runs_total == 0 {
        println!("No runs recorded yet (log exists but is empty or unparsable).");
        return Ok(());
    }

    let last_when = s
        .last_run_unix
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());
    println!("Runs recorded:   {} ({} in the last 7 days)", s.runs_total, s.runs_last_7d);
    println!(
        "Last run:        {} — {}",
        last_when,
        s.last_outcome.map(|o| o.to_string()).unwrap_or_else(|| "?".to_string())
    );
    if let Some(freed) = &s.last_freed {
        println!("Last reclaim:    {freed}");
    }
    println!("  detail: {}", s.last_detail);
    match read_last_trigger_unix() {
        Some(t) => println!(
            "Last pressure-trigger: {}",
            chrono::DateTime::from_timestamp(t, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| t.to_string())
        ),
        None => println!("Last pressure-trigger: never"),
    }
    if s.consecutive_failures >= 2 {
        println!(
            "⚠ {} consecutive failures — the unattended pipeline is broken; \
             run the stages manually to see the error",
            s.consecutive_failures
        );
    }
    Ok(())
}

/// Sends a macOS notification about a run outcome. Non-fatal by contract: a
/// notification failure must never turn an already-logged outcome into a
/// run failure — the log line is the durable record, the banner is a
/// convenience.
fn notify_nonfatal(title: &str, body: &str) {
    if let Err(e) = crate::integration::notify::send_notification(title, body) {
        eprintln!("[autoclean] notification failed (non-fatal): {e}");
    }
}

const NOTIFY_TITLE: &str = "osx-clnr autoclean";

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
    // launchd runs LaunchAgents with cwd=/ (unwritable, and no scan cache
    // could ever live at the filesystem root). Anchor every subprocess to
    // the home directory so workspace-relative state — the `.oclnr-cache`
    // scan cache `plan build` maintains — lands somewhere usable, exactly
    // as it does for interactive runs.
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let home_ref: &std::path::Path = home.as_path();

    // Orchestration evidence: the run-level OCEL log relating this run to
    // the plan/receipt/snapshot artifacts it produced (or refused to
    // produce). Written at every terminal outcome — a refused or failed run
    // is exactly when the evidence matters most. Non-fatal to write: the
    // per-stage artifacts are the primary receipts; this is the run-level
    // index over them.
    let mut facts = AutocleanRunFacts {
        run_id: ts.clone(),
        trigger: std::env::var("OCLNR_AUTOCLEAN_TRIGGER").unwrap_or_else(|_| "scheduled".into()),
        max_reclaim_gb,
        ..Default::default()
    };
    let emit_run_ocel = |facts: &AutocleanRunFacts| {
        let log = build_autoclean_run_ocel(facts);
        let path = dir.join(format!("{}-autoclean-run.jsonocel", facts.run_id));
        match serde_json::to_string_pretty(&log)
            .map_err(anyhow::Error::from)
            .and_then(|s| std::fs::write(&path, s).map_err(anyhow::Error::from))
        {
            Ok(()) => println!("[autoclean {ts}] run OCEL written: {}", path.display()),
            Err(e) => eprintln!("[autoclean {ts}] warning: could not write run OCEL: {e}"),
        }
    };

    println!("[autoclean {ts}] building plan (ignore_recent_hours={ignore_recent_hours})...");
    let build = Command::new(&exe)
        .current_dir(home_ref)
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
        facts.outcome = "failed".into();
        facts.stage_failed = "plan_build".into();
        emit_run_ocel(&facts);
        let msg = format!(
            "[autoclean {ts}] plan build FAILED: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        notify_nonfatal(NOTIFY_TITLE, "unattended run FAILED at plan build — see autoclean.log");
        anyhow::bail!("plan build failed");
    }

    let plan_content = std::fs::read_to_string(&plan_file)?;
    let mut plan: crate::domain::plan::DeletionPlan = serde_json::from_str(&plan_content)?;
    let total_bytes: u64 = plan.items.iter().map(|i| i.bytes).sum();
    facts.plan_path = Some(plan_file.display().to_string());

    if plan.items.is_empty() {
        facts.outcome = "nothing_to_do".into();
        emit_run_ocel(&facts);
        let msg = format!("[autoclean {ts}] nothing to clean this run.");
        println!("{msg}");
        append_log(&msg)?;
        return Ok(());
    }

    if exceeds_cap(total_bytes, max_reclaim_gb) {
        // Cap as budget, not veto: a machine that genuinely accumulated more
        // rebuildable artifacts than one run's cap (the machine this was
        // diagnosed on: 106 GB plan vs 50 GB default cap) used to get NO
        // cleanup at all — the whole run was refused. Instead, execute the
        // largest-first subset that fits the cap and defer the rest to the
        // next scheduled run. If nothing fits (a single item larger than the
        // whole cap — the runaway-detection backstop), keep the original
        // refuse-and-log behavior.
        let (kept, deferred) =
            trim_plan_items_to_cap(std::mem::take(&mut plan.items), max_reclaim_gb);
        if kept.is_empty() {
            facts.outcome = "refused".into();
            emit_run_ocel(&facts);
            let msg = format!(
                "[autoclean {ts}] REFUSED: single plan item exceeds the {} GB safety cap — not \
                 approving. Review manually: oclnr plan inspect --plan {}",
                max_reclaim_gb,
                plan_file.display()
            );
            eprintln!("{msg}");
            append_log(&msg)?;
            notify_nonfatal(
                NOTIFY_TITLE,
                "run REFUSED: single plan item exceeds the safety cap — review needed",
            );
            return Ok(());
        }
        let kept_bytes: u64 = kept.iter().map(|i| i.bytes).sum();
        let deferred_bytes: u64 = deferred.iter().map(|i| i.bytes).sum();
        plan.items = kept;
        facts.items_approved = plan.items.len() as i64;
        facts.deferred_items = deferred.len() as i64;
        facts.planned_bytes = kept_bytes as i64;
        std::fs::write(&plan_file, serde_json::to_string_pretty(&plan)?)?;
        let msg = format!(
            "[autoclean {ts}] plan claims {} which exceeds the {} GB cap — executing largest-first \
             {} ({} items) this run, deferring {} items ({}) to future runs",
            crate::integration::progress::human_bytes(total_bytes),
            max_reclaim_gb,
            crate::integration::progress::human_bytes(kept_bytes),
            plan.items.len(),
            deferred.len(),
            crate::integration::progress::human_bytes(deferred_bytes),
        );
        println!("{msg}");
        append_log(&msg)?;
    }
    // Under-cap path: the facts weren't touched by the trim branch, so
    // record the whole approved plan.
    if facts.planned_bytes == 0 {
        facts.items_approved = plan.items.len() as i64;
        facts.planned_bytes = total_bytes as i64;
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
        facts.outcome = "skipped".into();
        emit_run_ocel(&facts);
        let msg = format!(
            "[autoclean {ts}] SKIPPED: plan contains {non_reversible_count} unknown/irreversible-\
             reversibility item(s) — autoclean never overrides that unattended. Review manually: \
             oclnr plan inspect --plan {}",
            plan_file.display()
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        notify_nonfatal(
            NOTIFY_TITLE,
            "run SKIPPED: unknown-reversibility items need human review — see autoclean.log",
        );
        return Ok(());
    }

    println!(
        "[autoclean {ts}] approving plan ({} items, {})...",
        plan.items.len(),
        crate::integration::progress::human_bytes(total_bytes)
    );
    let approve = Command::new(&exe)
        .current_dir(home_ref)
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
        facts.outcome = "failed".into();
        facts.stage_failed = "plan_approve".into();
        emit_run_ocel(&facts);
        let msg = format!(
            "[autoclean {ts}] plan approve FAILED: {}",
            String::from_utf8_lossy(&approve.stderr)
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        notify_nonfatal(NOTIFY_TITLE, "unattended run FAILED at plan approve — see autoclean.log");
        anyhow::bail!("plan approve failed");
    }

    println!("[autoclean {ts}] executing...");
    let execute = Command::new(&exe)
        .current_dir(home_ref)
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
        facts.outcome = "failed".into();
        facts.stage_failed = "delete_execute".into();
        emit_run_ocel(&facts);
        let msg = format!(
            "[autoclean {ts}] delete execute FAILED (exit {:?}): {}\n{}",
            execute.status.code(),
            String::from_utf8_lossy(&execute.stderr),
            exec_stdout
        );
        eprintln!("{msg}");
        append_log(&msg)?;
        notify_nonfatal(
            NOTIFY_TITLE,
            "unattended run FAILED at delete execute — see autoclean.log",
        );
        anyhow::bail!("delete execute failed");
    }

    // Record the measured reclaim in the log so `autoclean status` can show
    // it without parsing the receipt: `delete execute` prints a
    // `Freed:  X (measured)` line on success.
    if let Some(freed_line) = exec_stdout.lines().find(|l| l.contains("Freed:")) {
        let figure = freed_line.split("Freed:").nth(1).unwrap_or("").trim();
        let figure = figure.trim_end_matches("(measured)").trim();
        if !figure.is_empty() {
            append_log(&format!("[autoclean {ts}] freed: {figure}"))?;
        }
    }

    let verify = Command::new(&exe)
        .current_dir(home_ref)
        .args(["receipt", "verify", "--receipt"])
        .arg(&receipt_file)
        .args(["--plan"])
        .arg(&plan_file)
        .output()?;

    facts.receipt_path = Some(receipt_file.display().to_string());
    facts.outcome = "completed".into();
    let msg = format!(
        "[autoclean {ts}] done. plan={} receipt={} verify_exit={:?}",
        plan_file.display(),
        receipt_file.display(),
        verify.status.code()
    );
    println!("{msg}");
    append_log(&msg)?;
    notify_nonfatal(NOTIFY_TITLE, "unattended cleanup completed — details in autoclean.log");

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
        .current_dir(home_ref)
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

    // Terminal evidence written last so the snapshot receipt — if the thin
    // step produced one — is part of the run's OCEL graph.
    if snapshot_receipt.exists() {
        facts.snapshot_receipt_path = Some(snapshot_receipt.display().to_string());
    }
    emit_run_ocel(&facts);
    Ok(())
}

// ── Pressure-trigger coordination (used by `monitor --trigger-autoclean`) ─────
//
// The monitor may fire the same lawful autoclean pipeline on demand when the
// disk is under pressure, bounded by a cooldown so a slow cleanup can never
// be stacked on itself. The state file records when the last trigger fired;
// it is written *before* the run starts, so a crashed or hung run still
// holds the cooldown open instead of letting every monitor fire re-trigger.

/// State file recording the last pressure-trigger time (Unix seconds):
/// `~/.oclnr/last-autoclean-trigger`.
fn trigger_state_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(home.join(".oclnr").join("last-autoclean-trigger"))
}

/// Unix seconds of the last monitor pressure-trigger, if any.
pub fn read_last_trigger_unix() -> Option<i64> {
    let path = trigger_state_path().ok()?;
    std::fs::read_to_string(path).ok()?.trim().parse::<i64>().ok()
}

/// Records `now_unix` as the last pressure-trigger time.
pub fn write_trigger_stamp(now_unix: i64) -> anyhow::Result<()> {
    let path = trigger_state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{now_unix}\n"))?;
    Ok(())
}
