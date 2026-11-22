// SPDX-License-Identifier: MIT

//! Allocation-free owned policy from pinned firmware/nvs_guard.cpp.
//! No SDK calls or global state; begin/end loans end before native callbacks.
use crate::save_notice::NvsIssue;

pub const OK: i32 = 0;
pub const INVALID_STATE: i32 = 0x103;
pub const NO_FREE_PAGES: i32 = 0x110d;
pub const NEW_VERSION: i32 = 0x1110;
pub const READONLY: i32 = 0;

#[derive(Clone, Copy)]
pub struct Partition {
    pub kind: u8,
    pub subtype: u8,
    pub size: usize,
}

impl Partition {
    fn is_nvs(self) -> bool {
        self.kind == 1 && self.subtype == 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Refusals {
    pub opens: u8,
    pub writes: u8,
}

pub struct Guard {
    initializing: bool,
    denied_startup_mutation: bool,
    initialization_result: i32,
    wifi_fence: bool,
    refused_opens: u8,
    refused_writes: u8,
}

impl Guard {
    pub const fn new() -> Self {
        Self {
            initializing: false,
            denied_startup_mutation: false,
            initialization_result: INVALID_STATE,
            wifi_fence: false,
            refused_opens: 0,
            refused_writes: 0,
        }
    }

    /// False refuses reentry without changing any state or calling native init.
    /// End this loan before native init; finish exactly once after its return.
    pub fn begin_initialization(&mut self) -> bool {
        if self.initializing {
            return false;
        }
        self.initializing = true;
        self.denied_startup_mutation = false;
        self.initialization_result = INVALID_STATE;
        true
    }

    /// Caller pairs this with its successful begin, after all nested callbacks.
    pub fn finish_initialization(&mut self, result: i32) -> i32 {
        self.initializing = false;
        self.initialization_result = if result == OK && self.denied_startup_mutation {
            INVALID_STATE
        } else {
            result
        };
        self.initialization_result
    }

    fn deny_mutation(&mut self, partition: Option<Partition>, whole_erase: bool) -> bool {
        if self.wifi_fence {
            self.refused_writes = self.refused_writes.saturating_add(1);
            return true;
        }
        if !partition.is_some_and(Partition::is_nvs) {
            return false;
        }
        let denied = self.initializing || self.initialization_result != OK || whole_erase;
        if denied && self.initializing {
            self.denied_startup_mutation = true;
        }
        denied
    }

    pub fn deny_write(&mut self, partition: Option<Partition>) -> bool {
        self.deny_mutation(partition, false)
    }

    pub fn deny_erase(&mut self, partition: Option<Partition>, offset: usize, size: usize) -> bool {
        let whole_erase = partition.is_some_and(|p| offset == 0 && size >= p.size);
        self.deny_mutation(partition, whole_erase)
    }

    pub fn deny_open(&mut self, mode: i32) -> bool {
        if self.wifi_fence && mode != READONLY {
            self.refused_opens = self.refused_opens.saturating_add(1);
            return true;
        }
        false
    }

    /// Every format wrapper uses this, including a null partition/name.
    pub fn deny_format(&mut self) -> i32 {
        if self.initializing {
            self.denied_startup_mutation = true;
        }
        INVALID_STATE
    }

    pub fn storage_ready(&self) -> bool {
        !self.initializing && self.initialization_result == OK
    }

    pub fn storage_issue(&self) -> NvsIssue {
        if self.storage_ready() {
            return NvsIssue::None;
        }
        if self.initialization_result == NEW_VERSION {
            return NvsIssue::Version;
        }
        if self.initialization_result == NO_FREE_PAGES {
            return NvsIssue::Recovery;
        }
        NvsIssue::Unavailable
    }

    pub fn fence_for_wifi(&mut self, raised: bool) {
        if raised {
            self.refused_opens = 0;
            self.refused_writes = 0;
        }
        self.wifi_fence = raised;
    }

    pub fn flash_fenced(&self) -> bool {
        self.wifi_fence
    }

    pub fn refusals(&self) -> Refusals {
        Refusals {
            opens: self.refused_opens,
            writes: self.refused_writes,
        }
    }
}
