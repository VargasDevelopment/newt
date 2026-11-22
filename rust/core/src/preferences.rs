//! Reader view settings and NWTP v1/v2 compatibility.
use crate::wire::{crc32, get32, put32};

pub const BYTES: usize = 24;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Orientation {
    Portrait = 0,
    #[default]
    LandscapeCcw = 1,
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
        put32(&mut bytes, 4, 2);
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
        if version != 1 && version != 2 {
            return Err(DecodeError::Unsupported);
        }
        if bytes.len() != BYTES
            || bytes[12] > 1
            || bytes[13] > 1
            || get32(bytes, 20) != crc32(&bytes[..20])
            || (version == 1 && bytes[14] != 0)
            || (version == 2 && !valid_refresh_pages(u32::from(bytes[14])))
            || bytes[15..20].iter().any(|&byte| byte != 0)
        {
            return Err(DecodeError::Corrupt);
        }
        Ok(Self {
            sequence: get32(bytes, 8),
            settings: ViewSettings {
                orientation: if bytes[12] == 0 {
                    Orientation::Portrait
                } else {
                    Orientation::LandscapeCcw
                },
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
    let (width, height) = match settings.orientation {
        Orientation::Portrait => (crate::frame::WIDTH as u16, crate::frame::HEIGHT as u16),
        Orientation::LandscapeCcw => (crate::frame::HEIGHT as u16, crate::frame::WIDTH as u16),
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
