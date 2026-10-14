//! Small synchronisation helpers.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `m`, recovering the guard if another thread panicked while holding it.
///
/// The shared data here (register files, log buffers, status strings) stays
/// structurally valid after a panic, so one failed thread should not take every
/// other thread, or the GUI, down with it.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
