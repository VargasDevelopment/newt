// SPDX-License-Identifier: MIT
//! One short view of the SAME app publication, also lent during restart drain.
#![forbid(unsafe_code)]
use crate::{app_dev::Put, app_image::{Application, Events, Home, Reader}, dev_usb::{Bridge, Diagnostics, PanelRing, Status}};
use super::display::DisplayReceipt;
use newt_c3_board::hardware::{HardwareError, HardwareReader};
use newt_core::{app::Screen, frame, preferences::Orientation, update_confirm::{self, FirstScreen}};

/// Actual scalar observations captured before the exclusive dispatch loan.
/// There is no app/catalog/controller owner here and no retained view.
#[derive(Clone, Copy)]
pub struct Facts {
    screen: Screen,
    library_count: u32,
    library_selected: u32,
    first_screen: FirstScreen,
    fingerprint: u64,
}
impl Facts {
    pub fn current(ui: &Application<'_>) -> Self {
        let count = if ui.app.screen == Screen::Library { ui.catalog.as_ref().map_or(0, |catalog| catalog.size() as u32) } else { 0 };
        Self { screen: ui.app.screen, library_count: count,
            library_selected: if count == 0 { 0 } else { ui.library.selected.min(count as usize - 1) as u32 },
            first_screen: ui.first_screen, fingerprint: ui.device_fingerprint }
    }
}
pub struct Published<'a, 'sdk> {
    pub facts: Facts,
    pub displayed: &'a DisplayReceipt,
    pub hardware: &'a mut HardwareReader<'sdk>,
    pub bridge: &'a mut Bridge,
    pub panels: &'a PanelRing,
    /// False while draining for a restart, when no card write may begin.
    pub writes: bool,
}
impl Published<'_, '_> {
    pub fn poll(&mut self, home: &mut Home<'_>, reader: &impl Reader, logical: &newt_core::frame::Frame,
        events: &mut Events<'_, '_>,
    ) -> Result<(), HardwareError> {
        events.task.gate().with_pending(|pending| self.bridge.drain_pending(pending));
        let facts = reader.facts();
        let free_heap = self.hardware.free_heap_bytes()?;
        let largest = self.hardware.largest_heap_block_bytes()?;
        let orientation = if self.displayed.valid { self.displayed.orientation } else { Orientation::Portrait };
        let landscape = orientation.is_landscape();
        let status = Status { generation: self.displayed.generation, refresh_ms: self.displayed.refresh_ms,
            free_heap, dropped: events.control.dropped(), screen: self.facts.screen as u32,
            width: (if landscape { frame::HEIGHT } else { frame::WIDTH }) as u32,
            height: (if landscape { frame::WIDTH } else { frame::HEIGHT }) as u32,
            ready: self.displayed.valid, orientation, monochrome: !self.displayed.grayscale };
        let mut diagnostics = Diagnostics::default();
        reader.diagnostics(&mut diagnostics);
        diagnostics.reader_status = facts.status;
        diagnostics.reader_open = facts.opened;
        diagnostics.progress_ready = facts.progress_ready;
        diagnostics.actual_spine = facts.spine;
        diagnostics.actual_character = facts.character;
        diagnostics.current_book = facts.book;
        diagnostics.free_heap = free_heap;
        diagnostics.largest_block = largest;
        diagnostics.opening = events.control.active();
        diagnostics.library_count = self.facts.library_count;
        diagnostics.library_selected = self.facts.library_selected;
        diagnostics.device_fingerprint = self.facts.fingerprint;
        // An owed reopen is card work about to start, as an open in progress.
        let put = if self.writes { Put::Admits(newt_core::dev_put::Facts { home: self.facts.screen == Screen::Home,
            reader_open: facts.opened, opening: diagnostics.opening || facts.reopen_owed }) } else { Put::Unavailable };
        let clock = home.bus.board.monotonic_clock();
        let mut channel = home.bus.log.dev_channel(clock);
        let mut publication = crate::app_dev::Publication { status, frame: facts.frame(self.facts.screen, logical), queue: events.queue,
            panels: self.panels, diagnostics, put };
        crate::app_dev::poll(self.bridge, &mut channel, &mut publication, crate::app_dev::BUILD_ID,
            update_confirm::image_confirmed(self.facts.first_screen));
        if let Put::Granted(session) = publication.put {
            super::card_write::lease(self.bridge, home, events, session);
        }
        Ok(())
    }
}
