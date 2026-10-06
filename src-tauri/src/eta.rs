//! Direct port of backend/src/utils/eta.ts -- EWMA-smoothed rate + re-anchored
//! countdown ETA, used for download/sideload progress.

use std::time::Instant;

pub struct EtaEstimator {
    alpha: f64,
    reanchor_threshold: f64,
    min_sample_seconds: f64,

    last_sample_time: Instant,
    last_sample_done: f64,
    smoothed_units_per_second: f64,

    eta_anchor_value: Option<f64>,
    eta_anchor_time: Instant,
}

impl EtaEstimator {
    pub fn new(alpha: f64, reanchor_threshold: f64, min_sample_seconds: f64) -> Self {
        let now = Instant::now();
        Self {
            alpha,
            reanchor_threshold,
            min_sample_seconds,
            last_sample_time: now,
            last_sample_done: 0.0,
            smoothed_units_per_second: 0.0,
            eta_anchor_value: None,
            eta_anchor_time: now,
        }
    }

    pub fn default_tuning() -> Self {
        Self::new(0.05, 0.2, 0.15)
    }

    pub fn reset(&mut self) {
        let now = Instant::now();
        self.last_sample_time = now;
        self.last_sample_done = 0.0;
        self.smoothed_units_per_second = 0.0;
        self.eta_anchor_value = None;
        self.eta_anchor_time = now;
    }

    pub fn update(&mut self, total_units: f64, done_units: f64) {
        if total_units <= 0.0 {
            return;
        }
        let now = Instant::now();
        let done_units = done_units.clamp(0.0, total_units);
        let remaining_units = (total_units - done_units).max(0.0);

        let dt = now.duration_since(self.last_sample_time).as_secs_f64();
        let d_units = done_units - self.last_sample_done;

        if dt >= self.min_sample_seconds && d_units > 0.0 {
            let inst_rate = d_units / dt;
            self.smoothed_units_per_second = if self.smoothed_units_per_second <= 0.0 {
                inst_rate
            } else {
                self.alpha * inst_rate + (1.0 - self.alpha) * self.smoothed_units_per_second
            };
            self.last_sample_time = now;
            self.last_sample_done = done_units;
        }

        if self.smoothed_units_per_second > 1e-6 && remaining_units > 0.0 {
            let new_eta = remaining_units / self.smoothed_units_per_second;

            match self.eta_anchor_value {
                None => {
                    self.eta_anchor_value = Some(new_eta);
                    self.eta_anchor_time = now;
                }
                Some(anchor) => {
                    let elapsed = now.duration_since(self.eta_anchor_time).as_secs_f64();
                    let predicted_now = (anchor - elapsed).max(0.0);
                    let base_seconds = predicted_now.max(1.0);
                    let diff_ratio = (new_eta - predicted_now).abs() / base_seconds;

                    if diff_ratio > self.reanchor_threshold {
                        self.eta_anchor_value = Some(new_eta);
                        self.eta_anchor_time = now;
                    }
                }
            }
        }
    }

    pub fn speed(&self) -> f64 {
        self.smoothed_units_per_second
    }

    pub fn display_eta(&self) -> Option<u64> {
        let anchor = self.eta_anchor_value?;
        let elapsed = Instant::now().duration_since(self.eta_anchor_time).as_secs_f64();
        let remaining = anchor - elapsed;
        Some(remaining.max(0.0).ceil() as u64)
    }
}
