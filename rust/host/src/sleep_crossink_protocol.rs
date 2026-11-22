//! Host transport only; no production I/O in the core modules.
use newt_core::{
    crossink_stats::*, reading_stats::*, sleep_stats::*, stats_clock::*,
};
fn unhex(s: &str) -> Vec<u8> {
    if s == "-" {
        return vec![];
    }
    assert_eq!(s.len() % 2, 0);
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn run(f: &[&str]) -> String {
    let b = unhex(f[1]);
    match f[0].strip_prefix("sc_").expect("sleep/CrossInk namespace") {
        "global" => {
            let class = match classify_cross_ink_global(&b) {
                CrossInkFile::Invalid => 0,
                CrossInkFile::Valid => 1,
                CrossInkFile::Newer => 2,
            };
            let h = decode_cross_ink_global(&b).unwrap_or_default();
            format!(
                "{class} {} {} {} {} {} {} {}",
                h.sessions,
                h.seconds,
                h.pages,
                h.finished,
                h.newest_day,
                h.longest_streak,
                hex(&h.days)
            )
        }
        "book" => format!(
            "{} {}",
            u8::from(valid_cross_ink_book(&b)),
            u8::from(cross_ink_book_completed(&b))
        ),
        "name" => u8::from(cross_ink_source_name(&b)).to_string(),
        "streak" => decode_streak(&b).map_or_else(|| "-".into(), |s| hex(&encode_streak(&s))),
        "global-record" => decode_cross_ink_global_record(&b)
            .and_then(|c| encode_cross_ink_global(&c))
            .map_or_else(|| "-".into(), |b| hex(&b)),
        "source-record" => decode_cross_ink_source(&b)
            .and_then(|c| encode_cross_ink_source(&c))
            .map_or_else(|| "-".into(), |b| hex(&b)),
        "import-record" => {
            decode_import(&b).map_or_else(|| "-".into(), |c| hex(&encode_import(&c)))
        }
        "sleep" => {
            let mut state = StatsState::default();
            assert!(decode_ledger(&b, &mut state));
            let ledger = state.seeded().unwrap();
            let archive_bytes = unhex(f[2]);
            let archive = if f[2] == "-" {
                None
            } else {
                Some(decode_month(&archive_bytes).expect("archive"))
            };
            let cache = if f[3] == "-" {
                StatsStreak::default()
            } else {
                decode_streak(&unhex(f[3])).unwrap()
            };
            let today = stats_clock_for_day(f[4].parse().unwrap(), f[5].parse().unwrap());
            let unsaved = f[6].parse().unwrap();
            let summary = StatsSummary {
                dated: f[7] == "1",
                current_streak: f[8].parse().unwrap(),
                ..StatsSummary::default()
            };
            let recent = f[9].parse().unwrap();
            let before = sleep_seconds_before(ledger, archive.as_ref(), today.day);
            let week = sleep_week(ledger, &cache, archive.as_ref(), today, unsaved);
            format!(
                "{} {} {} {} {} {} {} {} {}",
                u8::from(before.is_some()),
                before.unwrap_or(0),
                sleep_archive_month(ledger, today, false),
                sleep_archive_month(ledger, today, true),
                u8::from(sleep_today_archived(ledger, today)),
                week.days,
                week.streak,
                reading_habit(ledger, archive.as_ref(), today, unsaved) as u8,
                hex(&encode_streak(&sleep_streak(
                    ledger, &summary, recent, today
                )))
            )
        }
        _ => panic!("unknown command"),
    }
}