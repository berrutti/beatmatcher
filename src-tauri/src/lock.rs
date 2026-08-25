use std::sync::{Mutex, MutexGuard};

/// Takes the guard even when a panic elsewhere poisoned the lock. The state behind it is
/// still valid audio state, and refusing it would unwind the audio thread over someone
/// else's failure.
pub(crate) trait LockIgnoringPoison<T> {
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> LockIgnoringPoison<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
