//! `dedupe` noun: read-only measurement of bytes reclaimable via APFS clones.
//!
//! Only `scan` exists. Clone replacement (`execute`) is
//! `UNSUPPORTED(dedupe-execute)`; see [`crate::domain::dedupe::EXECUTE_STATUS`].

use std::path::PathBuf;

use clap::Subcommand;

use crate::{
    domain::dedupe::{DedupeReport, DEFAULT_MIN_SIZE},
    integration::progress::human_bytes,
};

#[derive(Subcommand, Debug)]
pub enum DedupeAction {
    /// Measure duplicate regular files and bytes reclaimable by APFS cloning (read-only)
    Scan {
        /// Root directory to scan (repeatable)
        #[arg(long = "root", required = true)]
        roots: Vec<PathBuf>,
        /// Minimum file size in bytes to consider
        #[arg(long, default_value_t = DEFAULT_MIN_SIZE)]
        min_size: u64,
        /// Write the full JSON report to this path
        #[arg(long)]
        output: Option<PathBuf>,
        /// Number of top groups to print
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
}

pub fn handle(action: DedupeAction) -> anyhow::Result<()> {
    match action {
        DedupeAction::Scan { roots, min_size, output, top } => {
            handle_scan(&roots, min_size, output, top)
        }
    }
}

fn handle_scan(
    roots: &[PathBuf],
    min_size: u64,
    output: Option<PathBuf>,
    top: usize,
) -> anyhow::Result<()> {
    let report = crate::integration::dedupe::scan_duplicates(roots, min_size)?;
    print_summary(&report, top);
    if let Some(path) = output {
        let json = serde_json::to_string_pretty(&report)?;
        let (outcome, _) =
            crate::integration::fs::write_output_file(&path, &json, false, "dedupe report")?;
        if outcome.is_written() {
            println!("Report written: {}", path.display());
        }
    }
    Ok(())
}

fn print_summary(r: &DedupeReport, top: usize) {
    println!("Dedupe scan (read-only)");
    println!("{:-<72}", "");
    for root in &r.roots {
        println!("Root:                 {}", root.display());
    }
    println!("Min size:             {} bytes", r.min_size);
    println!("Files scanned:        {}", r.files_scanned);
    println!("Candidates hashed:    {} ({} hash errors)", r.candidates_hashed, r.hash_errors);
    println!("Duplicate groups:     {}", r.group_count);
    println!("Files in groups:      {}", r.duplicate_files);
    println!(
        "Logical duplicate:    {} ({} bytes)",
        human_bytes(r.logical_duplicate_bytes),
        r.logical_duplicate_bytes
    );
    println!(
        "Reclaimable (clone):  {} ({} bytes){}",
        human_bytes(r.reclaimable_bytes),
        r.reclaimable_bytes,
        if r.estimate { " [estimate: private size unavailable for some files]" } else { "" }
    );
    println!("Execute:              {}", r.execute_status);
    if r.groups.is_empty() {
        return;
    }
    println!("{:-<72}", "");
    println!("Top {} groups by reclaimable bytes:", top.min(r.groups.len()));
    for (i, g) in r.groups.iter().take(top).enumerate() {
        println!(
            "{:>3}. {:>10} reclaim  {} x {}  keeper: {}",
            i + 1,
            human_bytes(g.reclaimable_bytes),
            g.members.len(),
            human_bytes(g.size),
            g.keeper.display()
        );
    }
}
