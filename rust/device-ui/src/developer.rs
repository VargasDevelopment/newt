// SPDX-License-Identifier: MIT
//! The hidden Developer page, opened from the first Newt: one row, Test builds.
use newt_core::app::SettingsNotice;
use newt_core::update_online::TestBuilds;
use newt_system_ui::{SYSTEM_ROW_PITCH, SystemBar, SystemPage, clamped_list_buttons};
use newt_ui_canvas::Frame;

pub(crate) fn developer(frame: Option<&mut Frame>, bar: &SystemBar, test_builds: TestBuilds, notice: SettingsNotice) {
    let on = test_builds == TestBuilds::On;
    let mut page = SystemPage::new(
        frame,
        bar,
        clamped_list_buttons(Some(b"Back"), Some(b"Switch"), 0, 1),
        Some(b"Developer"),
    );
    let label: &[u8] = if on { b"Test builds: On" } else { b"Test builds: Off" };
    page.row(146, 0, Some(label), true);
    let note: &[u8] = match (notice == SettingsNotice::TestBuildsSaveFailed, on) {
        (true, true) => b"Could not confirm the save. Test builds stay On; after a restart they may change.",
        (true, false) => b"Could not confirm the save. Test builds stay Off; after a restart they may change.",
        _ => b"Update also offers test builds. They may be unstable.",
    };
    crate::para(page.canvas(), 50, 146 + SYSTEM_ROW_PITCH + 26, note, 21, 428, 430);
}
