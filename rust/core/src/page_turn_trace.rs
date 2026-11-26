// SPDX-License-Identifier: MIT
//! Opt-in page-turn and segment-seal timing structures.
#![forbid(unsafe_code)]

/// Microsecond breakdown of one block-envelope cache file write and seal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SealBreakdown {
    pub write_us: u32,
    pub finalize_us: u32,
    pub read_us: u32,
    pub crc_us: u32,
    pub dir_us: u32,
    pub sync_us: u32,
    pub verify_us: u32,
    pub commit_us: u32,
}

/// Measured cost of producing and sealing one page-cache segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentTrace {
    pub idx: u32,
    pub pages: u32,
    pub bytes: u32,
    pub layout_us: u32,
    pub tee_us: u32,
    pub seal: SealBreakdown,
}
