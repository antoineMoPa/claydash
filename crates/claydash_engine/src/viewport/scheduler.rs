//! Submission ownership and stage-local throughput. GPU fences release work;
//! timestamp readbacks are optional observations, never a liveness dependency.
use super::{budget_after_sample, Work, INITIAL_PIXELS, MIN_PREVIEW_PIXELS, MIN_COVERAGE_PIXELS};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum Plan {
    Tiles { total: u32 },
    Sparse { phase_samples: u32 },
}
impl Plan {
    pub fn new(size: [u32; 2], sparse: bool) -> Self {
        if sparse {
            Self::Sparse {
                phase_samples: super::sparse_phase_pixels(size),
            }
        } else {
            Self::Tiles {
                total: super::tile_count(size),
            }
        }
    }
    pub fn total(self) -> u32 {
        match self {
            Self::Tiles { total } => total,
            Self::Sparse { phase_samples } => 257 * phase_samples,
        }
    }
    pub fn stage(self, cursor: u32) -> Stage {
        match self {
            Self::Tiles { .. } => Stage::Tiles,
            Self::Sparse { phase_samples } if cursor < phase_samples => Stage::Coverage,
            Self::Sparse { .. } => Stage::Sparse,
        }
    }
    /// Every ready step advances a finite cursor, even at zero/invalid budget.
    /// Coverage and full-material work can never share one submitted batch.
    pub fn next(self, cursor: u32, pixels: u32) -> Option<Work> {
        if cursor >= self.total() {
            return None;
        }
        let (count, sparse) = match self {
            Self::Tiles { .. } => ((pixels / 1024).max(1), false),
            Self::Sparse { phase_samples } if cursor < phase_samples => {
                (pixels.max(64).min(phase_samples - cursor), true)
            }
            Self::Sparse { phase_samples } => (pixels.clamp(1, 256 * phase_samples), true),
        };
        let boundary = self.total();
        let end = cursor.saturating_add(count).min(boundary);
        Some(if sparse {
            Work::Interleave { first: cursor, end }
        } else {
            Work::Refine { first: cursor, end }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Preview,
    Coverage,
    Sparse,
    Tiles,
    Relight,
}

impl Stage {
    fn index(self) -> usize {
        match self {
            Self::Preview => 0,
            Self::Coverage => 1,
            Self::Sparse => 2,
            Self::Tiles => 3,
            Self::Relight => 4,
        }
    }
    pub fn minimum(self) -> u32 {
        match self {
            Self::Preview => MIN_PREVIEW_PIXELS,
            Self::Coverage => MIN_COVERAGE_PIXELS,
            Self::Sparse => 1,
            Self::Tiles => 1024,
            Self::Relight => 1,
        }
    }
    fn initial(self, ceiling: u32) -> u32 {
        match self {
            Self::Preview => ceiling,
            Self::Coverage => ceiling.min(1024),
            Self::Sparse => ceiling.min(256),
            Self::Tiles => ceiling.min(1024),
            Self::Relight => 1,
        }
        .max(self.minimum())
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Batch {
    pub work: Work,
    generation: u64,
    pub stage: Stage,
    pixels: u32,
    budget: u32,
}

#[derive(Default)]
pub(super) struct Completion {
    fence: AtomicBool,
    pub milliseconds: Mutex<Option<f64>>,
    receipt: AtomicBool,
}
impl Completion {
    pub fn finish_fence(&self) {
        self.fence.store(true, Ordering::Release);
    }
    pub fn verify_receipt(&self, valid: bool) {
        self.receipt.store(valid, Ordering::Release);
    }
    #[cfg(test)]
    pub fn finish(&self) {
        self.verify_receipt(true);
        self.finish_fence();
    }
    pub fn finished(&self) -> bool {
        self.fence.load(Ordering::Acquire)
    }
}

enum Submission {
    Idle,
    Encoded(Batch),
    Submitted {
        batch: Batch,
        completion: Arc<Completion>,
        started: web_time::Instant,
    },
}

pub(super) struct Scheduler {
    generation: u64,
    submission: Submission,
    budgets: [u32; 5],
    pub last_gpu_ms: Option<f64>,
    pub submitted_count: u64,
    pub completed_count: u64,
    pub fault: Option<&'static str>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            generation: 0,
            submission: Submission::Idle,
            budgets: Self::initial_budgets(INITIAL_PIXELS),
            last_gpu_ms: None,
            submitted_count: 0,
            completed_count: 0,
            fault: None,
        }
    }
}

impl Scheduler {
    fn initial_budgets(ceiling: u32) -> [u32; 5] {
        [
            Stage::Preview,
            Stage::Coverage,
            Stage::Sparse,
            Stage::Tiles,
            Stage::Relight,
        ]
        .map(|stage| stage.initial(ceiling))
    }
    pub fn new_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        // Preserve the real in-flight fence when a view/document is replaced.
        if matches!(self.submission, Submission::Encoded(_)) {
            self.submission = Submission::Idle;
        }
    }
    pub fn reset_budgets(&mut self, ceiling: u32) {
        self.budgets = Self::initial_budgets(ceiling);
    }
    pub fn scale_budgets(&mut self, ratio: f64) {
        for stage in [
            Stage::Preview,
            Stage::Coverage,
            Stage::Sparse,
            Stage::Tiles,
            Stage::Relight,
        ] {
            self.budgets[stage.index()] = ((self.budgets[stage.index()] as f64 * ratio) as u32)
                .clamp(stage.minimum(), 4 * 1024 * 1024);
        }
    }
    pub fn budget(&self, stage: Stage, ceiling: u32) -> u32 {
        self.budgets[stage.index()]
            .max(stage.minimum())
            .min(ceiling.max(stage.minimum()))
    }
    pub fn busy(&self) -> bool {
        matches!(&self.submission, Submission::Submitted { completion, .. } if !completion.finished())
    }
    pub fn gpu_fault(&self) -> Option<&'static str> {
        self.fault.or_else(|| match &self.submission {
            Submission::Submitted { completion, .. }
                if completion.finished() && !completion.receipt.load(Ordering::Acquire) =>
            {
                Some("GPU command buffer did not execute its completion receipt")
            }
            _ => None,
        })
    }
    pub fn abandon_encoded(&mut self) -> Option<Work> {
        if let Submission::Encoded(batch) = &self.submission {
            let work = batch.work;
            self.submission = Submission::Idle;
            return Some(work);
        }
        None
    }
    pub fn retire(&mut self, target_ms: f64) {
        if let Submission::Submitted {
            completion,
            started,
            ..
        } = &self.submission
        {
            if !completion.finished() && started.elapsed() > std::time::Duration::from_secs(2) {
                self.fault = Some("GPU scene work did not complete within two seconds");
            }
        }
        if !matches!(&self.submission, Submission::Submitted { completion, .. } if completion.finished())
        {
            return;
        }
        let Submission::Submitted {
            batch,
            completion,
            started,
        } = std::mem::replace(&mut self.submission, Submission::Idle)
        else {
            unreachable!()
        };
        self.completed_count += 1;
        if !completion.receipt.load(Ordering::Acquire) {
            self.fault = Some("GPU command buffer did not execute its completion receipt");
            return;
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let ms = (*completion.milliseconds.lock().unwrap())
            .filter(|ms| ms.is_finite() && *ms > 0.0 && *ms <= elapsed_ms + 1.0);
        self.last_gpu_ms = ms;
        if batch.generation != self.generation || batch.pixels == 0 {
            return;
        }
        let budget = &mut self.budgets[batch.stage.index()];
        if let Some(ms) = ms {
            // Stationary native refinement can use a longer GPU slice than
            // navigation previews. Heavy source kernels have fixed costs
            // above8ms; forcing them into that target collapses throughput.
            let target_ms = match batch.stage {
                Stage::Sparse => target_ms.max(48.0),
                _ => target_ms,
            };
            *budget = budget_after_sample(
                *budget,
                batch.budget,
                batch.pixels,
                ms,
                target_ms,
                batch.stage.minimum(),
            );
        } else if started.elapsed() > std::time::Duration::from_millis(32) {
            // Poll frequency cannot repeatedly halve a pending batch.
            *budget = (*budget / 2).max(batch.stage.minimum());
        } else {
            // Empty/padded regions and some drivers report zero timestamps.
            // A quickly completed real fence still demonstrates headroom;
            // preserving a tiny budget here would require thousands of idle
            // redraws to cross cheap regions and look like a permanent stall.
            *budget = budget
                .saturating_mul(6)
                .div_ceil(5)
                .max(batch.stage.minimum())
                .min(4 * 1024 * 1024);
        }
    }
    pub fn encoded(&mut self, work: Work, stage: Stage, pixels: u32, budget: u32, target_ms: f64) {
        self.retire(target_ms);
        assert!(
            self.fault.is_none() && matches!(self.submission, Submission::Idle),
            "one owned scene submission at a time"
        );
        self.submission = Submission::Encoded(Batch {
            work,
            stage,
            pixels,
            budget,
            generation: self.generation,
        });
    }
    pub fn submitted(&mut self, work: Work) -> Arc<Completion> {
        let Submission::Encoded(batch) = std::mem::replace(&mut self.submission, Submission::Idle)
        else {
            panic!("submit requires an encoded batch")
        };
        assert_eq!(batch.work, work);
        let completion = Arc::new(Completion::default());
        self.submission = Submission::Submitted {
            batch,
            completion: completion.clone(),
            started: web_time::Instant::now(),
        };
        self.submitted_count += 1;
        completion
    }
    #[cfg(any(test, all(not(target_arch = "wasm32"), unix)))]
    pub fn stage(&self) -> Option<Stage> {
        match &self.submission {
            Submission::Idle => None,
            Submission::Encoded(batch) | Submission::Submitted { batch, .. } => Some(batch.stage),
        }
    }
    #[cfg(any(test, all(not(target_arch = "wasm32"), unix)))]
    pub fn state(&self) -> &'static str {
        match &self.submission {
            Submission::Idle => "idle",
            Submission::Encoded(_) => "encoded",
            Submission::Submitted { completion, .. } if completion.finished() => "completed",
            Submission::Submitted { .. } => "gpu_pending",
        }
    }
    #[cfg(any(test, all(not(target_arch = "wasm32"), unix)))]
    pub fn generation(&self) -> u64 {
        self.generation
    }
    #[cfg(any(test, all(not(target_arch = "wasm32"), unix)))]
    pub fn current_gpu_ms(&self) -> Option<f64> {
        match &self.submission {
            Submission::Submitted { completion, .. } => {
                (*completion.milliseconds.lock().unwrap()).or(self.last_gpu_ms)
            }
            _ => self.last_gpu_ms,
        }
    }
    #[cfg(test)]
    fn set_elapsed(&mut self, elapsed: std::time::Duration) {
        if let Submission::Submitted { started, .. } = &mut self.submission {
            *started = web_time::Instant::now() - elapsed;
        }
    }
    #[cfg(test)]
    pub fn set_budget(&mut self, stage: Stage, pixels: u32) {
        self.budgets[stage.index()] = pixels;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stable_plan_finishes_despite_missing_and_out_of_order_observations() {
        let mut scheduler = Scheduler::default();
        let mut previous: Option<Arc<Completion>> = None;
        // Repeated new views, changing dimensions, quality, and rendering paths.
        for view in 0..500 {
            scheduler.new_generation();
            if view % 7 == 0 {
                scheduler.reset_budgets(1024);
            }
            let plan = Plan::new([17 + view % 49, 31 + view % 19], view % 3 != 0);
            let mut cursor = 0;
            let mut steps = 0;
            while let Some(work) = plan.next(cursor, scheduler.budget(plan.stage(cursor), 4096)) {
                let (first, end) = match work {
                    Work::Interleave { first, end } | Work::Refine { first, end } => (first, end),
                    _ => unreachable!(),
                };
                assert_eq!(first, cursor);
                assert!(end > first && end <= plan.total());
                let stage = plan.stage(cursor);
                if let Plan::Sparse { phase_samples } = plan {
                    assert!(
                        first >= phase_samples || end <= phase_samples,
                        "a batch cannot straddle the quality boundary"
                    );
                }
                let budget = scheduler.budget(stage, 4096);
                scheduler.encoded(work, stage, budget, budget, 8.0);
                if steps % 7 == 0 {
                    assert_eq!(scheduler.abandon_encoded(), Some(work));
                    scheduler.encoded(work, stage, budget, budget, 8.0);
                }
                let current = scheduler.submitted(work);
                // Late callbacks on a retired ticket cannot free this batch.
                if let Some(old) = previous.take() {
                    *old.milliseconds.lock().unwrap() = Some(999999.0);
                    old.finish();
                }
                for _ in 0..3 {
                    scheduler.retire(8.0);
                    assert!(scheduler.busy());
                }
                match steps % 5 {
                    0 => {} // readback never arrives
                    1 => *current.milliseconds.lock().unwrap() = Some(f64::NAN),
                    2 => *current.milliseconds.lock().unwrap() = Some(0.0),
                    _ => *current.milliseconds.lock().unwrap() = Some(100.0),
                }
                current.finish();
                scheduler.retire(8.0);
                assert!(!scheduler.busy());
                previous = Some(current);
                cursor = end;
                steps += 1;
                assert!(
                    steps <= plan.total(),
                    "finite ready work must not stop or retry indefinitely"
                );
            }
            assert_eq!(cursor, plan.total());
        }
        assert_eq!(scheduler.completed_count, scheduler.submitted_count);
    }

    #[test]
    fn fence_retires_without_any_timing_callback() {
        let mut scheduler = Scheduler::default();
        scheduler.encoded(Work::Preview, Stage::Preview, 64, 64, 8.0);
        assert!(
            !scheduler.busy(),
            "unsubmitted commands cannot own a GPU fence"
        );
        let signal = scheduler.submitted(Work::Preview);
        assert!(scheduler.busy());
        signal.finish();
        scheduler.retire(8.0);
        assert_eq!(scheduler.state(), "idle");
        assert_eq!(scheduler.completed_count, 1);
    }

    #[test]
    fn late_timestamp_and_old_generation_cannot_release_or_resize_new_batch() {
        let mut scheduler = Scheduler::default();
        scheduler.encoded(Work::Preview, Stage::Preview, 4096, 4096, 8.0);
        let old = scheduler.submitted(Work::Preview);
        scheduler.new_generation();
        assert!(scheduler.busy());
        old.finish();
        scheduler.retire(8.0);
        scheduler.encoded(
            Work::Interleave { first: 0, end: 64 },
            Stage::Coverage,
            64,
            64,
            8.0,
        );
        let current = scheduler.submitted(Work::Interleave { first: 0, end: 64 });
        *old.milliseconds.lock().unwrap() = Some(99999.0);
        old.finish();
        scheduler.retire(8.0);
        assert!(scheduler.busy());
        assert_eq!(
            scheduler.budget(Stage::Preview, INITIAL_PIXELS),
            INITIAL_PIXELS
        );
        current.finish();
        scheduler.retire(8.0);
        assert_eq!(scheduler.completed_count, 2);
    }

    #[test]
    fn expensive_native_work_cannot_poison_preview_or_coverage_budgets() {
        let mut scheduler = Scheduler::default();
        for _ in 0..100 {
            let budget = scheduler.budget(Stage::Sparse, INITIAL_PIXELS);
            scheduler.encoded(
                Work::Interleave { first: 1, end: 2 },
                Stage::Sparse,
                budget,
                budget,
                8.0,
            );
            let signal = scheduler.submitted(Work::Interleave { first: 1, end: 2 });
            *signal.milliseconds.lock().unwrap() = Some(1000.0);
            scheduler.set_elapsed(std::time::Duration::from_millis(1001));
            signal.finish();
            scheduler.retire(8.0);
        }
        assert_eq!(
            scheduler.budget(Stage::Sparse, INITIAL_PIXELS),
            Stage::Sparse.minimum()
        );
        assert_eq!(
            scheduler.budget(Stage::Preview, INITIAL_PIXELS),
            INITIAL_PIXELS
        );
        assert_eq!(scheduler.budget(Stage::Coverage, INITIAL_PIXELS), 1024);
    }

    #[test]
    fn dropped_encoding_is_recoverable_without_a_fake_gpu_timeout() {
        let mut scheduler = Scheduler::default();
        let work = Work::Interleave { first: 20, end: 21 };
        scheduler.encoded(work, Stage::Sparse, 16, 16, 8.0);
        assert_eq!(scheduler.abandon_encoded(), Some(work));
        assert_eq!(scheduler.state(), "idle");
        scheduler.encoded(work, Stage::Sparse, 16, 16, 8.0);
        scheduler.submitted(work).finish();
        scheduler.retire(8.0);
        assert_eq!(scheduler.completed_count, 1);
    }

    #[test]
    fn zero_missing_and_invalid_timestamps_recover_from_the_minimum() {
        for observation in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(6.1e12),
        ] {
            let mut scheduler = Scheduler::default();
            scheduler.set_budget(Stage::Sparse, 16);
            for _ in 0..20 {
                let budget = scheduler.budget(Stage::Sparse, INITIAL_PIXELS);
                let work = Work::Interleave { first: 20, end: 21 };
                scheduler.encoded(work, Stage::Sparse, budget, budget, 8.0);
                let signal = scheduler.submitted(work);
                *signal.milliseconds.lock().unwrap() = observation;
                signal.finish();
                scheduler.retire(8.0);
            }
            assert!(
                scheduler.budget(Stage::Sparse, INITIAL_PIXELS) > 512,
                "unusable profiling observations must not strand cheap completed work at one block"
            );
        }
    }

    #[test]
    fn failed_gpu_receipt_is_a_fault_even_if_the_queue_fence_reports_done() {
        let mut scheduler = Scheduler::default();
        scheduler.encoded(Work::Preview, Stage::Preview, 64, 64, 8.0);
        let signal = scheduler.submitted(Work::Preview);
        signal.verify_receipt(false);
        signal.finish_fence();
        assert!(
            scheduler.gpu_fault().is_some(),
            "failed work is visible before the next scheduler poll"
        );
        scheduler.retire(8.0);
        assert!(scheduler.fault.is_some());
        scheduler.new_generation();
        assert!(
            scheduler.fault.is_some(),
            "a camera change cannot heal a failed GPU"
        );
    }

    #[test]
    fn uncompleted_gpu_work_has_a_terminal_recovery_deadline() {
        let mut scheduler = Scheduler::default();
        scheduler.encoded(Work::Preview, Stage::Preview, 64, 64, 8.0);
        scheduler.submitted(Work::Preview);
        scheduler.set_elapsed(std::time::Duration::from_secs(3));
        scheduler.retire(8.0);
        assert!(scheduler.fault.is_some());
        assert!(
            scheduler.busy(),
            "a timeout must not pretend the physical fence completed"
        );
    }
}
