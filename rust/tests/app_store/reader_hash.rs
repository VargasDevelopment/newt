// SPDX-License-Identifier: MIT
//! Stands in for the X3's mbedTLS provider: real SHA-256, or a provider
//! failure for every input or for one exact input.
use std::cell::RefCell;
pub enum Fail {
    Every,
    Input(Vec<u8>),
}
thread_local! {
    pub static FAIL: RefCell<Option<Fail>> = const { RefCell::new(None) };
}
pub(crate) fn package_hash(input: &[u8], output: &mut [u8; 32]) -> bool {
    let fails = FAIL.with_borrow(|fail| match fail {
        Some(Fail::Every) => true,
        Some(Fail::Input(bytes)) => bytes == input,
        None => false,
    });
    if fails { return false; }
    *output = crate::sha256::sha256(input);
    true
}
