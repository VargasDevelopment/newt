//! Layout of a resume-point section inside a v4 page segment. MIT.
//!
//! A point lets a later producer pass restart at a page near the segment's
//! end instead of at the member's first byte. This module freezes only the
//! section's outer layout. No writer emits points yet, and no restore reads one.
//!
//! The sections follow the restore's consumption order. Restore reads the
//! section front to back with one small window and never seeks backwards:
//! the binding is checked before any state is used. The source is positioned
//! before the feed buffer receives prolog, names or replay. The replay comes
//! last, so it is still in the feed buffer when the output opens.
//!
//! ```text
//! header   "FIRP" u16 version u8 mode u8 0, then one u32 length per section
//! Binding  route, positions (pages at the cut, edge, cut, fed), member
//! Source   member position: raw bytes consumed
//! Stream   parser input state: counters and the whole entity filter, raw buffer included
//! Engine   layout engine state
//! Prolog   filtered bytes before the root element
//! Names    open element names: u8 depth, then u8 length + bytes per name
//! Replay   filtered bytes from the cut to the end of the last feed
//! trailer  u32 CRC-32 over the header and every section
//! ```
//!
//! A point that fails any check here is lost on its own. The pages of its
//! segment stay valid, because their extents come from the verified index and
//! footer, never from the point's content.
#![forbid(unsafe_code)]

pub const MAGIC: [u8; 4] = *b"FIRP";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 8 + 4 * SECTIONS.len();
pub const TRAILER_BYTES: u32 = 4;

/// How restore positions the chapter member at the cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A stored member: seek to the saved position.
    Seek,
    /// A deflated member: inflate from its start, discarding output.
    Reinflate,
}

/// The sections in wire order, which is also restore's consumption order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Binding,
    Source,
    Stream,
    Engine,
    Prolog,
    Names,
    Replay,
}

pub const SECTIONS: [Section; 7] = [
    Section::Binding,
    Section::Source,
    Section::Stream,
    Section::Engine,
    Section::Prolog,
    Section::Names,
    Section::Replay,
];

impl Section {
    /// Largest accepted length. Prolog, names and replay match the capture
    /// limits; the engine bound covers the largest layout storage profile.
    /// The stream holds two u64 counters and the entity filter, whose raw
    /// buffer can keep up to 512 unread bytes of the last pull, so restore
    /// continues from the source position after that pull: about 600 B.
    pub const fn max_bytes(self) -> u32 {
        match self {
            Self::Binding => 160,
            Self::Source => 16,
            Self::Stream => 640,
            Self::Engine => 24 * 1024,
            Self::Prolog => 512,
            Self::Names => 1 + 48 + 256,
            Self::Replay => 2048,
        }
    }
    /// Prolog and replay may be empty. Names always holds its depth byte.
    const fn min_bytes(self) -> u32 {
        match self {
            Self::Prolog | Self::Replay => 0,
            _ => 1,
        }
    }
}

/// A point's validated outer layout. Offsets are relative to the section start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub mode: Mode,
    lengths: [u32; SECTIONS.len()],
}

impl Layout {
    /// Checks the header against the point's length from the verified footer.
    /// Every length is bounded, so the arithmetic cannot overflow on RV32.
    pub fn parse(header: &[u8; HEADER_BYTES], point_bytes: u32) -> Option<Self> {
        if header[..4] != MAGIC
            || u16::from_le_bytes([header[4], header[5]]) != VERSION
            || header[7] != 0
        {
            return None;
        }
        let mode = match header[6] {
            0 => Mode::Seek,
            1 => Mode::Reinflate,
            _ => return None,
        };
        let mut lengths = [0; SECTIONS.len()];
        let mut total = HEADER_BYTES as u32 + TRAILER_BYTES;
        for (i, section) in SECTIONS.into_iter().enumerate() {
            let at = 8 + 4 * i;
            let length = u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]);
            if !(section.min_bytes()..=section.max_bytes()).contains(&length) {
                return None;
            }
            lengths[i] = length;
            total += length;
        }
        (total == point_bytes).then_some(Self { mode, lengths })
    }

    /// The section's (offset, length) within the point.
    pub fn range(&self, section: Section) -> (u32, u32) {
        let index = section as usize;
        let before: u32 = self.lengths[..index].iter().sum();
        (HEADER_BYTES as u32 + before, self.lengths[index])
    }

    /// Offset of the trailing CRC-32.
    pub fn trailer(&self) -> u32 {
        HEADER_BYTES as u32 + self.lengths.iter().sum::<u32>()
    }
}
