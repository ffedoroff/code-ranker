use chrono::Local;
use std::time::Instant;

fn ts() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

pub fn info(msg: &str) {
    eprintln!("[{}] {}", ts(), msg);
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
        let ms = self.start.elapsed().as_millis() as u64;
        eprintln!(
            "[{}] ✓ {} — {:.1}s{}",
            ts(),
            self.label,
            ms as f64 / 1000.0,
            if extra.is_empty() {
                String::new()
            } else {
                format!(" ({})", extra)
            }
        );
        ms
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
