// SPDX-License-Identifier: MIT
//! Pocket Pause state and NWPA durable preference records.
use crate::app::Action;
use crate::wire::{crc32, get32, put32};

pub const PROMPT_COUNT: u32 = 3;
pub const KNOWN_MASK: u32 = (1 << PROMPT_COUNT) - 1;
pub const PREFERENCES_BYTES: usize = 20;

/// Raw values preserve defensive handling of unknown prompt IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prompt(pub u8);
impl Prompt {
    pub const BREATH: Self = Self(0);
    pub const MOVE: Self = Self(1);
    pub const WATER: Self = Self(2);
    pub const NONE: Self = Self(255);
    pub const fn bit(self) -> u32 {
        if (self.0 as u32) < PROMPT_COUNT {
            1 << self.0
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum View {
    #[default]
    Experience,
    Settings,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    None,
    Home,
    Sleep,
    Save,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    pub view: View,
    pub enabled_mask: u32,
    pub draft_mask: u32,
    pub selected_prompt: Prompt,
    pub last_pick: Prompt,
    pub settings_focus: u32,
    pub save_unconfirmed: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            view: View::Experience,
            enabled_mask: KNOWN_MASK,
            draft_mask: KNOWN_MASK,
            selected_prompt: Prompt::NONE,
            last_pick: Prompt::NONE,
            settings_focus: 0,
            save_unconfirmed: false,
        }
    }
}

pub const fn valid_mask(mask: u32) -> bool {
    mask & !KNOWN_MASK == 0
}

pub fn choose(mask: u32, last: Prompt, random: u32) -> Prompt {
    if !valid_mask(mask) || mask == 0 {
        return Prompt::NONE;
    }
    let mut candidates = mask;
    if candidates & (candidates - 1) != 0 && last != Prompt::NONE {
        candidates &= !last.bit();
    }
    let mut choice = random % candidates.count_ones();
    for index in 0..PROMPT_COUNT {
        if candidates & (1 << index) != 0 {
            if choice == 0 {
                return Prompt(index as u8);
            }
            choice -= 1;
        }
    }
    Prompt::NONE
}

impl State {
    fn pick(&mut self, random: u32) {
        self.selected_prompt = choose(self.enabled_mask, self.last_pick, random);
        if self.selected_prompt != Prompt::NONE {
            self.last_pick = self.selected_prompt;
        }
    }

    pub fn open(&mut self, mask: u32, random: u32) -> bool {
        if !valid_mask(mask) {
            return false;
        }
        self.view = View::Experience;
        self.enabled_mask = mask;
        self.draft_mask = mask;
        self.settings_focus = 0;
        self.save_unconfirmed = false;
        self.pick(random);
        true
    }

    pub fn resume(&mut self, mask: u32, saved: Prompt, experience: bool) -> bool {
        if !valid_mask(mask) {
            return false;
        }
        *self = Self {
            enabled_mask: mask,
            draft_mask: mask,
            view: if experience {
                View::Experience
            } else {
                View::Settings
            },
            selected_prompt: if mask & saved.bit() != 0 {
                saved
            } else {
                Prompt::NONE
            },
            ..Self::default()
        };
        self.last_pick = self.selected_prompt;
        true
    }

    pub fn dispatch(&mut self, action: Action, random: u32) -> Intent {
        if action == Action::Sleep {
            return Intent::Sleep;
        }
        if action == Action::Back {
            if self.view == View::Experience {
                return Intent::Home;
            }
            self.view = View::Experience;
            self.draft_mask = self.enabled_mask;
            self.save_unconfirmed = false;
            return Intent::None;
        }
        if self.view == View::Experience {
            if matches!(action, Action::Next | Action::Previous) {
                self.pick(random);
            } else if action == Action::Activate {
                self.view = View::Settings;
                self.draft_mask = self.enabled_mask;
                self.settings_focus = 0;
                self.save_unconfirmed = false;
            }
            return Intent::None;
        }
        let rows = PROMPT_COUNT + 1;
        if self.settings_focus >= rows {
            self.settings_focus = 0;
        }
        match action {
            Action::Next => self.settings_focus = (self.settings_focus + 1) % rows,
            Action::Previous => self.settings_focus = (self.settings_focus + rows - 1) % rows,
            Action::Activate => {
                if self.settings_focus == PROMPT_COUNT {
                    return Intent::Save;
                }
                self.draft_mask ^= 1 << self.settings_focus;
                self.save_unconfirmed = false;
            }
            _ => {}
        }
        Intent::None
    }

    pub fn can_pick_other(&self) -> bool {
        self.view == View::Experience && self.enabled_mask & !self.selected_prompt.bit() != 0
    }

    pub fn finish_save(&mut self, confirmed: bool, random: u32) {
        if self.view != View::Settings {
            return;
        }
        if !confirmed || !valid_mask(self.draft_mask) {
            self.save_unconfirmed = true;
            return;
        }
        self.enabled_mask = self.draft_mask;
        self.view = View::Experience;
        self.save_unconfirmed = false;
        if self.selected_prompt == Prompt::NONE
            || self.enabled_mask & self.selected_prompt.bit() == 0
        {
            self.pick(random);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub sequence: u32,
    pub enabled_mask: u32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            sequence: 0,
            enabled_mask: KNOWN_MASK,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Corrupt,
    Unsupported,
}

impl Preferences {
    pub fn encode_into(self, bytes: &mut [u8]) -> bool {
        if bytes.len() != PREFERENCES_BYTES || !valid_mask(self.enabled_mask) {
            return false;
        }
        bytes[..4].copy_from_slice(b"NWPA");
        put32(bytes, 4, 1);
        put32(bytes, 8, self.sequence);
        put32(bytes, 12, self.enabled_mask);
        let checksum = crc32(&bytes[..16]);
        put32(bytes, 16, checksum);
        true
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 8 || &bytes[..4] != b"NWPA" {
            return Err(DecodeError::Corrupt);
        }
        if get32(bytes, 4) != 1 {
            return Err(DecodeError::Unsupported);
        }
        if bytes.len() != PREFERENCES_BYTES
            || !valid_mask(get32(bytes, 12))
            || get32(bytes, 16) != crc32(&bytes[..16])
        {
            return Err(DecodeError::Corrupt);
        }
        Ok(Self {
            sequence: get32(bytes, 8),
            enabled_mask: get32(bytes, 12),
        })
    }
}
