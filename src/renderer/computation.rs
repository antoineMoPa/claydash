//! Shared lifecycle and progress for background computations and incremental GPU jobs.
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage { Preparation, Sampling, Reconstruction, Training, Capture, TextureBake, Writing }

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub stage: Stage,
    pub percent: Option<u32>,
    pub seconds_remaining: Option<u64>,
}

impl Snapshot {
    pub fn remaining_label(&self) -> String {
        match self.seconds_remaining {
            Some(seconds) if seconds < 60 => format!("About {seconds}s left in this stage"),
            Some(seconds) => format!("About {} min left in this stage", seconds.div_ceil(60)),
            None if self.percent.is_some() => "Estimating time remaining…".into(),
            None => "Time remaining unavailable for this stage".into(),
        }
    }
}

struct Timing { stage: Stage, started: Instant, first: u32, last: u32, estimates: EstimateWindow }

#[derive(Default)]
struct EstimateWindow { samples: std::collections::VecDeque<(f64, f64)> }

impl EstimateWindow {
    fn update(&mut self, now: f64, remaining: Option<u64>) -> Option<u64> {
        let Some(remaining) = remaining else {
            self.samples.clear();
            return None;
        };
        // Sample once per second, independently of viewport repaint frequency.
        if self.samples.back().is_none_or(|(time, _)| now - time >= 1.0) {
            if self.samples.len() == 3 { self.samples.pop_front(); }
            self.samples.push_back((now, remaining as f64));
        }
        let sum: f64 = self.samples.iter()
            .map(|(time, seconds)| (seconds - (now - time)).max(0.0)).sum();
        Some((sum / self.samples.len() as f64).ceil().max(1.0) as u64)
    }
}

pub(crate) struct Computation {
    // Shared tokens adapt existing CPU callbacks and GPU polling to one lifecycle.
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    timing: Mutex<Timing>,
}

impl Computation {
    pub fn new(stage: Stage) -> Self {
        Self { progress: Arc::new(AtomicU32::new(0)), cancel: Arc::new(AtomicBool::new(false)),
            timing: Mutex::new(Timing { stage, started: Instant::now(), first: 0, last: 0, estimates: EstimateWindow::default() }) }
    }
    pub fn cancel(&self) { self.cancel.store(true, Ordering::Relaxed); }
    pub fn is_cancelled(&self) -> bool { self.cancel.load(Ordering::Relaxed) }
    pub fn percent(&self) -> u32 { self.progress.load(Ordering::Relaxed).min(100) }
    pub fn snapshot(&self, stage: Stage, percent: Option<u32>) -> Snapshot {
        let mut timing = self.timing.lock().unwrap();
        let current = percent.unwrap_or(0).min(100);
        if stage != timing.stage || current < timing.last {
            *timing = Timing { stage, started: Instant::now(), first: current, last: current, estimates: EstimateWindow::default() };
        }
        timing.last = current;
        let elapsed = timing.started.elapsed().as_secs_f64();
        let estimate = percent.and_then(|_| estimated_seconds_remaining(elapsed, timing.first, current));
        Snapshot { stage, percent, seconds_remaining: timing.estimates.update(elapsed, estimate) }
    }
}

impl Drop for Computation { fn drop(&mut self) { self.cancel(); } }

fn estimated_seconds_remaining(elapsed: f64, first: u32, percent: u32) -> Option<u64> {
    if elapsed < 2.0 || !elapsed.is_finite() || percent <= first || percent >= 100 { return None; }
    Some((elapsed * f64::from(100 - percent) / f64::from(percent - first)).ceil().max(1.0) as u64)
}

pub(crate) type Statuses = std::collections::HashMap<uuid::Uuid, Snapshot>;
pub(crate) fn status_id() -> egui::Id { egui::Id::new("computation-status") }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eta_averages_three_samples_after_subtracting_their_age() {
        let mut window = EstimateWindow::default();
        assert_eq!(window.update(0.0, Some(12)), Some(12));
        assert_eq!(window.update(1.0, Some(20)), Some(16));
        assert_eq!(window.update(2.0, Some(10)), Some(13));
        assert_eq!(window.update(3.0, Some(9)), Some(12));
        assert_eq!(window.update(3.5, Some(100)), Some(12), "repaints must not replace samples");
        assert_eq!(window.update(4.0, None), None);
        assert_eq!(window.update(5.0, Some(2)), Some(2), "new stage starts a fresh window");
        assert_eq!(window.update(20.0, Some(1)), Some(1), "old estimates cannot go negative");
    }
    #[test]
    fn eta_waits_for_progress_and_resets_for_a_new_stage() {
        assert_eq!(estimated_seconds_remaining(10.0, 20, 40), Some(30));
        assert_eq!(estimated_seconds_remaining(1.0, 0, 50), None);
        assert_eq!(estimated_seconds_remaining(10.0, 40, 40), None);
        assert_eq!(estimated_seconds_remaining(10.0, 50, 0), None);
        assert_eq!(estimated_seconds_remaining(10.0, 0, 100), None);
        let work = Computation::new(Stage::Sampling);
        work.timing.lock().unwrap().started -= std::time::Duration::from_secs(10);
        assert!(work.snapshot(Stage::Sampling, Some(50)).seconds_remaining.is_some());
        assert!(work.snapshot(Stage::Reconstruction, None).seconds_remaining.is_none());
        assert!(work.snapshot(Stage::Sampling, Some(0)).seconds_remaining.is_none());
    }
    #[test]
    fn dropping_owner_cancels_workers() {
        let work = Computation::new(Stage::Capture);
        let token = work.cancel.clone();
        drop(work);
        assert!(token.load(Ordering::Relaxed));
    }
}
