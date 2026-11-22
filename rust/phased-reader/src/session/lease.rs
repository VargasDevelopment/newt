// SPDX-License-Identifier: MIT
//! Scope the existing parser/display lease and its guaranteed return.
use super::Status;

/// The lender owns parser storage and its release callback, separately from the
/// reader's catalog/plan/selection storage. None means a valid short/base lease.
/// `finish` returns storage before the final source/generation guard; it must
/// also release on Drop during unwinding. Returned tail is exclusively borrowed.
pub trait ParseLease {
    fn acquire(&mut self) -> Result<Option<&mut [u8; 2048]>, Status>;
    fn finish(&mut self) -> Result<(), Status>;
}


/// Owner's existing parser/display lend and return callbacks. `acquire` reports
/// whether the extended tail is available; false is a successful base lease.
pub trait LeaseCallbacks {
    fn acquire(&mut self) -> Result<bool, Status>;
    fn release(&mut self) -> Result<(), Status>;
}
pub struct BorrowedParseLease<'a> {
    tail: &'a mut [u8; 2048],
    callbacks: &'a mut dyn LeaseCallbacks,
    held: bool,
}
impl<'a> BorrowedParseLease<'a> {
    pub fn new(tail: &'a mut [u8; 2048], callbacks: &'a mut dyn LeaseCallbacks) -> Self {
        Self {
            tail,
            callbacks,
            held: false,
        }
    }
}
impl ParseLease for BorrowedParseLease<'_> {
    fn acquire(&mut self) -> Result<Option<&mut [u8; 2048]>, Status> {
        if self.held {
            return Err(Status::Unsupported);
        }
        let extended = self.callbacks.acquire()?;
        self.held = true;
        Ok(if extended {
            Some(&mut *self.tail)
        } else {
            None
        })
    }
    fn finish(&mut self) -> Result<(), Status> {
        if !self.held {
            return Ok(());
        }
        self.held = false;
        self.callbacks.release()
    }
}
impl Drop for BorrowedParseLease<'_> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
