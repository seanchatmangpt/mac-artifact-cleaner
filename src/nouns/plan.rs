//! Plan CLI noun implementation.

use std::{os::unix::fs::MetadataExt, path::PathBuf, sync::Arc};

use clap::Subcommand;
use dashmap::DashMap;
use rayon::prelude::*;

use crate::{
    domain::{
        artifact::{ArgsSnapshot, Candidate},
        audit::Stats,
        dcm::{classify_reversibility, Reversibility},
        plan::{DeletionPlan, PlanItem, PlanItemKind},
        tool_roots::build_tool_root_defs,
    },
    integration::{
        fs::{physical_dir_size, scan_root, write_output_file, WriteOutcome},
        progress::ProgressReporter,
        scan_cache::ScanCache,
    },
};

#[derive(Subcommand, Debug)]
pub enum PlanAction {
    /// Build a new dry-run deletion plan
    Build {
        /// Roots to scan (defaults to home directory)
        #[arg(long)]
        root: Vec<PathBuf>,
        /// Include dependencies (e.g. node_modules)
        #[arg(long)]
        deps: bool,
        /// Include aggressive build files
        #[arg(long)]
        aggressive: bool,
        /// Ignore projects modified within the specified number of hours
        #[arg(long, default_value_t = 168)]
        ignore_recent_hours: u64,
        /// Plan file output destination
        #[arg(short, long)]
        output: PathBuf,
        /// Also nominate large user-level caches (~/Library/Caches, Xcode
        /// DerivedData, ~/.cache, cargo registry, npm/go caches) — off by default
        #[arg(long)]
        include_global_caches: bool,
        /// Verbose trace output
        #[arg(long)]
        verbose: bool,
        /// Redact local usernames and credential-shaped values from the
        /// written plan file before it hits disk.
        #[arg(long)]
        redact: bool,
    },
    /// Inspect a built deletion plan
    Inspect {
        /// Path to the deletion plan
        #[arg(short, long)]
        plan: PathBuf,
    },
    /// Sign a plan for deletion (CLI-only path — mirrors the MCP `plan_approve`
    /// tool's HMAC-signing logic exactly, so an unattended script/launchd job
    /// can complete the full audit->plan->approve->delete pipeline without an
    /// MCP/Claude session in the loop). `delete execute` refuses any plan
    /// lacking a signature this command (or the MCP tool) produced.
    Approve {
        /// Path to the deletion plan to sign
        #[arg(short, long)]
        plan: PathBuf,
        /// Recorded approver identity (free text, goes into the receipt)
        #[arg(long, default_value = "oclnr-cli")]
        approver: String,
        /// Recorded reason for this approval (free text, goes into the receipt)
        #[arg(long)]
        reason: String,
        /// Required to approve a plan containing any item whose reversibility
        /// classification is Unknown or Irreversible (see `plan inspect`) —
        /// forces the caller to have actually looked before signing.
        #[arg(long)]
        acknowledge_unknown_reversibility: bool,
        /// Required: this command signs the plan for real deletion.
        #[arg(long)]
        yes: bool,
    },
}

use std::sync::atomic::Ordering;

use crate::integration::progress::human_bytes;

pub fn handle(action: PlanAction) -> anyhow::Result<()> {
    match action {
        PlanAction::Build {
            root,
            deps,
            aggressive,
            ignore_recent_hours,
            output,
            include_global_caches,
            verbose,
            redact,
        } => {
            let roots = if root.is_empty() { crate::nouns::default_scan_roots()? } else { root };

            let args = ArgsSnapshot {
                deps,
                aggressive,
                verbose,
                tool_roots: false,
                ignore_recent_hours,
                all_filesystems: false,
            };

            let candidates: Arc<DashMap<PathBuf, Candidate>> = Arc::new(DashMap::new());
            let stats = Arc::new(Stats::default());
            *stats.phase.lock().unwrap() = "scanning files".to_string();

            let reporter = ProgressReporter::start("Scanning for plan".to_string(), stats.clone());

            let tool_defs = build_tool_root_defs();
            let tool_accs = Arc::new(DashMap::new());

            // Share the workspace-relative scan cache `audit run` maintains
            // (per-directory mtime + child-listing early cutoff): a repeat
            // `plan build` on a mostly-unchanged home folds cached subtree
            // totals instead of re-walking every byte. A cache-open failure
            // must never fail the build — degrade to a full uncached walk.
            // Cached candidates are only *replayed* here; every plan item is
            // still sized live below, so a stale byte count can never reach
            // the plan.
            let scan_cache = match ScanCache::open(
                std::path::Path::new("."),
                &crate::domain::artifact::scan_cache_fingerprint(&args),
            ) {
                Ok(cache) => Some(Arc::new(cache)),
                Err(e) => {
                    eprintln!("warning: could not open scan cache, scanning without it: {e}");
                    None
                }
            };

            for r in &roots {
                scan_root(
                    r,
                    &args,
                    candidates.clone(),
                    stats.clone(),
                    &tool_defs,
                    tool_accs.clone(),
                    scan_cache.clone(),
                )?;
            }

            reporter.finish("✅ Scan complete!");

            let files = stats.files_seen.load(Ordering::Relaxed);
            let dirs = stats.dirs_seen.load(Ordering::Relaxed);
            let bytes = stats.bytes_seen.load(Ordering::Relaxed);
            let projects = stats.projects_seen.load(Ordering::Relaxed);
            let skipped = stats.pruned_dirs.load(Ordering::Relaxed);
            let errors = stats.errors.load(Ordering::Relaxed);

            println!("\n==================================================");
            println!("               SCAN SUMMARY               ");
            println!("==================================================");
            println!("  Files analyzed:      {}", files);
            println!("  Directories walked:  {}", dirs);
            println!("  Total size scanned:  {}", human_bytes(bytes));
            println!("  Projects detected:   {}", projects);
            println!("  Skipped paths:       {} (OS/caches/barriers)", skipped);
            println!("  Errors encountered:  {}", errors);
            println!("==================================================");

            let mut candidate_vec: Vec<Candidate> =
                candidates.iter().map(|e| e.value().clone()).collect();

            // Never let the plan nominate the directory containing the binary
            // that is running this very `plan build` — see
            // `exclude_self_binary_ancestors` doc comment for the concrete
            // incident this guards against (deleting `oclnr-mcp`'s own
            // `target/` mid-session via the MCP server it was serving).
            let exe_path = std::env::current_exe().ok();
            candidate_vec = crate::domain::artifact::exclude_self_binary_ancestors(
                candidate_vec,
                exe_path.as_deref(),
            );

            // Never admit a candidate `plan validate` would reject as an OS/TCC
            // fence (e.g. `~/Library/Containers/<bundle>/Data/tmp`): a plan
            // that fails its own validator on build is unusable, and replayed
            // cache entries from before a fence was added can carry them.
            candidate_vec.retain(|c| {
                !crate::domain::artifact::is_macos_os_dir(&c.path)
                    && !crate::domain::artifact::is_inside_package_store(&c.path)
            });

            candidate_vec.sort();

            // Optionally nominate large user-level caches the per-project scanner
            // never reaches. Guarded by `is_macos_os_dir` and existence; the curated
            // allowlist itself is the safety boundary.
            if include_global_caches {
                if let Some(home) = dirs::home_dir() {
                    // Merge curated global-cache nominations into the scanned set
                    // with ancestor preference (`merge_global_cache_candidates`):
                    // a curated cache dir that contains scanned candidates keeps
                    // the WHOLE cache dir and drops the sub-items (the historical
                    // either-direction overlap veto here let a 0-byte scanned
                    // `…/crate/lz4/build` item suppress the multi-GB
                    // `~/.cargo/registry/src` nomination). Only candidates that
                    // exist and are not macOS OS dirs are nominated; existence is
                    // filtered below alongside the sizing pass.
                    let global: Vec<(std::path::PathBuf, String)> =
                        crate::domain::artifact::global_cache_candidates(&home)
                            .into_iter()
                            .filter(|(path, _)| {
                                path.exists() && !crate::domain::artifact::is_macos_os_dir(path)
                            })
                            .collect();
                    candidate_vec = crate::domain::artifact::merge_global_cache_candidates(
                        candidate_vec,
                        global,
                    );
                }
            }

            // Size each candidate in parallel using physical allocation (blocks × 512),
            // so the plan shows reclaim impact and the receipt can prove bytes freed.
            // `physical_dir_size` (jwalk) is fast enough to run once per candidate here.
            let mut items: Vec<PlanItem> = candidate_vec
                .par_iter()
                .map(|c| {
                    let kind =
                        if c.path.is_file() { PlanItemKind::File } else { PlanItemKind::Dir };
                    let bytes = match kind {
                        PlanItemKind::File => std::fs::symlink_metadata(&c.path)
                            .map(|m| m.blocks() * 512)
                            .unwrap_or(0),
                        PlanItemKind::Dir => physical_dir_size(&c.path),
                        PlanItemKind::GithubRepo
                        | PlanItemKind::GithubBranch
                        | PlanItemKind::GithubRun
                        | PlanItemKind::GithubRelease
                        | PlanItemKind::GithubCache
                        | PlanItemKind::GithubIssue
                        | PlanItemKind::GithubPr
                        | PlanItemKind::GithubReleaseAsset => 0,
                    };

                    let reversibility = classify_reversibility(kind, &c.reason);
                    PlanItem {
                        path: c.path.clone(),
                        kind,
                        reason: c.reason.clone(),
                        bytes,
                        reversibility,
                    }
                })
                .collect();

            // Drop zero-byte file/dir candidates (mostly near-empty AI-tool temp
            // folders like `.claude/tmp`) — they aren't worth reasoning about or
            // deleting individually, and left unfiltered they can dominate the
            // item count and drown out real reclaim opportunities. GitHub-kind
            // items intentionally carry `bytes: 0` here (their size isn't known
            // at plan-build time) and are kept regardless.
            items.retain(|i| {
                i.bytes > 0 || !matches!(i.kind, PlanItemKind::File | PlanItemKind::Dir)
            });

            // Largest reclaim first — both for the printed preview and so deletion
            // tackles the biggest wins before any I/O errors can interrupt it.
            items.sort_by_key(|b| std::cmp::Reverse(b.bytes));

            let plan_total: u64 = items.iter().map(|i| i.bytes).sum();

            let plan = DeletionPlan::new(roots, deps, aggressive, items, vec![]);
            let serialized = serde_json::to_string_pretty(&plan)?;
            let (plan_write, ledger) =
                write_output_file(&output, &serialized, redact, "deletion plan")?;

            match plan_write {
                WriteOutcome::Written => {
                    println!("\n✨ Success: Wrote deletion plan to: {}", output.display());
                }
                WriteOutcome::DumpedToStdout => {
                    println!(
                        "\n⚠️  Deletion plan could NOT be written to disk — dumped to stdout above, no file exists at {}",
                        output.display()
                    );
                }
            }
            if let Some(ledger) = ledger {
                eprintln!("redacted {} item(s) in {}", ledger.entries.len(), output.display());
            }
            println!("   Total deletion items: {}", plan.items.len());
            println!("   Estimated reclaim:    {}", human_bytes(plan_total));
            let top_n = 10.min(plan.items.len());
            if top_n > 0 {
                println!("   Top {} items by size:", top_n);
                for item in plan.items.iter().take(top_n) {
                    println!("     {:>10}  {}", human_bytes(item.bytes), item.path.display());
                }
            }

            let err_lock = stats.error_details.lock().unwrap();
            if !err_lock.is_empty() {
                println!("\n==================================================");
                println!("               TRAVERSAL ERRORS                   ");
                println!("==================================================");
                let display_limit = 10;
                for (path, err_msg) in err_lock.iter().take(display_limit) {
                    println!("  ❌ {}: {}", path.display(), err_msg);
                }
                if err_lock.len() > display_limit {
                    println!("  ... and {} more errors.", err_lock.len() - display_limit);
                }
                println!("==================================================");
            }
        }
        PlanAction::Inspect { plan } => {
            let content = std::fs::read_to_string(&plan)?;
            let plan_data: DeletionPlan = serde_json::from_str(&content)?;
            println!("\n==================================================");
            println!("            DELETION PLAN INSPECTION              ");
            println!("==================================================");
            println!("  Plan File:   {}", plan.display());
            println!("  Version:     {}", plan_data.version);
            println!(
                "  Created:     {}",
                chrono::DateTime::from_timestamp(plan_data.created_unix as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| plan_data.created_unix.to_string())
            );
            println!("  Roots:       {:?}", plan_data.roots);
            println!("  Flags:       deps={}, aggressive={}", plan_data.deps, plan_data.aggressive);
            println!("  Total Items: {}", plan_data.items.len());
            println!("==================================================");
            println!("\nScheduled Deletions:");
            if plan_data.items.is_empty() {
                println!("  (No items scheduled)");
            } else {
                for item in &plan_data.items {
                    println!(
                        "  • [{:?}] {:>10}  ({}) {} - {}",
                        item.kind,
                        human_bytes(item.bytes),
                        item.reversibility.label(),
                        item.path.display(),
                        item.reason
                    );
                }
                let total: u64 = plan_data.items.iter().map(|i| i.bytes).sum();
                println!("  ── Estimated reclaim: {}", human_bytes(total));

                // DCM §5 ("silent pruning is forbidden"): unknown/irreversible
                // items are never dropped from the plan, but they are surfaced
                // explicitly here so a reviewer sees them before approving —
                // per DCM §3, `Unknown` is a fence, not evidence of safety.
                let irreversible = plan_data
                    .items
                    .iter()
                    .filter(|i| i.reversibility == Reversibility::Irreversible)
                    .count();
                let unknown = plan_data
                    .items
                    .iter()
                    .filter(|i| i.reversibility == Reversibility::Unknown)
                    .count();
                if irreversible > 0 || unknown > 0 {
                    println!(
                        "  ⚠ Reversibility review: {} irreversible, {} unknown (unclassifiable) — verify before approving",
                        irreversible, unknown
                    );
                }
            }
            println!("==================================================");
        }
        PlanAction::Approve { plan, approver, reason, acknowledge_unknown_reversibility, yes } => {
            if !yes {
                anyhow::bail!(
                    "Refusing to approve without --yes — this signs the plan for real deletion. \
                     Review it first with `oclnr plan inspect --plan {}`.",
                    plan.display()
                );
            }
            let content = std::fs::read_to_string(&plan)?;
            let mut plan_data: DeletionPlan = serde_json::from_str(&content)?;

            // Same fence as the MCP `plan_approve` tool: a plan with any
            // Unknown/Irreversible item requires an explicit acknowledgement,
            // not just `--yes`, so an unattended caller can't sign past a
            // classification gap it never looked at.
            let non_reversible: Vec<String> = plan_data
                .items
                .iter()
                .filter(|i| {
                    matches!(i.reversibility, Reversibility::Unknown | Reversibility::Irreversible)
                })
                .map(|i| format!("{} [{}]", i.path.display(), i.reversibility.label()))
                .collect();
            if !non_reversible.is_empty() && !acknowledge_unknown_reversibility {
                anyhow::bail!(
                    "plan contains {} item(s) classified unknown or irreversible reversibility; \
                     review with `oclnr plan inspect --plan {}`, then re-run with \
                     --acknowledge-unknown-reversibility to proceed. Items: {}",
                    non_reversible.len(),
                    plan.display(),
                    non_reversible.join(", ")
                );
            }

            let secret = crate::integration::config::approval_secret()?;
            plan_data.approval = Some(plan_data.sign_approval(&secret, &approver, &reason));
            let signed = serde_json::to_string_pretty(&plan_data)?;
            std::fs::write(&plan, signed)?;
            println!(
                "✅ Plan approved: {} (approver: {}, reason: {})",
                plan.display(),
                approver,
                reason
            );
        }
    }
    Ok(())
}
