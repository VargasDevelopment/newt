// SPDX-License-Identifier: MIT
//! The actual card image and existing Core SlotInstaller, with short repaint
//! loans between SDK partition calls. Only its frozen uncertainty byte lives
//! for the boot; the three-sector allocation lives for one install.
#![forbid(unsafe_code)]
use crate::{app_image::Events, update_runtime::UpdateView, update_ui::ModeUi};
use newt_c3_board::{Error, image::ImagePartitions};
use newt_core::{frame::Frame, update_card::{CardImage, Yield, slot_installer::{
    InstallIssue, Ports, SlotInstaller, SECTOR_BYTES, identity::{OtaRecord, OTA_SECTOR_BYTES}, selection::OtaFlash}}};
use newt_device_ui::update::{self, UpdatePage};
use newt_native_io::UpdateFiles;
use newt_sdk_heap::Bytes;

pub struct Installer { selection_unconfirmed: bool }
impl Installer {
    pub const fn new() -> Self { Self { selection_unconfirmed: false } }

    pub fn install(&mut self, ui: &mut ModeUi<'_>, view: &mut UpdateView<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, radio_on: bool,
    ) -> Result<InstallIssue, Error> {
        ui.paint(view, Some(logical), radio_on)?;
        let index = update::focused_offer(view).ok_or(Error::Mode)?;
        let consent = update::update_consent(view, index).ok_or(Error::Mode)?;
        view.issue = InstallIssue::Unreadable;
        if let Some(mut sectors) = Bytes::try_new(3 * SECTOR_BYTES) {
            let (first, rest) = sectors.as_mut().split_at_mut(SECTOR_BYTES);
            let (second, window) = rest.split_at_mut(SECTOR_BYTES);
            let first = first.try_into().map_err(|_| Error::Length)?;
            let second = second.try_into().map_err(|_| Error::Length)?;
            let window = window.try_into().map_err(|_| Error::Length)?;
            view.issue = ui.with_parts(|board, shown| board.with_optional_sd_io(|board, io| {
                let mut files = UpdateFiles::new(io);
                let mut yielding = CardYield;
                let mut image = CardImage::new(&mut files, window, &mut yielding);
                board.with_image_partitions(true, |partitions| {
                    // Frozen Sha256 exists before CardImage::open, even if
                    // the file subsequently fails. No algorithm is copied.
                    partitions.with_hash(|partitions, hash| {
                        if !image.open(&view.offers[index]) { return InstallIssue::Unreadable; }
                        let mut progress = |board: &mut newt_c3_board::Board<'_>, done: u32, total: u32| {
                            let percent = (u64::from(done) * 100 / u64::from(total)) as u8;
                            if view.checking || percent / 20 != view.percent / 20 {
                                view.checking = false; view.percent = percent;
                                if shown.paint_installing(board, view.checking, view.percent,
                                    &mut view.password_on_panel, logical, radio_on).is_err() { crate::app_power::stop(); }
                            }
                            // Installing is explicitly nonprivate; its offer
                            // stays borrowed by Core throughout this callback.
                            if shown.poll_ready(board, false, Some(logical), events).is_err() { crate::app_power::stop(); }
                            crate::app_power::delay_ms(1);
                        };
                        let mut slots = Slots::new(partitions, &mut self.selection_unconfirmed, &mut progress);
                        SlotInstaller::new(&mut slots, &mut image, hash, first, second)
                            .install(&view.offers[index].manifest, Some(view.running), consent)
                    }).unwrap_or(InstallIssue::Unreadable)
                })
            }));
        }
        view.page = UpdatePage::Result; view.checking = false;
        ui.paint(view, Some(logical), radio_on)?;
        if view.issue == InstallIssue::None {
            ui.bus.board.with_optional_sd_io(|_, io| { let _ = crate::update_download::official_return::Store::new(io).remove(); });
        }
        Ok(view.issue)
    }
}
struct CardYield;
impl Yield for CardYield { fn yield_now(&mut self) { crate::app_power::delay_ms(1); } }

/// This is EspInstallSlots' concrete SDK adapter, not another installer.
/// Its validated otadata descriptor replaces the frozen nullable OtaFlash.
struct Slots<'a, 'op, 'sdk, P> {
    partitions: &'a mut ImagePartitions<'op, 'sdk>,
    selection: &'a mut bool,
    progress: &'a mut P,
    ota_usable: bool,
}
impl<'a, 'op, 'sdk, P> Slots<'a, 'op, 'sdk, P> {
    fn new(partitions: &'a mut ImagePartitions<'op, 'sdk>, selection: &'a mut bool, progress: &'a mut P) -> Self {
        let ota_usable = partitions.ota_bytes().is_ok_and(|size| size >= 2 * OTA_SECTOR_BYTES);
        Self { partitions, selection, progress, ota_usable }
    }
}
impl<P> OtaFlash for Slots<'_, '_, '_, P> {
    fn read(&mut self, at: u32, record: &mut OtaRecord) -> bool { self.ota_usable && self.partitions.read(at, record) }
    fn erase(&mut self, at: u32) -> bool { self.ota_usable && self.partitions.erase(at) }
    fn write(&mut self, at: u32, record: &OtaRecord) -> bool { self.ota_usable && self.partitions.write(at, record) }
}
impl<'sdk, P: FnMut(&mut newt_c3_board::Board<'sdk>, u32, u32)> Ports for Slots<'_, '_, 'sdk, P> {
    fn running(&mut self) -> i32 { self.partitions.running_slot() }
    fn running_pending(&mut self) -> bool {
        match self.partitions.board().running_image_state() {
            Ok(raw) => raw == 1,
            Err(Error::Native(0x105)) => false,
            Err(_) => true,
        }
    }
    fn selection_unconfirmed(&mut self) -> bool { *self.selection }
    fn mark_selection_unconfirmed(&mut self) { *self.selection = true; }
    fn slot_bytes(&mut self) -> u32 {
        match (self.partitions.app_bytes(0), self.partitions.app_bytes(1)) {
            (Ok(a), Ok(b)) if a == b => a,
            _ => 0,
        }
    }
    fn read_slot(&mut self, slot: i32, at: u32, out: &mut [u8]) -> bool {
        matches!(slot, 0 | 1) && self.partitions.read_app(slot as usize, u64::from(at), out).is_ok()
    }
    fn erase_slot(&mut self, slot: i32, at: u32, count: u32) -> bool {
        matches!(slot, 0 | 1) && self.partitions.erase_app(slot as usize, at, count).is_ok()
    }
    fn write_slot(&mut self, slot: i32, at: u32, bytes: &[u8]) -> bool {
        matches!(slot, 0 | 1) && self.partitions.write_app(slot as usize, at, bytes).is_ok()
    }
    fn ota(&mut self) -> &mut dyn OtaFlash { self }
    fn progress(&mut self, done: u32, total: u32) { (self.progress)(self.partitions.board(), done, total); }
}
const _: () = assert!(core::mem::size_of::<Installer>() == 1);
