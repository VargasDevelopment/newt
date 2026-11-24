//! Reader view settings and NWTP v1-v4 compatibility.
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

/// Reading text size, named by its ascent in pixels. 30 px was the only size
/// before the choice existed, so it stays the default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum TextSize {
    Px22 = 22,
    Px24 = 24,
    Px27 = 27,
    #[default]
    Px30 = 30,
    Px34 = 34,
}

impl TextSize {
    pub const ALL: [Self; 5] = [Self::Px22, Self::Px24, Self::Px27, Self::Px30, Self::Px34];

    pub const fn px(self) -> u16 {
        self as u16
    }

    /// Stored byte: 0 for the default, so a default record or word stays
    /// byte-identical to one written before sizes existed.
    pub const fn byte(self) -> u8 {
        match self {
            Self::Px30 => 0,
            other => other as u8,
        }
    }

    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Px30),
            22 => Some(Self::Px22),
            24 => Some(Self::Px24),
            27 => Some(Self::Px27),
            34 => Some(Self::Px34),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewSettings {
    pub orientation: Orientation,
    pub slim_progress_bar: bool,
    pub refresh_pages: u8,
    pub text_size: TextSize,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: true,
            refresh_pages: 5,
            text_size: TextSize::Px30,
        }
    }
}

/// What a page's layout depends on: the layout class and the text size. The
/// half turn, slim bar and refresh cadence change no page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageLayout {
    pub landscape: bool,
    pub text_size: TextSize,
}

impl PageLayout {
    pub const PORTRAIT: Self = Self { landscape: false, text_size: TextSize::Px30 };
    pub const LANDSCAPE: Self = Self { landscape: true, text_size: TextSize::Px30 };
}

pub fn valid_refresh_pages(value: u32) -> bool {
    matches!(value, 1 | 5 | 10)
}

/// Packed settings words (RTC record, resume intent): bit 0 is the layout
/// class, bit 4 the half turn, bit 1 the slim bar, bits 8..16 the refresh
/// cadence and bits 16..24 the text size byte. Bits 2 and 3 belong to each
/// record. Older builds reject bits 4 and 16..24 as reserved, so they never
/// misread a clockwise or resized word.
pub const PACKED_SETTINGS: u32 = 0xff_ff13;

impl ViewSettings {
    pub const fn page_layout(self) -> PageLayout {
        PageLayout { landscape: self.orientation.is_landscape(), text_size: self.text_size }
    }

    pub const fn packed(self) -> u32 {
        let orientation = match self.orientation {
            Orientation::Portrait => 0,
            Orientation::LandscapeCcw => 1,
            Orientation::LandscapeCw => 0x11,
        };
        orientation
            | ((self.slim_progress_bar as u32) << 1)
            | ((self.refresh_pages as u32) << 8)
            | ((self.text_size.byte() as u32) << 16)
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
        let text_size = TextSize::from_byte((word >> 16 & 0xff) as u8)?;
        valid_refresh_pages(refresh).then_some(Self {
            orientation,
            slim_progress_bar: word & 2 != 0,
            refresh_pages: refresh as u8,
            text_size,
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
        // Each record takes the lowest version that can hold it: v4 for a
        // size other than 30 px, v3 for clockwise, else v2. An older build
        // keeps reading what it understands and refuses the rest unread.
        let version = if self.settings.text_size != TextSize::Px30 {
            4
        } else if self.settings.orientation == Orientation::LandscapeCw {
            3
        } else {
            2
        };
        put32(&mut bytes, 4, version);
        put32(&mut bytes, 8, self.sequence);
        bytes[12] = self.settings.orientation as u8;
        bytes[13] = u8::from(self.settings.slim_progress_bar);
        bytes[14] = self.settings.refresh_pages;
        bytes[15] = self.settings.text_size.byte();
        let checksum = crc32(&bytes[..20]);
        put32(&mut bytes, 20, checksum);
        Some(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 8 || &bytes[..4] != b"NWTP" {
            return Err(DecodeError::Corrupt);
        }
        let version = get32(bytes, 4);
        if !(1..=4).contains(&version) {
            return Err(DecodeError::Unsupported);
        }
        let Some(orientation) = bytes.get(12).copied().and_then(Orientation::from_byte)
            .filter(|&o| o != Orientation::LandscapeCw || version >= 3)
        else {
            return Err(DecodeError::Corrupt);
        };
        let Some(text_size) = bytes.get(15).copied()
            .filter(|&byte| byte == 0 || version == 4)
            .and_then(TextSize::from_byte)
        else {
            return Err(DecodeError::Corrupt);
        };
        if bytes.len() != BYTES
            || bytes[13] > 1
            || get32(bytes, 20) != crc32(&bytes[..20])
            || (version == 1 && bytes[14] != 0)
            || (version > 1 && !valid_refresh_pages(u32::from(bytes[14])))
            || bytes[16..20].iter().any(|&byte| byte != 0)
        {
            return Err(DecodeError::Corrupt);
        }
        Ok(Self {
            sequence: get32(bytes, 8),
            settings: ViewSettings {
                orientation,
                slim_progress_bar: bytes[13] == 1,
                refresh_pages: if version == 1 { 5 } else { bytes[14] },
                text_size,
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
