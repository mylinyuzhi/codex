//! Shared lock-poisoning recovery helpers.
//!
//! Both `HookRegistry` and `AsyncHookTracker` use `std::sync::RwLock` for interior
//! mutability. A panicking thread can poison the lock. These helpers recover by
//! logging a warning and returning the inner guard, which keeps the system running
//! instead of propagating the panic to unrelated tasks.

use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

/// Acquires a write lock, recovering from poison if necessary.
pub(crate) fn lock_write<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(g) => g,
        Err(poisoned) => {
            tracing::warn!("{name} lock poisoned, recovering");
            poisoned.into_inner()
        }
    }
}

/// Acquires a read lock, recovering from poison if necessary.
pub(crate) fn lock_read<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(g) => g,
        Err(poisoned) => {
            tracing::warn!("{name} lock poisoned, recovering");
            poisoned.into_inner()
        }
    }
}
