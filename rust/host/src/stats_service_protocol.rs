// SPDX-License-Identifier: MIT
mod backend;
use backend::Host;
use newt_core::{
    reading_stats::*, reading_stats_service::*, stats_clock::stats_clock_for_day, tracker::*,
};
use std::collections::BTreeMap;
fn hex(b: &[u8]) -> String {
    if b.is_empty() {
        return "-".into();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}
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
fn n(s: &str) -> u64 {
    s.parse().unwrap()
}
pub fn run(input: &str) -> String {
    let mut output = String::new();
    macro_rules! emit { ($($args:tt)*) => {{ use std::fmt::Write as _; writeln!(&mut output,$($args)*).expect("receipt"); }}; }
    let mut h = Host::default();
    backend::reset_tick();
    let mut hashes = BTreeMap::<Vec<u8>, StatsDigest>::new();
    for line in input.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        assert_eq!(f[0], "SS1");
        match f[1] {
            "put" => h.inject(f[2], f[3], unhex(f[4])),
            "sha" => {
                hashes.insert(unhex(f[2]), unhex(f[3]).try_into().unwrap());
            }
            "fault" => {
                h.unavailable = n(f[2]) != 0;
                h.read_fail = n(f[3]) != 0;
                h.fail_from = n(f[4]) as usize;
                h.lost = n(f[5]) as usize;
                h.aside_fail = n(f[6]) != 0;
                h.list_fail = n(f[7]) != 0;
                h.puts = 0;
            }
            "nsfault" => {
                h.fail_namespace = f[2].into();
                h.namespace_fail_from = n(f[3]) as usize;
                h.namespace_puts = 0;
            }
            "open" => {
                h.opens.insert(f[2].into(), n(f[3]) as u8);
            }
            "cold" => {
                h.visible = h.durable.clone();
                h.synced = true;
            }
            "clear" => {
                h.trace.clear();
                backend::reset_tick();
            }
            "dump" => {
                for (ns, kv) in &h.visible {
                    for (k, b) in kv {
                        emit!("V {ns} {k} {}", hex(b));
                    }
                }
                for (ns, kv) in &h.durable {
                    for (k, b) in kv {
                        emit!("D {ns} {k} {}", hex(b));
                    }
                }
                emit!("T {}", h.trace.join("|"));
            }
            "run" => {
                let digest: StatsDigest = unhex(f[3]).try_into().unwrap();
                let clock = stats_clock_for_day(n(f[7]) as u16, n(f[8]) as u8);
                let open = StatsBook {
                    digest,
                    counters: StatsCounters {
                        milliseconds: n(f[4]),
                        pages: n(f[5]) as u32,
                        sessions: n(f[6]) as u32,
                    },
                    end_reached: n(f[9]) != 0,
                    permille: n(f[10]) as u16,
                };
                let mut calls = 0;
                let at = n(f[11]);
                let mut cancel = || {
                    calls += 1;
                    at != 0 && calls >= at
                };
                let sha = |name: &[u8], out: &mut StatsDigest| {
                    *out = *hashes
                        .get(name)
                        .expect("supply native SHA mapping before scan");
                    true
                };
                let report = match f[2] {
                    "seed" => seed_stats(&mut h, Some(&digest), Some(&mut cancel)),
                    "seed-none" => seed_stats(&mut h, None, Some(&mut cancel)),
                    "fold" => fold_stats(&mut h, &open, clock, Some(&mut cancel), &sha),
                    "mark" => mark_stats(
                        &mut h,
                        &digest,
                        clock,
                        if n(f[12]) == 1 {
                            StatsMark::Finished
                        } else {
                            StatsMark::NotFinished
                        },
                        &sha,
                    ),
                    "marked" => {
                        let mark = ReadingDayMark {
                            day: n(f[12]) as u16,
                            state: if n(f[13]) == 1 {
                                DayMarkState::Armed
                            } else {
                                DayMarkState::Crossed
                            },
                            milliseconds: n(f[14]),
                            pages: n(f[15]) as u32,
                            sessions: n(f[16]) as u32,
                            ..ReadingDayMark::default()
                        };
                        fold_marked_stats(&mut h, &open, clock, &mark, Some(&mut cancel), &sha)
                    }
                    "refresh" => StatsReport {
                        result: refresh_sleep_streak(&mut h, clock, Some(&mut cancel)),
                        ..StatsReport::default()
                    },
                    "summary" => {
                        let mut s = StatsSummary::default();
                        let r = summarize_stored_stats(&mut h, clock, &mut s);
                        emit!(
                            "S {} {} {} {} {} {} {} {} {} {}",
                            s.total_seconds,
                            s.pages,
                            s.sessions,
                            s.books_finished,
                            s.today_seconds,
                            s.current_streak,
                            s.longest_streak,
                            u8::from(s.dated),
                            u8::from(s.cross_ink),
                            s.week_seconds
                        );
                        StatsReport {
                            result: r,
                            ..StatsReport::default()
                        }
                    }
                    "facts" => {
                        let mut b = BookFacts::default();
                        let r = stored_book_facts(&mut h, &digest, &mut b);
                        emit!(
                            "B {} {} {} {} {} {}",
                            b.first_day,
                            b.last_day,
                            b.end_day,
                            b.manual_finish_day,
                            b.start_permille,
                            b.flags
                        );
                        StatsReport {
                            result: r,
                            ..StatsReport::default()
                        }
                    }
                    "sleep" => {
                        let field = |at: usize, fallback: u64| f.get(at).map_or(fallback, |s| n(s));
                        let habit = match field(16, 0) {
                            0 => newt_core::sleep_stats::ReadingHabit::None,
                            1 => newt_core::sleep_stats::ReadingHabit::Morning,
                            2 => newt_core::sleep_stats::ReadingHabit::Afternoon,
                            3 => newt_core::sleep_stats::ReadingHabit::Evening,
                            4 => newt_core::sleep_stats::ReadingHabit::Night,
                            _ => panic!("habit"),
                        };
                        let mut e = SleepExtras {
                            progress: field(15, 0) != 0,
                            habit,
                            week: field(17, 0) != 0,
                            days: field(18, 0) as u8,
                            streak: field(19, 0) as u16,
                        };
                        let mut unsaved_calls = 0u32;
                        let mut unsaved_at = None;
                        let mut unsaved_total = None;
                        read_sleep_stats(
                            &mut h,
                            clock,
                            SleepScreenChoices {
                                habit: field(13, 1) != 0,
                                week: field(14, 1) != 0,
                            },
                            |ledger| {
                                unsaved_calls += 1;
                                unsaved_at = Some(backend::tick());
                                unsaved_total = Some(ledger.total_seconds());
                                field(12, 0) as u32
                            },
                            &mut e,
                        );
                        emit!(
                            "E {} {} {} {} {}",
                            u8::from(e.progress),
                            e.habit as u8,
                            u8::from(e.week),
                            e.days,
                            e.streak
                        );
                        emit!(
                            "U {} {} {}",
                            unsaved_calls,
                            unsaved_at.map_or("-".into(), |n| n.to_string()),
                            unsaved_total.map_or("-".into(), |n| n.to_string())
                        );
                        StatsReport {
                            result: StatsResult::Done,
                            ..StatsReport::default()
                        }
                    }
                    _ => panic!("SS1 operation"),
                };
                emit!(
                    "R {} {} {} {} {} {}",
                    stats_kind_name(report.kind),
                    stats_result_name(report.result),
                    match report.finish {
                        BookFinish::No => 0,
                        BookFinish::Yes => 1,
                        BookFinish::Unknown => 2,
                    },
                    report.seconds,
                    report.pages,
                    calls
                );
            }
            _ => panic!("SS1 command"),
        }
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    use newt_core::{durable::Backend, reading_stats_store::*};
    fn sha_unexpected(_: &[u8], _: &mut StatsDigest) -> bool {
        panic!("absent CrossInk never hashes")
    }
    #[test]
    fn unconfirmed_fold_withholds_publication_then_retry_does_not_double_count() {
        let mut h = Host::default();
        let open = StatsBook {
            digest: [17; 32],
            ..StatsBook::default()
        };
        assert_eq!(
            seed_stats(&mut h, Some(&open.digest), None).result,
            StatsResult::Done
        );
        h.puts = 0;
        h.fail_from = 1;
        let changed = StatsBook {
            counters: StatsCounters {
                milliseconds: 9000,
                pages: 3,
                sessions: 1,
            },
            ..open
        };
        let clock = stats_clock_for_day(9800, 12);
        let r = fold_stats(&mut h, &changed, clock, None, &sha_unexpected);
        assert_eq!(
            (r.result, r.seconds, r.pages),
            (StatsResult::Unconfirmed, 0, 0)
        );
        h.fail_from = 0;
        let retry = fold_stats(&mut h, &changed, clock, None, &sha_unexpected);
        assert_eq!(
            (retry.result, retry.seconds, retry.pages),
            (StatsResult::Done, 0, 0)
        );
        let mut summary = StatsSummary::default();
        assert_eq!(
            summarize_stored_stats(&mut h, clock, &mut summary),
            StatsResult::Done
        );
        assert_eq!((summary.total_seconds, summary.pages), (9, 3));
    }
    #[test]
    fn cancellation_prevents_corrupt_ledger_repair() {
        let mut h = Host::default();
        let mut corrupt = vec![0; LEDGER_BYTES];
        corrupt[..4].copy_from_slice(&LEDGER_MAGIC.to_le_bytes());
        corrupt[4..8].copy_from_slice(&1u32.to_le_bytes());
        h.inject(LEDGER_NAMESPACE, "slot0", corrupt.clone());
        h.trace.clear();
        let mut stop = || true;
        let mut state = StatsState::default();
        assert_eq!(
            load_stats(&mut StatsStore::new(&mut h), &mut state, Some(&mut stop)),
            StatsResult::Cancelled
        );
        assert!(!h.trace.iter().any(|e| e.starts_with("aside:")));
        assert_eq!(h.visible[LEDGER_NAMESPACE]["slot0"], corrupt);
    }
    #[test]
    fn end_after_failed_seed_digest_open_is_observable() {
        let mut h = Host::default();
        h.inject("nb1111111111111", "digest", vec![17; 32]);
        h.opens.insert("nb1111111111111".into(), 2);
        assert_eq!(seed_stats(&mut h, None, None).result, StatsResult::Done);
        assert!(
            h.trace
                .windows(2)
                .any(|w| w == ["begin:nb1111111111111:1", "end"])
        );
        // A synthetic backend explicitly tolerates unmatched end; this is a
        // source-observable call requirement, not a physical adapter contract.
        h.end();
    }
}
