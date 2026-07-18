//! Env-gated (`SLOPGB_PERF=1`) wall-time section accounting for `flush` —
//! prints per-section totals every ~500 GB frames of flushes, then resets.

use std::cell::RefCell;
use std::time::{Duration, Instant};

const N: usize = 7;
static LABELS: [&str; N] = ["mmio", "ppu", "icd2", "mediate", "spc", "cpu", "emit"];

// One flush per FLUSH_CHUNK (4096 GB cycles) => ~17.1 flushes per GB frame.
const REPORT_EVERY: u64 = 500 * 70_224 / 4096;

thread_local! {
    static ACC: RefCell<([Duration; N], u64)> = const { RefCell::new(([Duration::ZERO; N], 0)) };
}

pub(crate) struct PerfTimer(Option<Instant>);

impl PerfTimer {
    pub fn start() -> Self {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let on = *ON.get_or_init(|| std::env::var_os("SLOPGB_PERF").is_some());
        PerfTimer(on.then(Instant::now))
    }

    pub fn lap(&mut self, i: usize) {
        if let Some(last) = &mut self.0 {
            let now = Instant::now();
            ACC.with(|a| a.borrow_mut().0[i] += now - *last);
            *last = now;
        }
    }

    pub fn finish(self) {
        if self.0.is_none() {
            return;
        }
        ACC.with(|a| {
            let (acc, count) = &mut *a.borrow_mut();
            *count += 1;
            if *count >= REPORT_EVERY {
                let total: Duration = acc.iter().sum();
                let mut line = format!("PERF {count} flushes total {total:?}:");
                for (lbl, d) in LABELS.iter().zip(acc.iter()) {
                    line += &format!(" {lbl}={d:?}");
                }
                eprintln!("{line}");
                *acc = [Duration::ZERO; N];
                *count = 0;
            }
        });
    }
}
