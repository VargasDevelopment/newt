// SPDX-License-Identifier: MIT
//! Frozen src/update_status_pages.cpp. Result text never implies a failed
//! acknowledgement left storage unchanged unless the actual Core issue does.
use super::*;
use crate::Buffer;
use core::fmt::Write;
use newt_system_ui::*;
use newt_ui_canvas::{Frame, UiCanvas};

fn result_text(issue: InstallIssue, other: bool) -> &'static [u8] {
    match issue {
        InstallIssue::None => "Installed. Restarting…",
        InstallIssue::Unreadable => "Nothing was changed. The card could not be read.",
        InstallIssue::Size => if other { "Nothing was changed. The file is too large for this X3, or it changed after Newt listed it." }
            else { "Nothing was changed. The file's size does not match its manifest." },
        InstallIssue::Digest => "Nothing was changed. The file does not match its manifest.",
        InstallIssue::Image => "Nothing was changed. The file is not a complete X3 app.",
        InstallIssue::Identity => if other { "Nothing was changed. The file changed after Newt listed it." }
            else { "Nothing was changed. The file is not the version its manifest names." },
        InstallIssue::Older => "Nothing was changed. Use USB to install an older version.",
        InstallIssue::CheckAgain => "Nothing was changed. Check again for the official release.",
        InstallIssue::Same => "Nothing was changed. This version is already installed.",
        InstallIssue::TargetUnreadable => "Nothing was changed. Newt could not read the other app slot.",
        InstallIssue::Unsettled => "Nothing was installed. Newt could not confirm which app starts next. After a restart, this Newt or the app in the other slot may start.",
        InstallIssue::Consent => "Nothing was changed.",
        InstallIssue::Write => "The install did not finish. This Newt still starts.",
        InstallIssue::Readback => "The installed copy did not match the file. This Newt still starts.",
        InstallIssue::Selection => if other { "The new firmware could not be selected. This Newt still starts." }
            else { "The new version could not be selected. This Newt still starts." },
        InstallIssue::Unconfirmed => if other { "Newt could not confirm the install. After a restart, Newt or the new firmware may start." }
            else { "Newt could not confirm the install. After a restart, either version may start." },
    }.as_bytes()
}
fn progress_bar(c: &mut UiCanvas<'_>, percent: u32) {
    let p = update_bar(percent);
    c.outline(p.left, p.top, p.width, p.height);
    if p.filled > 0 { c.rect(p.left, p.top, p.filled, p.height, true); }
    c.paragraph(SYSTEM_CONTENT_LEFT, 262, Some(b"Do not turn off the X3 or remove the card."), 21, SYSTEM_CONTENT_WIDTH, 360);
}
fn network_result<P, S>(view: &UpdateView<'_, P, S>) -> &'static [u8] {
    if view.committing { return if view.forgetting { b"Forgetting..." } else { b"Saving..." }; }
    match view.commit {
        WifiCommit::Saved => if view.forgetting { b"Network forgotten." } else { b"Network saved." },
        WifiCommit::NotSaved => b"Not saved. Nothing was changed.",
        WifiCommit::Unconfirmed => b"Not confirmed. After restart the X3 may use the old or the new setting.",
    }
}
fn join_result(join: WifiJoin) -> &'static [u8] {
    match join {
        WifiJoin::Joining => b"Connecting...", WifiJoin::Connected => b"Connected. Wi-Fi is off again.",
        WifiJoin::WrongPassword => b"Wrong password. Pick the network to type it again.",
        WifiJoin::NotFound => b"Network not found.", WifiJoin::NoAddress => b"Connected, but the router gave no address.",
        WifiJoin::TimedOut => b"No answer in time. Try again.", WifiJoin::Failed => b"Newt could not connect.",
    }
}
/// A call-stack view of only this page's existing fields. Join may keep the
/// password borrowed while its addressed callback paints these other fields.
pub struct OnlineStatus<'a> {
    pub online: OnlineCheck,
    pub percent: u8,
    pub running: &'a core::ffi::CStr,
    #[cfg(feature = "dev-usb")]
    pub card_failure: &'a CardFailure,
}
fn online_text<'a>(view: &OnlineStatus<'_>, line: &'a mut [u8; 96]) -> &'a [u8] {
    match view.online {
        OnlineCheck::Joining => "Connecting to Wi-Fi…".as_bytes(),
        OnlineCheck::Looking => "Looking for an update on GitHub…".as_bytes(),
        OnlineCheck::Downloading => "Downloading…".as_bytes(),
        OnlineCheck::CheckAgain => b"Check again. The return to the official release was not confirmed on the card.",
        OnlineCheck::ReturnIncompatible => b"The official release is too old for saved data. Keep this version.",
        OnlineCheck::Ready => b"Downloaded to the card.",
        OnlineCheck::Latest => {
            let prefix = b"No newer Newt on GitHub. This X3 has ";
            let mut used = 0;
            for part in [prefix.as_slice(), view.running.to_bytes(), b"."] {
                let n = part.len().min(line.len() - 1 - used);
                line[used..used + n].copy_from_slice(&part[..n]); used += n;
            }
            line[used] = 0; line
        }
        OnlineCheck::NoNetwork => b"No Wi-Fi network is saved. Save one in Settings, Wi-Fi.",
        OnlineCheck::NoJoin => b"Newt could not connect to the saved Wi-Fi network.",
        OnlineCheck::Unreachable => b"Newt could not reach GitHub. Try again later.",
        OnlineCheck::NoRelease => b"GitHub has no Newt update to download.",
        OnlineCheck::WrongFile => b"The update on GitHub is not a valid Newt file. Nothing was installed.",
        OnlineCheck::Interrupted => b"The download did not finish. Nothing was installed.",
        OnlineCheck::CardFailed => b"Newt could not save the update to the card. Nothing was installed.",
    }
}
#[cfg(feature = "dev-usb")]
pub fn card_step_line(failure: &CardFailure, line: &mut [u8; 64]) -> bool {
    let name = match failure.step {
        CardStep::None => return false, CardStep::MakeFolder => "make folder", CardStep::RemoveManifest => "remove manifest",
        CardStep::OpenImage => "open image", CardStep::WriteImage => "write image", CardStep::SaveImage => "save image",
        CardStep::CloseImage => "close image", CardStep::OpenManifest => "open manifest", CardStep::WriteManifest => "write manifest",
        CardStep::SaveManifest => "save manifest", CardStep::CloseManifest => "close manifest",
    };
    let _ = write!(Buffer::new(line), "Card step: {} (error {}) at {}; tries {}", name, failure.error, failure.at, failure.tries);
    true
}
pub fn render_online(view: &OnlineStatus<'_>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let mut page = SystemPage::new(frame, bar, SystemButtons { back: (!view.online.running()).then_some(b"Back"),
        ..SystemButtons::default() }, Some(b"Update"));
    let c = page.canvas(); let mut line = [0; 96]; let text = online_text(view, &mut line);
    if view.online != OnlineCheck::Downloading {
        c.paragraph(SYSTEM_CONTENT_LEFT, 160, Some(text), 24, SYSTEM_CONTENT_WIDTH, 400);
        #[cfg(feature = "dev-usb")]
        {
            let mut step = [0; 64];
            if view.online == OnlineCheck::CardFailed && card_step_line(&view.card_failure, &mut step) {
                c.fixed_complete_paragraph(SYSTEM_CONTENT_LEFT,
                    160 + newt_ui_canvas::paragraph_rows(Some(text), 24, SYSTEM_CONTENT_WIDTH) * 33 + 16,
                    Some(&step), 21, SYSTEM_CONTENT_WIDTH, 400);
            }
        }
    } else {
        c.text(SYSTEM_CONTENT_LEFT, 160, Some(text), 24, true, SYSTEM_CONTENT_WIDTH, true);
        progress_bar(c, u32::from(view.percent));
    }
}
pub(super) fn render<P, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    if view.page == UpdatePage::Online {
        render_online(&OnlineStatus { online: view.online, percent: view.percent, running: view.running,
            #[cfg(feature = "dev-usb")] card_failure: &view.card_failure }, bar, frame);
        return;
    }
    if view.page == UpdatePage::Installing { render_installing(view.checking, view.percent, bar, frame); return; }
    let (busy, title) = match view.page {
        UpdatePage::Network => (view.committing, b"Wi-Fi".as_slice()),
        UpdatePage::Join => (view.join == WifiJoin::Joining, b"Wi-Fi".as_slice()),
        UpdatePage::Result => (view.issue == InstallIssue::None, b"Update".as_slice()),
        _ => return,
    };
    let mut page = SystemPage::new(frame, bar, SystemButtons { back: (!busy).then_some(b"Back"),
        ..SystemButtons::default() }, Some(title));
    let c = page.canvas();
    match view.page {
        UpdatePage::Network => { c.paragraph(SYSTEM_CONTENT_LEFT, 160, Some(network_result(view)), 24, SYSTEM_CONTENT_WIDTH, 400); }
        UpdatePage::Join => { c.paragraph(SYSTEM_CONTENT_LEFT, 160, Some(join_result(view.join)), 24, SYSTEM_CONTENT_WIDTH, 400); }
        UpdatePage::Result => {
            let other = focused_offer(view).is_some_and(|index| dispatch::firmware(&view.offers[index]));
            c.paragraph(SYSTEM_CONTENT_LEFT, 160, Some(result_text(view.issue, other)), 24, SYSTEM_CONTENT_WIDTH, 400);
        }
        _ => {}
    }
}

/// The same page body also serves partition progress while Core borrows the
/// focused manifest. No offer, manifest or secret is copied for this paint.
pub fn render_installing(checking: bool, percent: u8, bar: &SystemBar, frame: Option<&mut Frame>) {
    let mut page = SystemPage::new(frame, bar, SystemButtons::default(), Some(b"Installing"));
    let c = page.canvas();
    let mut line = [0; 48];
    let _ = if checking { write!(Buffer::new(&mut line), "Checking the file…") }
        else { write!(Buffer::new(&mut line), "Writing… {}%", percent) };
    c.text(SYSTEM_CONTENT_LEFT, 160, Some(&line), 24, true, SYSTEM_CONTENT_WIDTH, true);
    progress_bar(c, if checking { 0 } else { u32::from(percent) });
}
