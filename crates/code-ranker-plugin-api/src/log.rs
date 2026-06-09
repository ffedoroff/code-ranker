//! Shared stderr progress/timing log.
//!
//! This lives in the foundation crate so every component — CLI stages and the
//! sub-commands plugins shell out to (`git`, `cargo metadata`, `rustc`) — emits
//! one consistent line format. All output goes to **stderr** (machine output and
//! artifacts go to stdout/files), prefixed with a local `HH:MM:SS.mmm` stamp.
//! Durations are printed to **millisecond precision** (`0.231s`).

use chrono::Local;
use std::time::{Duration, Instant};

/// Local wall-clock stamp, `HH:MM:SS.mmm`.
pub fn stamp() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

/// Format a duration as seconds with millisecond precision, e.g. `0.231s`,
/// `29.900s`. The single authority for how timings render across the tool.
pub fn secs(dur: Duration) -> String {
    format!("{:.3}s", dur.as_secs_f64())
}

/// Emit one stamped line to stderr: `[HH:MM:SS.mmm] <msg>`.
pub fn line(msg: &str) {
    eprintln!("[{}] {}", stamp(), msg);
}

/// Log a completed internal sub-command (an external tool code-ranker shelled out
/// to) with its duration: `[HH:MM:SS.mmm] ↳ <label> — 0.231s`. The `↳` marks it
/// as a nested step under the current stage.
pub fn subcmd(label: &str, dur: Duration) {
    line(&format!("↳ {label} — {}", secs(dur)));
}

/// Time `f`, log it as a sub-command (see [`subcmd`]), and return its value.
/// Wrap every `git` / `cargo` / `rustc` invocation in this so the cost of each
/// external call is visible — these dominate the wall clock on a cold cache.
pub fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    subcmd(label, start.elapsed());
    out
}
