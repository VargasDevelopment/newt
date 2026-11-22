// SPDX-License-Identifier: MIT
use newt_core::{
    book_days::*, book_days_store::*, durable::*, reading_stats::*, reading_stats_store::*,
    sleep_stats::*,
};
use std::collections::BTreeMap;
#[derive(Default)]
struct Host {
    visible: BTreeMap<String, [Vec<u8>; 2]>,
    durable: BTreeMap<String, [Vec<u8>; 2]>,
    current: String,
    trace: Vec<String>,
    open: u8,
    unavailable: bool,
    fail_read: u8,
    failures: u8,
    fail_put_from: u16,
    puts: u16,
    durable_reads: bool,
    repair_ok: bool,
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}

fn slot(key: &str) -> usize {
    usize::from(key == "slot1")
}
impl Backend for Host {
    fn begin(&mut self, n: &str, r: bool) -> StorageOpen {
        self.trace.push(format!("begin:{n}:{}", u8::from(r)));
        self.current = n.into();
        match self.open {
            1 => StorageOpen::Missing,
            2 => StorageOpen::Failed,
            3 => StorageOpen::Damaged,
            _ => {
                if r && !self.visible.contains_key(n) {
                    return StorageOpen::Missing;
                }
                self.visible.entry(n.into()).or_default();
                StorageOpen::Opened
            }
        }
    }
    fn end(&mut self) {
        self.trace.push("end".into());
    }
    fn read_blob(&mut self, key: &str, out: &mut [u8]) -> BlobRead {
        self.trace.push(format!("read:{key}"));
        let i = slot(key);
        if self.fail_read == i as u8 {
            return BlobRead::Failed;
        }
        let b = &self.visible[&self.current][i];
        if b.is_empty() {
            return BlobRead::Missing;
        }
        let n = out.len().min(b.len());
        out[..n].copy_from_slice(&b[..n]);
        BlobRead::Present(b.len())
    }
    fn put_bytes(&mut self, _permit: DurableWrite, key: &str, b: &[u8]) -> usize {
        self.trace.push(format!("put:{key}:{}", hex(b)));
        self.visible.get_mut(&self.current).unwrap()[slot(key)] = b.to_vec();
        self.puts += 1;
        let targeted = self.fail_put_from != 0 && self.puts >= self.fail_put_from;
        if self.failures != 0 || targeted {
            self.failures = self.failures.saturating_sub(1);
            return 0;
        }
        self.durable = self.visible.clone();
        b.len()
    }
    fn readback_durable(&self) -> bool {
        self.durable_reads
    }
}
impl StatsBackend for Host {
    fn reading_state_unavailable(&self) -> bool {
        self.unavailable
    }
    fn set_aside(&mut self, n: &str) -> bool {
        self.trace.push(format!("aside:{n}"));
        if !self.repair_ok {
            return false;
        }
        self.visible.remove(n);
        self.durable = self.visible.clone();
        true
    }
    fn repair_in_place(&mut self, n: &str, b: &[u8]) -> bool {
        self.trace.push(format!("repair:{n}:{}", hex(b)));
        if !self.repair_ok {
            return false;
        }
        self.visible.insert(n.into(), [b.to_vec(), vec![]]);
        self.durable = self.visible.clone();
        true
    }
}
struct Input {
    b: Vec<u8>,
    at: usize,
}
impl Input {
    fn n(&mut self, n: usize) -> u64 {
        let mut v = 0;
        for i in 0..n {
            v |= u64::from(self.b[self.at + i]) << (8 * i);
        }
        self.at += n;
        v
    }
    fn take(&mut self, n: usize) -> Vec<u8> {
        let b = self.b[self.at..self.at + n].to_vec();
        self.at += n;
        b
    }
    fn blob(&mut self) -> Vec<u8> {
        let n = self.n(2) as usize;
        self.take(n)
    }
}
fn state(m: &BTreeMap<String, [Vec<u8>; 2]>) -> String {
    m.iter()
        .map(|(n, b)| format!("{n}:{},{}", hex(&b[0]), hex(&b[1])))
        .collect::<Vec<_>>()
        .join(";")
}
fn lc(v: StatsLoad) -> u8 {
    match v {
        StatsLoad::Loaded => 0,
        StatsLoad::Absent => 1,
        StatsLoad::Corrupt => 2,
        StatsLoad::Unsupported => 3,
        StatsLoad::Unavailable => 4,
    }
}
fn sc(v: StatsSave) -> u8 {
    match v {
        StatsSave::Saved => 0,
        StatsSave::Corrupt => 1,
        StatsSave::Unsupported => 2,
        StatsSave::Unavailable => 3,
        StatsSave::Unconfirmed => 4,
    }
}
/// Host-only dispatch; std allocation makes this unsuitable as a target probe.
pub fn run(b: Vec<u8>) -> String {
    let mut r = Input { b, at: 0 };
    let op = r.n(1);
    let kind = r.n(1);
    let cancelled = r.n(1) != 0;
    let mut h = Host {
        open: r.n(1) as u8,
        unavailable: r.n(1) != 0,
        fail_read: r.n(1) as u8,
        failures: r.n(1) as u8,
        durable_reads: r.n(1) != 0,
        repair_ok: r.n(1) != 0,
        fail_put_from: r.n(2) as u16,
        ..Host::default()
    };
    let mut d = [0; 32];
    d.copy_from_slice(&r.take(32));
    let parameter = r.n(2) as u16;
    let payload = r.blob();
    let count = r.n(2);
    for _ in 0..count {
        let name = String::from_utf8(r.blob()).unwrap();
        let a = r.blob();
        let b = r.blob();
        h.visible.insert(name, [a, b]);
    }
    h.durable = h.visible.clone();
    assert_eq!(r.at, r.b.len());
    let mut output = vec![];
    let mut stats = StatsStore::new(&mut h);
    let mut cancel = || cancelled;
    let effects = StatsStore::<Host>::begin_effects(Some(&mut cancel));
    let result = if op != 0 && op != 6 && effects.is_none() {
        255
    } else {
        match op {
            6 => {
                let name = match kind {
                    0 => stats_book_namespace(&d),
                    1 => stats_month_namespace(parameter),
                    2 => book_days_namespace(&d),
                    _ => book_archive_namespace(&d, parameter),
                };
                output.extend_from_slice(name.c_bytes());
                0
            }
            0 => match kind {
                0 => {
                    let mut v = StatsState::default();
                    let s = stats.load_ledger(&mut v);
                    if let Some(l) = v.seeded() {
                        output = encode_ledger(l, 0).to_vec();
                    }
                    lc(s)
                }
                1 => {
                    let mut v = empty_stats_month(parameter);
                    let s = stats.load_month(parameter, &mut v);
                    output = encode_month(&v, 0).to_vec();
                    lc(s)
                }
                2 => {
                    let mut v = BookFacts::default();
                    let s = stats.load_book(&d, &mut v);
                    output = encode_book_facts(&d, &v, 0).to_vec();
                    lc(s)
                }
                3 => {
                    let mut v = StatsStreak::default();
                    let s = stats.load_streak(&mut v);
                    output = encode_streak(&v).to_vec();
                    lc(s)
                }
                4 => {
                    let mut v = BookDays::default();
                    let s = load_book_days(&mut stats, &d, &mut v);
                    output = encode_book_days(&d, &v, 0).to_vec();
                    lc(s)
                }
                5 => {
                    let mut v = BookDayBlock::default();
                    let s = load_day_block(&mut stats, &d, parameter, &mut v);
                    output = encode_book_archive(&d, &v, 0).to_vec();
                    lc(s)
                }
                6 => {
                    let mut v = BookFacts::default();
                    let mut found = [0; 32];
                    let s = stats.load_named_book(
                        core::str::from_utf8(&payload).unwrap(),
                        &mut found,
                        &mut v,
                    );
                    output = encode_book_facts(&found, &v, 0).to_vec();
                    lc(s)
                }
                _ => {
                    let mut v = CrossInkHistory::default();
                    let mut unreadable = false;
                    let s = stats.load_cross_ink(&mut v, Some(&mut unreadable));
                    output.push(u8::from(unreadable));
                    output.extend_from_slice(&v.seconds.to_le_bytes());
                    output.extend_from_slice(&v.days);
                    u8::from(s)
                }
            },
            1 => {
                let e = effects.as_ref().unwrap();
                sc(match kind {
                    0 => {
                        let mut v = StatsState::default();
                        assert!(decode_ledger(&payload, &mut v));
                        stats.save_ledger(e, v.seeded().unwrap())
                    }
                    1 => stats.save_month(e, &decode_month(&payload).unwrap()),
                    2 => stats.save_book(e, &d, &decode_book_facts(&payload, &d).unwrap()),
                    3 => stats.save_streak(e, &decode_streak(&payload).unwrap()),
                    4 => {
                        let mut v = BookDays::default();
                        assert!(decode_book_days(&payload, &d, &mut v));
                        save_book_days(e, &mut stats, &d, &v)
                    }
                    _ => {
                        let mut v = BookDayBlock::default();
                        assert!(decode_book_archive(&payload, &d, parameter, &mut v));
                        save_day_block(e, &mut stats, &d, &v)
                    }
                })
            }
            2 => {
                let e = effects.as_ref().unwrap();
                u8::from(match kind {
                    0 => stats.set_aside_ledger(e),
                    1 => stats.set_aside_month(e, parameter),
                    2 => stats.repair_book(e, &d, &unknown_book_facts()),
                    3 => stats.set_aside_streak(e),
                    4 => set_aside_book_days(e, &mut stats, &d),
                    _ => set_aside_day_block(e, &mut stats, &d, parameter),
                })
            }
            3 => {
                let mut v = BookFacts::default();
                let s = stats.book_facts(effects.as_ref().unwrap(), &d, &mut v);
                output = encode_book_facts(&d, &v, 0).to_vec();
                u8::from(s)
            }
            4 => sc(stats.store_book_facts(
                effects.as_ref().unwrap(),
                &d,
                &decode_book_facts(&payload, &d).unwrap(),
            )),
            5 => {
                let mut p = Input { b: payload, at: 0 };
                let before = StatsCounters {
                    milliseconds: p.n(8),
                    pages: p.n(4) as u32,
                    sessions: p.n(4) as u32,
                };
                let after = StatsCounters {
                    milliseconds: p.n(8),
                    pages: p.n(4) as u32,
                    sessions: p.n(4) as u32,
                };
                let day = p.n(2) as u16;
                let hour = p.n(1) as u8;
                let date = newt_core::stats_clock::stats_date(day);
                let clock = newt_core::stats_clock::stats_clock(newt_core::time_zone::ClockTime {
                    year: date.year,
                    month: date.month,
                    day: date.day,
                    hour,
                    minute: 0,
                });
                let fold = StatsFold {
                    counters: after,
                    clock,
                    ..StatsFold::default()
                };
                fold_stored_book_days(&mut stats, effects.as_ref().unwrap(), &d, &before, &fold);
                0
            }
            7 => {
                // d is supplied native-provider output, never a SHA implementation.
                if kind == 0 {
                    let n = cross_ink_source_namespace(&payload, |_, out| {
                        *out = d;
                        true
                    });
                    output.extend_from_slice(n.c_bytes());
                    0
                } else {
                    let at = usize::from(parameter);
                    u8::from(valid_named_source(
                        &payload[at..],
                        core::str::from_utf8(&payload[..at]).unwrap(),
                        |_, out| {
                            *out = d;
                            true
                        },
                    ))
                }
            }
            _ => panic!("operation"),
        }
    };
    format!(
        "{result}|{}|{}|{}|{}",
        hex(&output),
        h.trace.join(";"),
        state(&h.visible),
        state(&h.durable)
    )
}
