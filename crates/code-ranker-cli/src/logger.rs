use code_ranker_plugin_api::log;
use std::time::Instant;

/// Re-export the shared timed sub-command helper so call sites in this crate
/// (git / cargo / rustc shell-outs) log through the one common formatter.
pub use code_ranker_plugin_api::log::timed;

pub fn info(msg: &str) {
    log::line(msg);
}

pub struct Timer {
    label: String,
    start: Instant,
}

impl Timer {
    pub fn start(label: &str) -> Self {
        Self {
            label: label.to_string(),
            start: Instant::now(),
        }
    }

    pub fn finish_with(self, extra: &str) -> u64 {
        let elapsed = self.start.elapsed();
        log::line(&format!(
            "✓ {} — {}{}",
            self.label,
            log::secs(elapsed),
            if extra.is_empty() {
                String::new()
            } else {
                format!(" ({})", extra)
            }
        ));
        elapsed.as_millis() as u64
    }

    pub fn finish(self) -> u64 {
        self.finish_with("")
    }

    /// Measure the elapsed time without printing — for per-stage timers whose
    /// numbers are recorded in the snapshot but kept out of the console output.
    pub fn finish_quiet(self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}
