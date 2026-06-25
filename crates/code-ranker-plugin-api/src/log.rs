//! Shared stderr progress/timing log.
//!
//! This lives in the foundation crate so every component — CLI stages and the
//! sub-commands plugins shell out to (`git`, `cargo metadata`, `rustc`) — emits
//! one consistent line format. All output goes to **stderr** (machine output and
//! artifacts go to stdout/files), prefixed with a local `HH:MM:SS.mmm` stamp.
//! Durations are printed to **millisecond precision** (`0.231s`).
//!
//! How loud that stream is is governed by a single process-wide [verbosity
//! level](set_level), set once at startup from `--output.mode`. The level lives
//! here (not in the CLI) because the lines it gates are emitted from both the CLI
//! stages and the plugins — they share one switch. Emitters come in three tiers:
//! [`line`] always prints (errors); [`summary`] prints at `SUMMARY`+ (the closing
//! `✓` line, warnings, written-artifact paths); [`verbose`]/[`subcmd`] print only
//! at `VERBOSE` (the `▶`/`config:` startup lines and every external-tool timing).

use chrono::Local;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Silence everything but errors.
pub const QUIET: u8 = 0;
/// Default: errors, warnings, written-artifact paths, and the closing `✓` line.
pub const SUMMARY: u8 = 1;
/// Everything, including the `▶`/`config:` startup lines and per-tool `↳` timings.
pub const VERBOSE: u8 = 2;

// Defaults to SUMMARY so a process that never calls `set_level` (e.g. a test or a
// plugin exercised in isolation) still behaves like the documented default.
static LEVEL: AtomicU8 = AtomicU8::new(SUMMARY);

/// Set the process-wide verbosity. Called once from `main` after arg parsing,
/// before the first line is emitted. Takes one of [`QUIET`]/[`SUMMARY`]/[`VERBOSE`].
pub fn set_level(level: u8) {
    LEVEL.store(level, Ordering::Relaxed);
}

/// The current verbosity level.
pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed)
}

/// Local wall-clock stamp, `HH:MM:SS.mmm`.
pub fn stamp() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

/// Format a duration as seconds with millisecond precision, e.g. `0.231s`,
/// `29.900s`. The single authority for how timings render across the tool.
pub fn secs(dur: Duration) -> String {
    format!("{:.3}s", dur.as_secs_f64())
}

/// Emit one stamped line to stderr unconditionally: `[HH:MM:SS.mmm] <msg>`.
/// Reserved for messages that must show at every level (errors). Tier-gated
/// callers use [`summary`] / [`verbose`] instead.
pub fn line(msg: &str) {
    eprintln!("[{}] {}", stamp(), msg);
}

/// Emit a line only at [`SUMMARY`] or louder: the closing `✓` line, warnings,
/// and written-artifact paths — the minimal "what happened" trace.
pub fn summary(msg: &str) {
    if level() >= SUMMARY {
        line(msg);
    }
}

/// Emit a line only at [`VERBOSE`]: the `▶`/`config:` startup lines — diagnostic
/// detail that would clutter the default stream.
pub fn verbose(msg: &str) {
    if level() >= VERBOSE {
        line(msg);
    }
}

/// Log a completed internal sub-command (an external tool code-ranker shelled out
/// to) with its duration: `[HH:MM:SS.mmm] ↳ <label> — 0.231s`. The `↳` marks it
/// as a nested step under the current stage. Shown only at [`VERBOSE`] — the work
/// still runs at every level (see [`timed`]); only the line is gated.
pub fn subcmd(label: &str, dur: Duration) {
    if level() >= VERBOSE {
        line(&format!("↳ {label} — {}", secs(dur)));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The level switch round-trips, and `secs` renders millisecond precision.
    #[test]
    fn level_set_and_get_round_trips() {
        let saved = level();
        set_level(QUIET);
        assert_eq!(level(), QUIET);
        set_level(VERBOSE);
        assert_eq!(level(), VERBOSE);
        set_level(saved);
        assert_eq!(secs(Duration::from_millis(231)), "0.231s");
    }

    /// `timed` runs `f` and returns its value at EVERY level — the work is never
    /// gated, only the line. Exercising it at VERBOSE drives the gated emission in
    /// `subcmd` (and, via `verbose`, the matching `line` call) without asserting on
    /// the stderr text. The level is saved and restored so the process-wide switch
    /// is left as found for sibling tests.
    #[test]
    fn timed_runs_and_logs_at_verbose() {
        let saved = level();
        set_level(VERBOSE);
        let out = timed("unit-test sub-command", || {
            verbose("a verbose-only diagnostic line");
            7
        });
        set_level(saved);
        assert_eq!(out, 7, "timed returns the closure's value");
    }
}
