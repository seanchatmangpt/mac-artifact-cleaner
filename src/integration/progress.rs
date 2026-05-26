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

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{:.2} {}", size, UNITS[unit])
}
