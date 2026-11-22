// SPDX-License-Identifier: MIT
//! The frozen HomeRefreshPolicy input key; no pixels or source ownership.
use newt_core::app::{AppState, home_section};
use newt_stage_home::{HeaderInfo, HomeData, ESTIMATE};
use core::ffi::CStr;

pub struct HomeRefresh {
    clock: [u8; 32],
    identity: [u8; 129],
    description: [u8; 31],
    selected: u8,
    flags: u8,
    battery: i32,
    percent: i32,
    available: bool,
    cover: bool,
    valid: bool,
    progress_unavailable: bool,
}
impl Default for HomeRefresh {
    fn default() -> Self {
        Self { clock: [0; 32], identity: [0; 129], description: [0; 31],
            selected: 0, flags: 0, battery: -1, percent: -1, available: false,
            cover: false, valid: false, progress_unavailable: false }
    }
}
fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len())]
}
fn same(saved: &[u8], value: &[u8]) -> bool {
    let value = text(value);
    value.len() < saved.len() && text(saved) == value
}
fn copy(saved: &mut [u8], value: &[u8]) -> bool {
    let value = text(value);
    if value.len() >= saved.len() { return false; }
    saved[..value.len()].copy_from_slice(value);
    saved[value.len()] = 0;
    true
}
fn battery(info: &HeaderInfo) -> i32 {
    if info.battery_known && info.battery_percent <= 100 { i32::from(info.battery_percent) } else { -1 }
}
impl HomeRefresh {
    pub fn invalidate(&mut self) { self.valid = false; }
    pub fn selected(&self) -> u8 { self.selected }
    pub fn needs_paint(&self, app: &AppState, info: &HeaderInfo, data: &HomeData<'_>, clock: Option<&CStr>, force: bool) -> bool {
        force || !self.valid || self.selected != home_section(app.selected) as u8
            || !self.same_metadata(info, data, clock)
    }
    pub fn focus_only(&self, app: &AppState, info: &HeaderInfo, data: &HomeData<'_>, clock: Option<&CStr>) -> bool {
        self.valid && self.selected != home_section(app.selected) as u8 && self.same_metadata(info, data, clock)
    }
    fn same_metadata(&self, info: &HeaderInfo, data: &HomeData<'_>, clock: Option<&CStr>) -> bool {
        let title = text(data.book_title.unwrap_or(b"No current book"));
        let chapter = text(&data.chapter_title);
        let chapter_at = title.len() + 1;
        chapter_at <= self.identity.len() && chapter.len() < self.identity.len() - chapter_at
            && same(&self.identity[..chapter_at], title)
            && same(&self.identity[chapter_at..], chapter)
            && self.battery == battery(info) && self.percent == data.percent
            && self.available == data.reading_available && self.progress_unavailable == data.progress_unavailable
            && self.cover == data.fixture_cover && self.flags == data.flags & !ESTIMATE
            && same(&self.clock, clock.map_or(b"--:--".as_slice(), |clock| clock.to_bytes()))
            && same(&self.description, data.device_description)
    }
    /// Publish only after the complete blocking display operation succeeds.
    pub fn remember(&mut self, app: &AppState, info: &HeaderInfo, data: &HomeData<'_>) {
        self.valid = false;
        let title = text(data.book_title.unwrap_or(b"No current book"));
        let chapter = text(&data.chapter_title);
        let chapter_at = title.len() + 1;
        if chapter_at > self.identity.len() || chapter.len() >= self.identity.len() - chapter_at
            || !copy(&mut self.clock, data.clock.map_or(b"--:--".as_slice(), |clock| clock.to_bytes()))
            || !copy(&mut self.description, data.device_description) { return; }
        copy(&mut self.identity[..chapter_at], title);
        copy(&mut self.identity[chapter_at..], chapter);
        self.selected = home_section(app.selected) as u8;
        self.battery = battery(info);
        self.percent = data.percent;
        self.available = data.reading_available;
        self.progress_unavailable = data.progress_unavailable;
        self.cover = data.fixture_cover;
        self.flags = data.flags & !ESTIMATE;
        self.valid = true;
    }
}
