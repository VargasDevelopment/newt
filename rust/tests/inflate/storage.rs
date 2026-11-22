//! Compile the actual Core implementation and its normal integration tests.
extern crate self as newt_core;
#[path = "../../core/src/inflate.rs"]
pub mod inflate;
#[path = "../../core/tests/inflate.rs"]
mod existing;
#[path = "../../core/tests/inflate_storage.rs"]
mod storage;
