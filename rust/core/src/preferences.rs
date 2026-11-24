//! Reader view settings and NWTP v1/v2/v3 compatibility.
use crate::wire::{crc32, get32, put32};

pub const BYTES: usize = 24;

/// How the reader holds the X3. The two landscapes share one layout; the
/// clockwise one is the counterclockwise frame turned half a turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Orientation {
    Portrait = 0,
    /// Turned counterclockwise: the front buttons sit right of the text.
    #[default]
    LandscapeCcw = 1,
    /// Turned clockwise: the front buttons sit left of the text.
    LandscapeCw = 2,
}

impl Orientation {
    pub const ALL: [Self; 3] = [Self::Portrait, Self::LandscapeCcw, Self::LandscapeCw];

    /// Both landscapes lay out pages alike, so they share pages and caches.
    pub const fn is_landscape(self) -> bool {
        match self {
            Self::Portrait => false,
            Self::LandscapeCcw | Self::LandscapeCw => true,
        }
    }

    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Portrait),
            1 => Some(Self::LandscapeCcw),
            2 => Some(Self::LandscapeCw),
            _ => None,
        }
    }

    /// Dev USB and wake evidence name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::LandscapeCcw => "landscape-ccw",
            Self::LandscapeCw => "landscape-cw",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewSettings {
    pub orientation: Orientation,
    pub slim_progress_bar: bool,
    pub refresh_pages: u8,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: true,
            refresh_pages: 5,
        }
    }
}

pub fn valid_refresh_pages(value: u32) -> bool {
    matches!(value, 1 | 5 | 10)
}

/// Packed settings words (RTC record, resume intent): bit 0 is the layout
/// class, bit 4 the half turn, bit 1 the slim bar and bits 8..16 the refresh
/// cadence. Bits 2 and 3 belong to each record. Builds before the clockwise
/// choice reject bit 4 as reserved, so they never misread it.
pub const PACKED_SETTINGS: u32 = 0xff13;

impl ViewSettings {
    pub const fn packed(self) -> u32 {
        let orientation = match self.orientation {
            Orientation::Portrait => 0,
            Orientation::LandscapeCcw => 1,
            Orientation::LandscapeCw => 0x11,
        };
        orientation | ((self.slim_progress_bar as u32) << 1) | ((self.refresh_pages as u32) << 8)
    }

    /// Reads only the shared bits; the caller checks its own bits 2 and 3.
    pub fn unpacked(word: u32) -> Option<Self> {
        let orientation = match word & 0x11 {
            0 => Orientation::Portrait,
            1 => Orientation::LandscapeCcw,
            0x11 => Orientation::LandscapeCw,
            _ => return None,
        };
        let refresh = word >> 8 & 0xff;
        valid_refresh_pages(refresh).then_some(Self {
            orientation,
            slim_progress_bar: word & 2 != 0,
            refresh_pages: refresh as u8,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Preferences {
    pub sequence: u32,
    pub settings: ViewSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Corrupt,
    Unsupported,
}

impl Preferences {
    pub fn encode(&self) -> Option<[u8; BYTES]> {
        if !valid_refresh_pages(u32::from(self.settings.refresh_pages)) {
            return None;
        }
        let mut bytes = [0; BYTES];
        bytes[..4].copy_from_slice(b"NWTP");
        // Only the clockwise choice needs v3. Every other value stays v2, so
        // an older build keeps reading it; v3 makes it refuse, not misread.
        let version = if self.settings.orientation == Orientation::LandscapeCw { 3 } else { 2 };
        put32(&mut bytes, 4, version);
        put32(&mut bytes, 8, self.sequence);
        bytes[12] = self.settings.orientation as u8;
        bytes[13] = u8::from(self.settings.slim_progress_bar);
        bytes[14] = self.settings.refresh_pages;
        let checksum = crc32(&bytes[..20]);
        put32(&mut bytes, 20, checksum);
        Some(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 8 || &bytes[..4] != b"NWTP" {
            return Err(DecodeError::Corrupt);
        }
        let version = get32(bytes, 4);
        if !(1..=3).contains(&version) {
            return Err(DecodeError::Unsupported);
        }
        let Some(orientation) = bytes.get(12).copied().and_then(Orientation::from_byte)
            .filter(|&o| o != Orientation::LandscapeCw || version == 3)
        else {
            return Err(DecodeError::Corrupt);
        };
        if bytes.len() != BYTES
            || bytes[13] > 1
            || get32(bytes, 20) != crc32(&bytes[..20])
            || (version == 1 && bytes[14] != 0)
            || (version > 1 && !valid_refresh_pages(u32::from(bytes[14])))
            || bytes[15..20].iter().any(|&byte| byte != 0)
        {
            return Err(DecodeError::Corrupt);
        }
        Ok(Self {
            sequence: get32(bytes, 8),
            settings: ViewSettings {
                orientation,
                slim_progress_bar: bytes[13] == 1,
                refresh_pages: if version == 1 { 5 } else { bytes[14] },
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub width: u16,
    pub height: u16,
    pub margin: u16,
    pub content_bottom: u16,
    pub bar_y: u16,
    pub footer_y: u16,
}

pub fn geometry(settings: ViewSettings) -> Geometry {
    let (width, height) = if settings.orientation.is_landscape() {
        (crate::frame::HEIGHT as u16, crate::frame::WIDTH as u16)
    } else {
        (crate::frame::WIDTH as u16, crate::frame::HEIGHT as u16)
    };
    Geometry {
        width,
        height,
        margin: 16,
        content_bottom: height - 48,
        bar_y: height - 42,
        footer_y: height - 33,
    }
}
