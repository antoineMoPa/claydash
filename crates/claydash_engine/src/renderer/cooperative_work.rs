//! Small pauses for CPU jobs. Never call checkpoints from the UI thread.
pub(crate) struct WorkerBudget {
    #[cfg(not(target_arch = "wasm32"))]
    started: web_time::Instant,
}

impl WorkerBudget {
    pub fn new() -> Self {
        Self { #[cfg(not(target_arch = "wasm32"))] started: web_time::Instant::now() }
    }

    pub(crate) fn checkpoint(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.started.elapsed() >= std::time::Duration::from_millis(8) {
            // yield_now alone may immediately schedule this worker again.
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.started = web_time::Instant::now();
        }
    }
}

/// Called only at the start of dedicated native worker threads. Utility QoS
/// lets macOS favor interaction even during the opaque Poisson solver call.
#[cfg(not(target_arch = "wasm32"))]
pub fn background_priority() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
        }
        // QOS_CLASS_UTILITY from the macOS SDK's sys/qos.h. Failure leaves
        // default scheduling in place; cooperative checkpoints still work.
        unsafe { let _ = pthread_set_qos_class_self_np(0x11, 0); }
    }
}
