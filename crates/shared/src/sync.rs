//! `std`'s locks without poisoning: a panic on one thread does not turn into panics on the others.

use std::ops::{Deref, DerefMut};
use std::sync::{self, PoisonError, TryLockError};
pub use std::sync::{RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

#[derive(Default)]
pub struct Mutex<T: ?Sized>(sync::Mutex<T>);

/// The inner guard is only absent while [`Condvar::wait`] holds it.
pub struct MutexGuard<'a, T: ?Sized>(Option<sync::MutexGuard<'a, T>>);

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(sync::Mutex::new(value))
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> Mutex<T> {
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        MutexGuard(Some(self.0.lock().unwrap_or_else(PoisonError::into_inner)))
    }

    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.0.try_lock() {
            Ok(guard) => Some(MutexGuard(Some(guard))),
            Err(TryLockError::Poisoned(e)) => Some(MutexGuard(Some(e.into_inner()))),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.0.as_ref().unwrap()
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        self.0.as_mut().unwrap()
    }
}

#[derive(Default)]
pub struct Condvar(sync::Condvar);

impl Condvar {
    pub const fn new() -> Self {
        Self(sync::Condvar::new())
    }

    #[inline]
    pub fn notify_one(&self) {
        self.0.notify_one();
    }

    #[inline]
    pub fn notify_all(&self) {
        self.0.notify_all();
    }

    pub fn wait<T>(&self, guard: &mut MutexGuard<'_, T>) {
        let inner = guard.0.take().unwrap();
        guard.0 = Some(self.0.wait(inner).unwrap_or_else(PoisonError::into_inner));
    }

    pub fn wait_for<T>(&self, guard: &mut MutexGuard<'_, T>, timeout: Duration) {
        let inner = guard.0.take().unwrap();
        let (inner, _) = self
            .0
            .wait_timeout(inner, timeout)
            .unwrap_or_else(PoisonError::into_inner);
        guard.0 = Some(inner);
    }
}

#[derive(Default)]
pub struct RwLock<T: ?Sized>(sync::RwLock<T>);

impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self(sync::RwLock::new(value))
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> RwLock<T> {
    #[inline]
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    #[inline]
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }

    #[inline]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.0.try_read() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    #[inline]
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.0.try_write() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}
