// SPDX-License-Identifier: MIT
//! Sleep's raw destination, frost, stamp, extras and final night inversion.
//! The caller supplies the already-published Home presentation, not a restore.
use super::{DeviceInfo, Native, Resources, Status};
use newt_core::{
    app::{AppState, Screen},
    preferences::{Orientation, ViewSettings, geometry},
};
use newt_device_ui::{
    HeaderInfo, HomeData, HomePlane, HomeStyle, SleepExtras, frost_sleep_backdrop_in_place,
    render_home, render_sleep_extras, render_sleep_ui, sleep_inverted,
};
use newt_phased_reader::PhasedReader;
use newt_reader_ui::{ExternalOwner, ReaderFooter, ReaderPanelLoan};

/// A call-scoped snapshot from Home's published choices/data. Storage reads,
/// power transitions and the native grayscale upload remain outside painting.
pub struct Scene<'a> {
    pub state: &'a AppState,
    pub home: &'a HomeData<'a>,
    pub style: &'a HomeStyle<'a>,
    pub extras: &'a SleepExtras,
}

impl<N: Native> Resources<'_, '_, '_, '_, '_, N> {
    /// #285: repaint the complete raw destination after every parse and for
    /// every plane. Only then may the bank prefix and frame be lent together.
    pub(super) fn paint_sleep(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut impl newt_reader_ui::SourceOperations,
        owner: &mut ExternalOwner,
        scene: &Scene<'_>,
        info: &DeviceInfo<'_>,
        plane: HomePlane,
        now: u32,
    ) -> Result<(), Status> {
        let reading = scene.state.resume_screen == Screen::EpubReading;
        let reader_branch = reading && owner.state.opened();
        let ready = reader_branch && owner.paint_physical(reader, revision, now, &mut self.owner());
        let use_reader = reading && owner.state.opened();
        let settings = self.settings;
        let size = geometry(if use_reader {
            settings
        } else {
            ViewSettings {
                orientation: Orientation::Portrait,
                ..ViewSettings::default()
            }
        });
        // The short PhysicalOwner above has ended. Storage's exclusive borrow
        // excludes a live parse Phase; the loan omits the retained Home tail.
        let panel = &mut *self.panel;
        self.storage.with_frame_scratch(|scratch| {
            panel.with_frame(|frame| {
                if use_reader {
                    let page = owner.state.position(reader).page;
                    let section = reader.current_section();
                    let footer = ReaderFooter {
                        spine: section.spine,
                        page: page.page,
                        pages: page.page_count,
                        overall_known: page.overall_known,
                        overall_percent: page.overall_percent,
                        section_label: Some(section.label),
                    };
                    newt_reader_ui::render_reader_chrome(
                        &footer,
                        info,
                        settings,
                        ready,
                        Some(frame),
                    );
                    if !ready {
                        if let Some(failure) = owner.chapter_failure() {
                            newt_reader_ui::render_chapter_failure(
                                failure,
                                info,
                                settings,
                                Some(frame),
                            );
                        }
                    }
                } else {
                    let mut destination = *scene.state;
                    destination.screen = Screen::Home;
                    let header = HeaderInfo {
                        battery_known: info.battery_known,
                        battery_percent: info.battery_percent,
                    };
                    // Frozen paintBackdrop chooses the reader branch before repaint.
                    // Losing that page drops its custom backdrop for this call only.
                    let fallback = HomeStyle {
                        focus: scene.style.focus,
                        ..HomeStyle::default()
                    };
                    let style = if reader_branch {
                        &fallback
                    } else {
                        scene.style
                    };
                    render_home(
                        &destination,
                        &header,
                        scene.home,
                        HomePlane::Monochrome,
                        Some(frame),
                        style,
                        None,
                    );
                }
                frost_sleep_backdrop_in_place(frame, scratch, size, plane)
                    .map_err(|_| Status::OutOfMemory)?;
                let selector = plane != HomePlane::Monochrome;
                render_sleep_ui(info, Some(frame), size, selector);
                render_sleep_extras(scene.extras, scene.home, Some(frame), size, selector);
                if sleep_inverted(scene.extras) {
                    for byte in &mut *frame {
                        *byte = !*byte;
                    }
                }
                // ReaderPanel holds SDK polarity, as compose_native does.
                for byte in frame {
                    *byte = !*byte;
                }
                Ok(())
            })?
        })?
    }
}
