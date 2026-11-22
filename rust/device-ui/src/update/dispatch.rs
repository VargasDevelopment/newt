// SPDX-License-Identifier: MIT
use super::*;
use newt_core::{app::Action, update_manifest::{UpdateKind, semver2_compare},
    update_card::slot_installer::InstallConsent, wifi_network_store::workflow::NativeWipe};

pub(super) fn firmware(offer: &UpdateOffer) -> bool { offer.manifest.kind == UpdateKind::Firmware }
pub(super) fn order<P, S>(view: &UpdateView<'_, P, S>, offer: &UpdateOffer) -> i32 {
    semver2_compare(CStr::from_bytes_until_nul(&offer.manifest.version).ok(), Some(view.running))
}
pub(super) fn present_count<P, S>(view: &UpdateView<'_, P, S>) -> u32 {
    view.offers.iter().filter(|offer| offer.present).count() as u32
}
pub(super) fn can_check<P, S>(view: &UpdateView<'_, P, S>) -> bool { view.saved.saved && view.card_ready }
/// With a card but no saved network, the check row sets up Wi-Fi instead of doing nothing.
pub(super) fn sets_up_wifi<P, S>(view: &UpdateView<'_, P, S>) -> bool { !view.saved.saved && view.card_ready }
pub(super) fn network_rows<P, S>(view: &UpdateView<'_, P, S>) -> u32 {
    1 + u32::from(view.network_count) + if view.saved.saved { 2 } else { 0 }
}
pub(super) fn forget_focused<P, S>(view: &UpdateView<'_, P, S>) -> bool {
    view.saved.saved && u32::from(view.network_focus) == network_rows(view) - 1
}
pub(super) fn connect_focused<P, S>(view: &UpdateView<'_, P, S>) -> bool {
    view.saved.saved && u32::from(view.network_focus) == network_rows(view) - 2
}
fn start_check<P, S>(view: &mut UpdateView<'_, P, S>) -> UpdateIntent {
    if !can_check(view) { return UpdateIntent::None; }
    view.page = UpdatePage::Online; view.online = OnlineCheck::Joining; view.percent = 0;
    UpdateIntent::Check
}
fn scan_networks<P, S>(view: &mut UpdateView<'_, P, S>) -> UpdateIntent {
    view.page = UpdatePage::Networks; view.scan = WifiScan::Scanning;
    view.network_count = 0; view.network_focus = 0;
    UpdateIntent::Scan
}
fn commit_network<P, S>(view: &mut UpdateView<'_, P, S>, forgetting: bool) -> UpdateIntent {
    view.page = UpdatePage::Network; view.forgetting = forgetting; view.committing = true;
    UpdateIntent::Network
}
fn dispatch_networks<P: Password, S>(view: &mut UpdateView<'_, P, S>, action: Action,
    wipe: &dyn NativeWipe,
) -> UpdateIntent {
    let rows = network_rows(view);
    if u32::from(view.network_focus) >= rows { view.network_focus = (rows - 1) as u8; }
    match action {
        Action::Back | Action::Sleep => return UpdateIntent::Exit,
        Action::Previous => { if view.network_focus == 0 { return UpdateIntent::None; }
            view.network_focus -= 1; return UpdateIntent::Repaint; }
        Action::Next => { if u32::from(view.network_focus) + 1 >= rows { return UpdateIntent::None; }
            view.network_focus += 1; return UpdateIntent::Repaint; }
        Action::Read => return UpdateIntent::None,
        Action::Activate => {}
    }
    if view.network_focus == 0 { return scan_networks(view); }
    if forget_focused(view) { return commit_network(view, true); }
    if connect_focused(view) { view.page = UpdatePage::Join; view.join = WifiJoin::Joining; return UpdateIntent::Join; }
    let Some(network) = view.networks.get(usize::from(view.network_focus - 1)) else { return UpdateIntent::None; };
    view.joining = *network;
    if view.joining.open { return commit_network(view, false); }
    view.page = UpdatePage::Password; view.password.clear(wipe);
    view.key_row = 0; view.key_column = 0; view.shifted = false; view.symbols = false;
    UpdateIntent::Repaint
}
fn dispatch_password<P: Password, S>(view: &mut UpdateView<'_, P, S>, action: Action, vertical: bool) -> UpdateIntent {
    match action {
        Action::Back => { view.page = UpdatePage::Networks; return UpdateIntent::Repaint; }
        Action::Sleep => return UpdateIntent::Exit,
        Action::Read => return UpdateIntent::None,
        Action::Previous | Action::Next => {
            (view.key_row, view.key_column) = keyboard::moved(view.symbols, view.shifted,
                view.key_row, view.key_column, action == Action::Next, vertical);
            return UpdateIntent::Repaint;
        }
        Action::Activate => {}
    }
    let (row, column) = keyboard::focus(view.symbols, view.shifted, view.key_row, view.key_column);
    let Some(key) = keyboard::key(view.symbols, view.shifted, row, column) else { return UpdateIntent::None; };
    let changed = match key.kind {
        keyboard::Kind::Text | keyboard::Kind::Space => view.password.append(&[key.byte]),
        keyboard::Kind::Delete => view.password.erase(),
        keyboard::Kind::Shift => { view.shifted = !view.shifted; true }
        keyboard::Kind::Mode => { view.symbols = !view.symbols; view.shifted = false; true }
        keyboard::Kind::Submit => return if view.password.size() >= PASSWORD_MINIMUM {
            commit_network(view, false)
        } else { UpdateIntent::None },
    };
    if changed { UpdateIntent::Repaint } else { UpdateIntent::None }
}
fn dispatch_page<P: Password, S>(view: &mut UpdateView<'_, P, S>, action: Action, vertical: bool,
    wipe: &dyn NativeWipe,
) -> UpdateIntent {
    match view.page {
        UpdatePage::Installing => return UpdateIntent::None,
        UpdatePage::Result => return if matches!(action, Action::Back | Action::Sleep) { UpdateIntent::Exit } else { UpdateIntent::None },
        UpdatePage::Networks => return dispatch_networks(view, action, wipe),
        UpdatePage::Password => return dispatch_password(view, action, vertical),
        UpdatePage::Join => {
            if view.join == WifiJoin::Joining { return UpdateIntent::None; }
            if action == Action::Back { view.page = UpdatePage::Networks; return UpdateIntent::Repaint; }
            return if action == Action::Sleep { UpdateIntent::Exit } else { UpdateIntent::None };
        }
        UpdatePage::Online => {
            if view.online.running() { return UpdateIntent::None; }
            if action == Action::Back {
                if view.quick_start { return UpdateIntent::Exit; }
                view.page = UpdatePage::List; view.focus = present_count(view) as u8; return UpdateIntent::Repaint;
            }
            return if action == Action::Sleep { UpdateIntent::Exit } else { UpdateIntent::None };
        }
        UpdatePage::Network => {
            if view.committing { return UpdateIntent::None; }
            if action == Action::Back { view.page = UpdatePage::Networks; return UpdateIntent::Repaint; }
            return if action == Action::Sleep { UpdateIntent::Exit } else { UpdateIntent::None };
        }
        UpdatePage::Confirm => {
            if action == Action::Back { view.page = UpdatePage::List; return UpdateIntent::Repaint; }
        }
        UpdatePage::List => {
            if action == Action::Back { return UpdateIntent::Exit; }
            if action == Action::Previous && view.focus > 0 { view.focus -= 1; return UpdateIntent::Repaint; }
            if action == Action::Next && u32::from(view.focus) + 1 < present_count(view) + 1 {
                view.focus += 1; return UpdateIntent::Repaint;
            }
            if action == Action::Activate && check_focused(view) {
                return if sets_up_wifi(view) { scan_networks(view) } else { start_check(view) };
            }
        }
    }
    if action == Action::Sleep { return UpdateIntent::Exit; }
    if action != Action::Activate { return UpdateIntent::None; }
    let choice = focused_offer(view).map_or(UpdateChoice::None, |index| update_choice(view, index));
    if view.page == UpdatePage::List && choice == UpdateChoice::Confirm {
        view.page = UpdatePage::Confirm; return UpdateIntent::Repaint;
    }
    if choice == UpdateChoice::None { return UpdateIntent::None; }
    view.page = UpdatePage::Installing; view.checking = true; view.percent = 0;
    UpdateIntent::Install
}
pub fn dispatch_update<P: Password, S>(view: &mut UpdateView<'_, P, S>, action: Action,
    vertical: bool, wipe: &dyn NativeWipe,
) -> UpdateIntent {
    let intent = dispatch_page(view, action, vertical, wipe);
    if intent == UpdateIntent::Exit || (view.page != UpdatePage::Password && intent != UpdateIntent::Network) {
        view.password.clear(wipe);
    }
    intent
}
pub fn open_networks<P, S>(view: &mut UpdateView<'_, P, S>) -> UpdateIntent { scan_networks(view) }
pub fn check_after_save<P, S>(view: &mut UpdateView<'_, P, S>) -> UpdateIntent {
    if !view.quick_start || view.forgetting || view.commit != WifiCommit::Saved { UpdateIntent::None }
    else { start_check(view) }
}
pub fn check_focused<P, S>(view: &UpdateView<'_, P, S>) -> bool { u32::from(view.focus) == present_count(view) }
pub fn focused_offer<P, S>(view: &UpdateView<'_, P, S>) -> Option<usize> {
    view.offers.iter().enumerate().filter(|(_, offer)| offer.present)
        .nth(usize::from(view.focus)).map(|(index, _)| index)
}
pub fn update_choice<P, S>(view: &UpdateView<'_, P, S>, index: usize) -> UpdateChoice {
    let Some(offer) = view.offers.get(index).filter(|offer| offer.present && offer.readable) else { return UpdateChoice::None; };
    if firmware(offer) { return UpdateChoice::Confirm; }
    let newer = order(view, offer);
    if newer < 0 { UpdateChoice::None }
    else if newer == 0 || view.target == SlotImage::Other { UpdateChoice::Confirm } else { UpdateChoice::Install }
}
pub fn update_consent<P, S>(view: &UpdateView<'_, P, S>, index: usize) -> Option<InstallConsent> {
    let offer = view.offers.get(index)?;
    Some(InstallConsent { confirmed: view.target == SlotImage::Other || firmware(offer),
        reinstall: !firmware(offer) && order(view, offer) == 0 })
}
pub fn add_wifi_network<P, S>(view: &mut UpdateView<'_, P, S>, name: Option<&[u8]>, rssi: i32, open: bool) {
    let Some(name) = name.map(crate::cstr).filter(|name| !name.is_empty()) else { return; };
    let name = &name[..name.len().min(NAME_BYTES)];
    let signal = rssi.clamp(-128, 0) as i8;
    let rows = &mut view.networks;
    let mut count = usize::from(view.network_count).min(NETWORK_ROWS);
    for i in 0..count {
        if crate::cstr(&rows[i].name) != name { continue; }
        if rows[i].rssi >= signal { return; }
        count -= 1;
        for at in i..count { rows[at] = rows[at + 1]; }
        break;
    }
    let mut at = count;
    while at > 0 && rows[at - 1].rssi < signal { at -= 1; }
    if at < NETWORK_ROWS {
        if count == NETWORK_ROWS { count -= 1; }
        for i in (at + 1..=count).rev() { rows[i] = rows[i - 1]; }
        rows[at] = WifiNetwork::default(); rows[at].name[..name.len()].copy_from_slice(name);
        rows[at].rssi = signal; rows[at].open = open; count += 1;
    }
    view.network_count = count as u8;
}
