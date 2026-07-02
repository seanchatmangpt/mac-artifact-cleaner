//! Progress indication spinner and progress bar.

use crate::domain::audit::Stats;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct ProgressReporter {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    pb: Option<ProgressBar>,
}

impl ProgressReporter {
    pub fn start(label: String, stats: Arc<Stats>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let use_spinner = std::io::stderr().is_terminal();

        let pb = if use_spinner {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner:.green} {msg}")
                    .unwrap()
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            pb.enable_steady_tick(Duration::from_millis(120));
            Some(pb)
        } else {
            None
        };

        let thread_stop = stop.clone();
        let thread_pb = pb.clone();

        let join = thread::spawn(move || {
            let started = Instant::now();
            let mut last_files = 0usize;
            let mut last_bytes = 0u64;
            let mut last_tick = Instant::now();

            while !thread_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(750));

                let now = Instant::now();
                let elapsed_total = started.elapsed().as_secs_f64();
                let elapsed_tick = now.duration_since(last_tick).as_secs_f64().max(0.001);

                let files = stats.files_seen.load(Ordering::Relaxed);
                let dirs = stats.dirs_seen.load(Ordering::Relaxed);
                let bytes = stats.bytes_seen.load(Ordering::Relaxed);
                let skipped = stats.pruned_dirs.load(Ordering::Relaxed);
                let errors = stats.errors.load(Ordering::Relaxed);
                let phase = stats.phase.lock().unwrap().clone();

                let _file_rate =
                    ((files.saturating_sub(last_files)) as f64 / elapsed_tick) as usize;
                let byte_rate = ((bytes.saturating_sub(last_bytes)) as f64 / elapsed_tick) as u64;

                last_files = files;
                last_bytes = bytes;
                last_tick = now;

                let msg = format!(
                    "{} | phase={} | files={} dirs={} seen={} rate={}/s skipped={} errors={} elapsed={}s",
                    label,
                    phase,
                    files,
                    dirs,
                    human_bytes(bytes),
                    human_bytes(byte_rate),
                    skipped,
                    errors,
                    elapsed_total as u64,
                );

                if let Some(pb) = &thread_pb {
                    pb.set_message(msg);
                } else {
                    eprintln!("{}", msg);
                }
            }

            if let Some(pb) = &thread_pb {
                pb.finish_and_clear();
            }
        });

        Self {
            stop,
            join: Some(join),
            pb,
        }
    }

    pub fn finish(mut self, message: &str) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if let Some(pb) = &self.pb {
            pb.finish_with_message(message.to_string());
        } else {
            eprintln!("{}", message);
        }
    }
}

/// Re-export of the single canonical size formatter (see
/// [`crate::domain::time::human_bytes`]). Kept at this path so existing
/// `integration::progress::human_bytes` call sites resolve unchanged, but there
/// is now exactly one definition — the two paths cannot diverge by construction.
pub use crate::domain::time::human_bytes;

/// Returns the byte multiplier for a case-insensitive size unit suffix, or
/// `None` if the unit is not recognized. Units are interpreted as base-1024
/// (binary) regardless of whether the "i" (`KiB`/`MiB`/...) form is used,
/// matching the values actually emitted by the tools this parser targets
/// (`docker system df`, `git count-objects -vH`, `brew cleanup -n`).
fn size_unit_multiplier(unit: &str) -> Option<f64> {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    match unit.trim_end_matches(',').to_uppercase().as_str() {
        "B" | "BYTE" | "BYTES" => Some(1.0),
        "KB" | "KIB" => Some(KIB),
        "MB" | "MIB" => Some(MIB),
        "GB" | "GIB" => Some(GIB),
        "TB" | "TIB" => Some(TIB),
        _ => None,
    }
}

/// Parses a human-readable byte-size string (e.g. `"2.30 GB"`, `"1KB"`,
/// `"512 MiB"`, `"2.1GB (40%)"`) into a byte count.
///
/// Handles a numeric value that is either directly attached to its unit
/// (`"2.1GB"`) or separated from it by whitespace (`"2.30 GB"`), and tolerates
/// being handed a larger string with surrounding text (e.g. `"This operation
/// would free 2.30 GB of disk space."` or `"2.1GB (40%)"`) by scanning for the
/// first `<number><unit>` pair. All units are treated as base-1024. Returns 0
/// if no numeric value with a recognized unit — or no numeric value at all —
/// is found.
///
/// This is the single shared implementation for what were previously three
/// near-identical parsers (`integration::brew::parse_free_bytes`,
/// `integration::git_health::parse_human_size`,
/// `integration::docker::parse_size_str`).
///
/// # Examples
///
/// ```
/// use osx_clnr::integration::progress::parse_human_size;
///
/// assert_eq!(parse_human_size("0B"), 0);
/// assert_eq!(parse_human_size("1KB"), 1_024);
/// assert_eq!(parse_human_size("1MB"), 1_048_576);
/// assert_eq!(parse_human_size("12.00 KiB"), 12_288);
/// assert_eq!(parse_human_size("2.1GB (40%)"), parse_human_size("2.1GB"));
/// ```
pub fn parse_human_size(s: &str) -> u64 {
    let tokens: Vec<&str> = s.split_whitespace().collect();

    for (i, token) in tokens.iter().enumerate() {
        // Split the token itself at the number/suffix boundary, so a token
        // like "2.1GB" becomes ("2.1", "GB").
        let split_pos = token
            .rfind(|c: char| c.is_ascii_digit() || c == '.')
            .map(|p| p + 1)
            .unwrap_or(0);
        let (num_part, suffix) = token.split_at(split_pos);

        let Ok(value) = num_part.parse::<f64>() else {
            continue;
        };

        if !suffix.is_empty() {
            let cleaned = suffix.trim_start_matches('(').trim_end_matches([')', ',']);
            if let Some(multiplier) = size_unit_multiplier(cleaned) {
                return (value * multiplier) as u64;
            }
            continue;
        }

        // No unit attached to the number itself — check the next token.
        if let Some(next) = tokens.get(i + 1) {
            let cleaned = next.trim_start_matches('(').trim_end_matches([')', ',']);
            if let Some(multiplier) = size_unit_multiplier(cleaned) {
                return (value * multiplier) as u64;
            }
        }
    }

    0
}
