// SPDX-License-Identifier: MIT
//! Read-only discovery is separate from ordinary record access.
use super::{Native, NativeError};

pub trait Inventory: Native {
    /// One complete SDK entry walk. The visitor cannot reenter NVS; returning
    /// false aborts the walk, and an incomplete walk must return an error.
    fn visit_entries(&mut self, visitor: &mut dyn FnMut(&[u8], &[u8], bool) -> bool)
        -> Result<(), NativeError>;
}
