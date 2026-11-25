use newt_core::app;
use app::*;
use std::io::{self, Read, Write};

fn screen(value: u32) -> Option<Screen> {
    [Screen::Home, Screen::Reading, Screen::Device, Screen::Sleeping,
        Screen::EpubReading, Screen::TinyApp, Screen::Library, Screen::QuickStart].get(value as usize).copied()
}
fn action(value: u32) -> Option<Action> {
    [Action::Previous, Action::Next, Action::Activate, Action::Back, Action::Sleep, Action::Read]
        .get(value as usize).copied()
}
fn boolean(value: u32) -> Option<bool> {
    match value { 0 => Some(false), 1 => Some(true), _ => None }
}
fn state(a: &[u32]) -> Option<AppState> {
    Some(AppState {
        // Alpha.16's AppState stores these two wire integers as uint8_t.
        screen: screen(a[0])?, resume_screen: screen(a[1])?,
        selected: u32::from(a[2] as u8), page: u32::from(a[3] as u8),
        about_presses: a[4].try_into().ok()?,
        about: [AboutView::Closed, AboutView::Version, AboutView::WhatsNewRow,
            AboutView::WhatsNew, AboutView::Updated].get(a[5] as usize).copied()?,
        first_newt: boolean(a[6])?, unlock_presses: 0, developer: false, holiday_outfits: false, row_open: boolean(a[7])?,
        settings_row: [SettingsRow::TimeZone, SettingsRow::TimeFormat, SettingsRow::SleepScreen,
            SettingsRow::Diagnostics, SettingsRow::ReadingStats, SettingsRow::FirmwareSwitch,
            SettingsRow::Update, SettingsRow::Wifi, SettingsRow::About, SettingsRow::QuickStart].get(a[8] as usize).copied()?,
        zone_list_open: boolean(a[9])?, list_focus: a[10].try_into().ok()?,
        settings_notice: [SettingsNotice::None, SettingsNotice::Saved, SettingsNotice::SaveFailed,
            SettingsNotice::FormatSaved, SettingsNotice::FormatSaveFailed, SettingsNotice::UpdateNotStarted,
            SettingsNotice::WifiNotStarted, SettingsNotice::SleepProgressSaveFailed,
            SettingsNotice::SleepWeekSaveFailed, SettingsNotice::SleepHabitSaveFailed, SettingsNotice::QuickStartSaveFailed].get(a[11] as usize).copied()?,
    })
}
enum Command { Dispatch(AppState, Action), Wake(AppState), Home(u32), Graph(u8, Action) }
fn parse(line: &str) -> Option<Command> {
    let t: Vec<_> = line.split(' ').collect();
    if t.iter().any(|x| x.is_empty()) { return None; }
    let a: Vec<u32> = t[1..].iter().map(|word| {
        if !word.bytes().all(|x| x.is_ascii_digit()) { return None; }
        let value = word.parse::<u32>().ok()?;
        (value.to_string() == *word).then_some(value)
    }).collect::<Option<_>>()?;
    match (t[0], a.len()) {
        ("D", 13) => Some(Command::Dispatch(state(&a)?, action(a[12])?)),
        ("W", 12) => Some(Command::Wake(state(&a)?)),
        ("H", 1) => Some(Command::Home(a[0])),
        ("G", 2) => Some(Command::Graph(a[0].try_into().ok()?, action(a[1])?)),
        _ => None,
    }
}
fn fields(s: &AppState) -> String {
    format!("{} {} {} {} {} {} {} {} {} {} {} {} {}", s.screen as u8,
        s.resume_screen as u8, s.selected, s.page, s.about_presses, s.about as u8,
        u8::from(s.first_newt), u8::from(s.row_open), s.settings_row as u8,
        u8::from(s.zone_list_open), s.list_focus, s.settings_notice as u8, device_page(s) as u8)
}
fn run() -> Option<()> {
    let mut input = String::new();
    io::stdin().take(2_100_001).read_to_string(&mut input).ok()?;
    if input.len() > 2_100_000 || !input.ends_with('\n') || input.contains('\r') { return None; }
    let mut rows = input[..input.len()-1].split('\n');
    if rows.next()? != "AP1" { return None; }
    // Admit the entire batch before dispatching or publishing any result.
    let commands: Vec<_> = rows.map(parse).collect::<Option<_>>()?;
    if commands.is_empty() || commands.len() > 16_384 { return None; }
    let mut output = String::from("AP1\n");
    use std::fmt::Write as _;
    for command in commands {
        match command {
            Command::Dispatch(mut s, a) => {
                let changed = dispatch(&mut s, a);
                writeln!(output, "D {} {}", u8::from(changed), fields(&s)).ok()?;
            }
            Command::Wake(mut s) => { wake(&mut s); writeln!(output, "W {}", fields(&s)).ok()?; }
            Command::Home(selected) => {
                let section = home_section(selected);
                writeln!(output, "H {} {} {}", section as u8, home_section_title(section), home_section_route(section) as u8).ok()?;
            }
            Command::Graph(mut packed, a) => {
                let changed = step_stats_graph(&mut packed, a);
                let position = stats_graph_position(packed);
                writeln!(output, "G {} {} {} {}", u8::from(changed), packed, position.view as u8, position.back).ok()?;
            }
        }
    }
    output.push_str("END\n");
    io::stdout().write_all(output.as_bytes()).ok()?;
    Some(())
}
fn main() { if run().is_none() { std::process::exit(2); } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha16_transport_uses_its_native_state_domains() {
        let mut values = [0; 12];
        values[2] = 256;
        values[3] = u32::MAX;
        values[8] = 9;
        values[11] = 10;
        let value = state(&values).unwrap();
        assert_eq!((value.selected, value.page), (0, 255));
        assert_eq!(value.settings_row, SettingsRow::QuickStart);
        assert_eq!(value.settings_notice, SettingsNotice::QuickStartSaveFailed);
        assert!(screen(7).is_some());
        assert!(screen(8).is_none());
        values[8] = 10;
        assert!(state(&values).is_none());
    }
}
