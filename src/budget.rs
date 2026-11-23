//! The charge ledger for [`Limits::max_alloc`]: every buffer whose size comes from the input is
//! charged here before it is allocated, and released when a temporary buffer is dropped. A nested
//! call into another crate gets [`Budget::remaining`] as its own `max_alloc`.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Error, Limit, Limits};

/// One per public call. Not `Sync`: this crate runs on the calling thread only.
pub(crate) struct Budget<'a> {
    used: Cell<usize>,
    max: usize,
    cancel: &'a AtomicBool,
}

impl<'a> Budget<'a> {
    pub(crate) fn new(limits: &Limits, cancel: &'a AtomicBool) -> Self {
        Self {
            used: Cell::new(0),
            max: limits.max_alloc,
            cancel,
        }
    }

    /// Bytes still free under `max_alloc`.
    pub(crate) fn remaining(&self) -> usize {
        self.max.saturating_sub(self.used.get())
    }

    /// Charges `n` bytes, or fails with `LimitExceeded(Alloc)` and charges nothing.
    pub(crate) fn charge(&self, n: usize) -> Result<(), Error> {
        let used = self
            .used
            .get()
            .checked_add(n)
            .filter(|&u| u <= self.max)
            .ok_or(Error::LimitExceeded(Limit::Alloc))?;
        self.used.set(used);
        Ok(())
    }

    /// Gives back `n` bytes charged before (a buffer that was dropped).
    pub(crate) fn release(&self, n: usize) {
        self.used.set(self.used.get().saturating_sub(n));
    }

    /// An empty `Vec` with room for `n` items, charged first.
    pub(crate) fn vec<T>(&self, n: usize) -> Result<Vec<T>, Error> {
        let bytes = n
            .checked_mul(size_of::<T>())
            .ok_or(Error::LimitExceeded(Limit::Alloc))?;
        self.charge(bytes)?;
        let mut v = Vec::new();
        if v.try_reserve_exact(n).is_err() {
            self.release(bytes);
            return Err(Error::OutOfMemory);
        }
        Ok(v)
    }

    /// A charged copy of `s`.
    pub(crate) fn string(&self, s: &str) -> Result<String, Error> {
        let mut v = self.vec::<u8>(s.len())?;
        v.extend_from_slice(s.as_bytes());
        // `v` holds the bytes of a `str`, so this cannot fail; the fallback keeps the lint quiet.
        String::from_utf8(v).map_err(|_| Error::Malformed("utf8"))
    }

    /// `Err(Cancelled)` when the cancel flag is set.
    pub(crate) fn check_cancel(&self) -> Result<(), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// The call's cancel flag, for nested calls.
    pub(crate) fn cancel(&self) -> &'a AtomicBool {
        self.cancel
    }
}
