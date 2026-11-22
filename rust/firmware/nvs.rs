// SPDX-License-Identifier: MIT
//! NVS policy and the existing thin SDK call boundary.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[path = "nvs/nvs.rs"]
pub mod nvs;
#[path = "nvs/sdk.rs"]
pub mod sdk;

pub use nvs::{Inventory, Native, NativeError, NvsBlobBackend, TransactionState, reclaim};
pub use sdk::Sdk552;
