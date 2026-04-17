use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// In-process registry of run-level cancellation flags so the orchestrator
/// doesn't have to poll the database to check if a run has been cancelled.
///
/// A run is "cancelled" when its flag transitions to `true`. Consumers should
/// read the flag with `Ordering::Relaxed` at every safe interruption point.
#[derive(Clone, Default)]
pub struct CancellationRegistry {
    inner: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new run and return its cancellation flag.
    pub fn register(&self, run_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(run_id.to_string(), flag.clone());
        }
        flag
    }

    /// Mark a run as cancelled. Returns `true` if a flag was found for it.
    pub fn cancel(&self, run_id: &str) -> bool {
        if let Ok(guard) = self.inner.lock() {
            if let Some(flag) = guard.get(run_id) {
                flag.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Remove a run's flag from the registry (e.g. after completion).
    pub fn clear(&self, run_id: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.remove(run_id);
        }
    }

    /// Check whether a run is cancelled.
    pub fn is_cancelled(&self, run_id: &str) -> bool {
        if let Ok(guard) = self.inner.lock() {
            if let Some(flag) = guard.get(run_id) {
                return flag.load(Ordering::Relaxed);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_and_clear() {
        let reg = CancellationRegistry::new();
        let flag = reg.register("run-1");
        assert!(!flag.load(Ordering::Relaxed));
        assert!(!reg.is_cancelled("run-1"));

        assert!(reg.cancel("run-1"));
        assert!(flag.load(Ordering::Relaxed));
        assert!(reg.is_cancelled("run-1"));

        reg.clear("run-1");
        assert!(!reg.is_cancelled("run-1"));
        assert!(!reg.cancel("run-1"));
    }
}
