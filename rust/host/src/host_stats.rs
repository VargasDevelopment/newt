//! Host-only transport; production logic remains in newt_core::reading_stats.
use newt_core::reading_stats::*;
use newt_core::stats_clock::stats_clock_for_day;

fn n(s: &str) -> u64 {
    s.parse().expect("stats numeric field")
}
fn unhex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    assert_eq!(s.len() % 2, 0);
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| {
            let text = std::str::from_utf8(p).expect("hex text");
            u8::from_str_radix(text, 16).expect("hex byte")
        })
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn facts(f: &[&str]) -> BookFacts {
    BookFacts {
        first_day: n(f[0]) as u16,
        last_day: n(f[1]) as u16,
        end_day: n(f[2]) as u16,
        manual_finish_day: n(f[3]) as u16,
        start_permille: n(f[4]) as u16,
        flags: n(f[5]) as u16,
    }
}
fn lease(f: &[&str]) -> StatsLease {
    StatsLease {
        digest: [n(f[0]) as u8; 32],
        counters: StatsCounters {
            milliseconds: n(f[1]),
            pages: n(f[2]) as u32,
            sessions: n(f[3]) as u32,
        },
        facts: facts(&f[4..]),
    }
}
fn fold(f: &[&str]) -> (StatsFold, Option<StatsMonth>) {
    let fold = StatsFold {
        counters: StatsCounters {
            milliseconds: n(f[0]),
            pages: n(f[1]) as u32,
            sessions: n(f[2]) as u32,
        },
        clock: stats_clock_for_day(n(f[3]) as u16, n(f[4]) as u8),
        end_reached: n(f[5]) != 0,
        mark: match n(f[6]) {
            0 => StatsMark::None,
            1 => StatsMark::Finished,
            2 => StatsMark::NotFinished,
            _ => panic!("stats mark"),
        },
        permille: n(f[7]) as u16,
        cross_ink_finished: n(f[8]) != 0,
    };
    let target = if f[9] == "-" {
        None
    } else {
        decode_month(&unhex(f[9]))
    };
    (fold, target)
}
pub fn ledger_command(f: &[&str]) -> String {
    let seed: Vec<_> = f[1].split(',').collect();
    let mut ledger = seed_ledger(&StatsSeed {
        milliseconds: n(seed[0]),
        pages: n(seed[1]),
        sessions: n(seed[2]),
        finished_books: n(seed[3]) as u32,
        open: n(seed[4]) != 0,
        lease: lease(&seed[5..]),
    });
    let mut rows = vec![format!("0:{}", hex(&encode_ledger(&ledger, 17)))];
    for word in &f[2..] {
        let e: Vec<_> = word.split(',').collect();
        let needs = match e[0] {
            "F" => {
                let (fold, target) = fold(&e[1..]);
                let needs = fold_needs_month(&ledger, &fold);
                ledger = fold_ledger(&ledger, &fold, target.as_ref());
                needs
            }
            "H" => {
                let next = lease(&e[1..11]);
                let final_fold = if e.len() > 11 {
                    assert_eq!(e[11], "F");
                    Some(fold(&e[12..]))
                } else {
                    None
                };
                let needs = final_fold
                    .as_ref()
                    .is_some_and(|(fold, _)| fold_needs_month(&ledger, fold));
                ledger = handover_ledger(
                    &ledger,
                    final_fold.as_ref().map(|(fold, _)| fold),
                    final_fold.as_ref().and_then(|(_, target)| target.as_ref()),
                    &next,
                );
                needs
            }
            "Q" => false,
            _ => panic!("stats event"),
        };
        rows.push(format!(
            "{}:{}",
            u8::from(needs),
            hex(&encode_ledger(&ledger, 17))
        ));
    }
    rows.join(" ; ")
}
fn class(c: StatsDecode) -> u8 {
    match c {
        StatsDecode::Valid => 0,
        StatsDecode::Corrupt => 1,
        StatsDecode::Unsupported => 2,
    }
}
pub fn wire_command(f: &[&str]) -> String {
    let bytes = unhex(f[2]);
    let magic = match f[1] {
        "L" => LEDGER_MAGIC,
        "M" => MONTH_MAGIC,
        "B" => BOOK_MAGIC,
        "C" => n(f[3]) as u32,
        _ => panic!("stats wire kind"),
    };
    let classification = class(classify_stats_record(&bytes, magic));
    if f[1] == "C" {
        return classification.to_string();
    }
    let generation = if bytes.len() >= 12 {
        u32::from_le_bytes(bytes[8..12].try_into().unwrap())
    } else {
        0
    };
    let (ok, state, canonical) = match f[1] {
        "L" => {
            let mut state = StatsState::Unavailable;
            let ok = decode_ledger(&bytes, &mut state);
            (
                ok,
                match state {
                    StatsState::Unseeded => 0,
                    StatsState::Seeded(_) => 1,
                    StatsState::Unsupported => 2,
                    StatsState::Unavailable => 3,
                },
                state.seeded().map(|l| hex(&encode_ledger(l, generation))),
            )
        }
        "M" => {
            let month = decode_month(&bytes);
            (
                month.is_some(),
                0,
                month.map(|m| hex(&encode_month(&m, generation))),
            )
        }
        "B" => {
            let digest = [n(f[3]) as u8; 32];
            let facts = decode_book_facts(&bytes, &digest);
            (
                facts.is_some(),
                0,
                facts.map(|v| hex(&encode_book_facts(&digest, &v, generation))),
            )
        }
        _ => unreachable!(),
    };
    format!(
        "{} {} {} {}",
        classification,
        u8::from(ok),
        state,
        canonical.unwrap_or_else(|| "-".into())
    )
}
pub fn make_command(f: &[&str]) -> String {
    match f[1] {
        "H" => {
            let mut cross = CrossInkHistory {
                newest_day: n(f[2]) as u32,
                ..CrossInkHistory::default()
            };
            cross.days.copy_from_slice(&unhex(f[3]));
            let mut out = vec![u8::from(cross.empty()).to_string()];
            out.extend(
                f[4..]
                    .iter()
                    .map(|day| u8::from(cross.read(n(day) as u32)).to_string()),
            );
            out.join(" ")
        }
        "C" => {
            let clock = stats_clock_for_day(n(f[2]) as u16, n(f[3]) as u8);
            format!(
                "{} {} {}",
                newt_core::stats_clock::stats_bucket(n(f[3]) as u8),
                u8::from(newt_core::stats_clock::halloween(clock)),
                u8::from(newt_core::stats_clock::christmas(clock))
            )
        }
        "M" => {
            let mut month = empty_stats_month(n(f[2]) as u16);
            month.pages = n(f[3]) as u32;
            for (i, v) in month.bucket_seconds.iter_mut().enumerate() {
                *v = n(f[4 + i]) as u32;
            }
            for (i, v) in month.day_seconds.iter_mut().enumerate() {
                *v = n(f[8 + i]) as u16;
            }
            hex(&encode_month(&month, n(f[39]) as u32))
        }
        "B" => hex(&encode_book_facts(
            &[n(f[2]) as u8; 32],
            &facts(&f[3..9]),
            n(f[9]) as u32,
        )),
        _ => panic!("stats make kind"),
    }
}
pub fn summary_command(f: &[&str]) -> String {
    let mut state = StatsState::Unavailable;
    if !decode_ledger(&unhex(f[1]), &mut state) {
        return "invalid".into();
    }
    let clock = stats_clock_for_day(n(f[2]) as u16, n(f[3]) as u8);
    let cross = if f[4] == "-" {
        None
    } else {
        let c: Vec<_> = f[4].split(',').collect();
        let mut cross = CrossInkHistory {
            seconds: n(c[0]) as u32,
            pages: n(c[1]) as u32,
            sessions: n(c[2]) as u32,
            finished: n(c[3]) as u32,
            newest_day: n(c[4]) as u32,
            longest_streak: n(c[5]) as u16,
            ..CrossInkHistory::default()
        };
        cross.days.copy_from_slice(&unhex(c[6]));
        Some(cross)
    };
    let mut calls = Vec::new();
    let mut recent = 171;
    let s = summarize_stats(
        state.seeded().unwrap(),
        clock,
        |month| {
            calls.push(month);
            let archive = f[5..].iter().find_map(|word| {
                let (key, value) = word.split_once(':').expect("archive pair");
                (n(key) as u16 == month).then_some(value)
            });
            match archive {
                None | Some("M") => MonthLoad::Missing,
                Some("U") => MonthLoad::Unavailable,
                Some(bytes) => {
                    decode_month(&unhex(bytes)).map_or(MonthLoad::Unavailable, MonthLoad::Loaded)
                }
            }
        },
        cross.as_ref(),
        Some(&mut recent),
    );
    format!(
        "{} {} {} {} {} {} {} {} {} {} {} | {}",
        s.total_seconds,
        s.sessions,
        s.pages,
        s.books_finished,
        u8::from(s.dated),
        u8::from(s.cross_ink),
        s.today_seconds,
        s.week_seconds,
        s.current_streak,
        s.longest_streak,
        recent,
        calls
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}
