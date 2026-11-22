// SPDX-License-Identifier: MIT
//! Sealed #272 main.cpp:437-442 / device_settings.h. Borrow existing Home
//! owners for one Settings press; no platform authority, state or frame held.
#![forbid(unsafe_code)]
use crate::{home_startup::{CardPort, HomeChoice, HomeRuntime}, nvs};
use core::fmt::Write;
use newt_core::{app::{self, Action, AppState, DevicePage, Screen, SettingsNotice, SettingsRow}, clock_format::ClockFormat, durable::Commit, time_zone::{TimeZone,TIME_ZONE_LABELS}, update_online::TestBuilds};
use newt_system_ui::{ListOverlay,ListOverlayResult,dispatch_list_overlay};

/// Required real Root operations; no success defaults. One borrowed capability
/// lets its own reader/platform aliases be lent sequentially, never duplicated.
pub trait Platform<'sdk,L,N,P> {
    fn firmware_switch(&mut self,home:&mut HomeRuntime<'sdk,L,N,P>,app:&mut AppState,action:Action)->bool;
    fn update_entry(&mut self,home:&mut HomeRuntime<'sdk,L,N,P>,app:&mut AppState,action:Action)->bool;
}

/// Run before RestApp::dispatch; its `settings_changed` receives this result.
/// Library/reader bypass the entire Settings chain. QuickStart's own branch
/// runs before this one; Sleep still falls through to the existing sleep owner.
pub fn dispatch<'sdk,L:Write,N:nvs::Native,P:CardPort>(
    home:&mut HomeRuntime<'sdk,L,N,P>,app:&mut AppState,action:Action,
    platform:&mut dyn Platform<'sdk,L,N,P>,
)->bool {
    if matches!(app.screen,Screen::Library|Screen::EpubReading|Screen::QuickStart) {return false;}
    if zone(home,app,action) {return true;}
    if format(home,app,action) {return true;}
    if sleep(home,app,action) {return true;}
    if test_builds(home,app,action) {return true;}
    if platform.firmware_switch(home,app,action) {return true;}
    platform.update_entry(home,app,action)
}
fn zone<L:Write,N:nvs::Native,P:CardPort>(home:&mut HomeRuntime<'_,L,N,P>,app:&mut AppState,action:Action)->bool {
    if app.screen!=Screen::Device || app::device_page(app)!=DevicePage::Settings {return false;}
    if !app.zone_list_open {
        if action!=Action::Activate || app.settings_row!=SettingsRow::TimeZone {return false;}
        app.zone_list_open=true;app.list_focus=home.clock.zone() as u8;return true;
    }
    let labels=TIME_ZONE_LABELS.map(|label|Some(label.as_bytes()));
    let Some(list)=ListOverlay::new(Some(b"Time zone"),&labels) else {return false;};
    match dispatch_list_overlay(&list,&mut app.list_focus,action) {
        ListOverlayResult::Unhandled|ListOverlayResult::Clamped=>return false,
        ListOverlayResult::Moved=>return true,
        ListOverlayResult::Cancelled=>{app.zone_list_open=false;return true;}
        ListOverlayResult::Picked=>{}
    }
    app.zone_list_open=false;
    app.settings_notice=if home.commit(HomeChoice::Zone(TimeZone::from_raw_or_utc(app.list_focus)))==Commit::Saved {SettingsNotice::Saved}else{SettingsNotice::SaveFailed};true
}
fn format<L:Write,N:nvs::Native,P:CardPort>(home:&mut HomeRuntime<'_,L,N,P>,app:&mut AppState,action:Action)->bool {
    if app.screen!=Screen::Device || app::device_page(app)!=DevicePage::Settings || app.zone_list_open || action!=Action::Activate || app.settings_row!=SettingsRow::TimeFormat {return false;}
    let next=if home.clock.format()==ClockFormat::TwelveHour {ClockFormat::TwentyFourHour}else{ClockFormat::TwelveHour};
    app.settings_notice=if home.commit(HomeChoice::Format(next))==Commit::Saved {SettingsNotice::FormatSaved}else{SettingsNotice::FormatSaveFailed};true
}
fn sleep<L:Write,N:nvs::Native,P:CardPort>(home:&mut HomeRuntime<'_,L,N,P>,app:&mut AppState,action:Action)->bool {
    if app.screen!=Screen::Device || app::device_page(app)!=DevicePage::SleepScreen || action!=Action::Activate {return false;}
    let mut next=home.clock.sleep_screen();
    match app.list_focus {0=>next.progress=!next.progress,1=>next.week=!next.week,_=>next.habit=!next.habit}
    app.settings_notice=if home.commit(HomeChoice::SleepScreen(next))==Commit::Saved {SettingsNotice::None}else{match app.list_focus {0=>SettingsNotice::SleepProgressSaveFailed,1=>SettingsNotice::SleepWeekSaveFailed,_=>SettingsNotice::SleepHabitSaveFailed}};true
}
fn test_builds<L:Write,N:nvs::Native,P:CardPort>(home:&mut HomeRuntime<'_,L,N,P>,app:&mut AppState,action:Action)->bool {
    if app.screen!=Screen::Device || app::device_page(app)!=DevicePage::Developer || action!=Action::Activate {return false;}
    let next=if home.clock.test_builds()==TestBuilds::On {TestBuilds::Off}else{TestBuilds::On};
    app.settings_notice=if home.commit(HomeChoice::TestBuilds(next))==Commit::Saved {SettingsNotice::None}else{SettingsNotice::TestBuildsSaveFailed};true
}
