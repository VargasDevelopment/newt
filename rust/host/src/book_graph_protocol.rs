// SPDX-License-Identifier: MIT
//! Host-only binary transport. Parent host dispatch calls nrust19::run(payload).
pub mod nrust19 {
    use newt_core::{book_days::*, reading_stats::*, stats_clock::*, stats_graph_data::*};
    struct Input<'a> {
        b: &'a [u8],
        at: usize,
    }
    impl<'a> Input<'a> {
        fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
            let end = self.at.checked_add(n).ok_or("extent")?;
            let b = self.b.get(self.at..end).ok_or("short payload")?;
            self.at = end;
            Ok(b)
        }
        fn n(&mut self, n: usize) -> Result<u64, &'static str> {
            let mut v = 0;
            for (i, &b) in self.take(n)?.iter().enumerate() {
                v |= u64::from(b) << (8 * i);
            }
            Ok(v)
        }
        fn blob(&mut self) -> Result<&'a [u8], &'static str> {
            let n = self.n(2)? as usize;
            self.take(n)
        }
        fn block(&mut self) -> Result<BookDayBlock, &'static str> {
            let mut b = BookDayBlock {
                block: self.n(2)? as u16,
                ..BookDayBlock::default()
            };
            for s in &mut b.seconds {
                *s = self.n(2)? as u16;
            }
            Ok(b)
        }
        fn month(&mut self) -> Result<StatsMonth, &'static str> {
            let mut m = empty_stats_month(self.n(2)? as u16);
            m.pages = self.n(4)? as u32;
            for s in &mut m.bucket_seconds {
                *s = self.n(4)? as u32;
            }
            for s in &mut m.day_seconds {
                *s = self.n(2)? as u16;
            }
            Ok(m)
        }
        fn counters(&mut self) -> Result<StatsCounters, &'static str> {
            Ok(StatsCounters {
                milliseconds: self.n(8)?,
                pages: self.n(4)? as u32,
                sessions: self.n(4)? as u32,
            })
        }
        fn head(&mut self, d: &StatsDigest) -> Result<BookDays, &'static str> {
            let mut h = BookDays::default();
            if !decode_book_days(self.take(BOOK_DAYS_BYTES)?, d, &mut h) {
                return Err("invalid fixture head");
            }
            Ok(h)
        }
    }
    fn put(o: &mut Vec<u8>, v: u64, n: usize) {
        o.extend_from_slice(&v.to_le_bytes()[..n]);
    }
    fn graph(o: &mut Vec<u8>, g: StatsDaysGraph) {
        put(o, u64::from(g.last_day), 2);
        o.push(g.count);
        for k in g.kinds {
            o.push(k as u8);
        }
        for s in g.seconds {
            put(o, u64::from(s), 4);
        }
    }
    fn periods(o: &mut Vec<u8>, p: StatsPeriodsGraph) {
        for s in p.seconds {
            put(o, u64::from(s), 4);
        }
        put(o, u64::from(p.unknown_seconds), 4);
    }
    fn log(o: &mut Vec<u8>, v: &[u16]) {
        put(o, v.len() as u64, 2);
        for &n in v {
            put(o, u64::from(n), 2);
        }
    }
    pub fn run(payload: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut r = Input { b: payload, at: 0 };
        let op = r.n(1)?;
        let mut o = Vec::new();
        match op {
            0 => {
                let mut p = [0; 4];
                for s in &mut p {
                    *s = r.n(4)? as u32;
                }
                let total = r.n(8)?;
                let newest = r.n(2)? as u16;
                let oldest = r.n(2)? as u16;
                let count = r.n(4)? as u32;
                let back = r.n(4)? as u32;
                let today = r.n(2)? as u16;
                let last = r.n(2)? as u16;
                let since = r.n(2)? as u16;
                put(&mut o, u64::from(stats_unknown_seconds(&p, total)), 4);
                put(
                    &mut o,
                    u64::from(stats_window_end(newest, oldest, count, back)),
                    2,
                );
                o.push(stats_weekday(newest) as u8);
                put(
                    &mut o,
                    u64::from(book_window_end(today, last, since, count, back)),
                    2,
                );
                put(&mut o, u64::from(book_block(newest)), 2);
            }
            1 => {
                let d: StatsDigest = r.take(32)?.try_into().map_err(|_| "digest")?;
                let mut h = r.head(&d)?;
                let mut a = r.block()?;
                let hb = r.blob()?;
                let ab = r.blob()?;
                let block = r.n(2)? as u16;
                let generation = r.n(4)? as u32;
                o.push(u8::from(decode_book_days(hb, &d, &mut h)));
                o.push(u8::from(decode_book_archive(ab, &d, block, &mut a)));
                o.extend(encode_book_days(&d, &h, generation));
                o.extend(encode_book_archive(&d, &a, generation));
            }
            2 => {
                let d: StatsDigest = r.take(32)?.try_into().map_err(|_| "digest")?;
                let mut h = r.head(&d)?;
                let before = r.counters()?;
                let counters = r.counters()?;
                let day = r.n(2)? as u16;
                let hour = r.n(1)? as u8;
                let target = if r.n(1)? != 0 { Some(r.block()?) } else { None };
                let f = StatsFold {
                    counters,
                    clock: stats_clock_for_day(day, hour),
                    ..StatsFold::default()
                };
                put(&mut o, u64::from(book_fold_block(&h, &before, &f)), 2);
                o.push(u8::from(fold_book_days(
                    &mut h,
                    &before,
                    &f,
                    target.as_ref(),
                )));
                o.extend(encode_book_days(&d, &h, 0));
                periods(&mut o, book_periods_graph(&h));
            }
            3 => {
                let d: StatsDigest = r.take(32)?.try_into().map_err(|_| "digest")?;
                let h = r.head(&d)?;
                let last = r.n(2)? as u16;
                let count = r.n(4)? as u32;
                let n = r.n(2)?;
                let mut replies = Vec::new();
                for _ in 0..n {
                    replies.push((r.n(1)?, r.block()?));
                }
                let mut calls = Vec::new();
                let mut index = 0;
                let g = book_days_graph(
                    &h,
                    |b, out| {
                        calls.push(b);
                        let reply = replies
                            .get(index)
                            .copied()
                            .unwrap_or((2, BookDayBlock::default()));
                        index += 1;
                        *out = reply.1;
                        match reply.0 {
                            0 => BookBlockRead::Loaded,
                            1 => BookBlockRead::Absent,
                            _ => BookBlockRead::Unreadable,
                        }
                    },
                    last,
                    count,
                );
                graph(&mut o, g);
                log(&mut o, &calls);
            }
            4 => {
                let mut state = StatsState::Unseeded;
                if !decode_ledger(r.take(LEDGER_BYTES)?, &mut state) {
                    return Err("invalid fixture ledger");
                }
                let ledger = state.seeded().ok_or("unseeded")?;
                let mut c = CrossInkHistory {
                    newest_day: r.n(4)? as u32,
                    ..CrossInkHistory::default()
                };
                c.days.copy_from_slice(r.take(92)?);
                let mut weekdays = [0; 7];
                for s in &mut weekdays {
                    *s = r.n(4)? as u32;
                }
                let mut cp = [0; 4];
                for s in &mut cp {
                    *s = r.n(4)? as u32;
                }
                let last = r.n(2)? as u16;
                let count = r.n(4)? as u32;
                let total = r.n(4)? as u32;
                let n = r.n(2)?;
                let mut replies = Vec::new();
                for _ in 0..n {
                    replies.push((r.n(1)?, r.month()?));
                }
                put(&mut o, u64::from(library_oldest_day(ledger, &c)), 2);
                let mut calls = Vec::new();
                let mut i = 0;
                graph(
                    &mut o,
                    library_days_graph(
                        ledger,
                        |m, out| {
                            calls.push(m);
                            let (s, v) = replies
                                .get(i)
                                .copied()
                                .unwrap_or((2, StatsMonth::default()));
                            i += 1;
                            *out = v;
                            match s {
                                0 => GraphMonthRead::Loaded,
                                1 => GraphMonthRead::Absent,
                                _ => GraphMonthRead::Unreadable,
                            }
                        },
                        &c,
                        &weekdays,
                        last,
                        count,
                    ),
                );
                log(&mut o, &calls);
                let mut calls = Vec::new();
                let mut i = 0;
                periods(
                    &mut o,
                    library_periods_graph(
                        ledger,
                        |m, out| {
                            calls.push(m);
                            let (s, v) = replies
                                .get(i)
                                .copied()
                                .unwrap_or((2, StatsMonth::default()));
                            i += 1;
                            *out = v;
                            match s {
                                0 => GraphMonthRead::Loaded,
                                1 => GraphMonthRead::Absent,
                                _ => GraphMonthRead::Unreadable,
                            }
                        },
                        &cp,
                        total,
                    ),
                );
                log(&mut o, &calls);
            }
            5 => {
                let b = r.blob()?;
                let mut sum = BookImported {
                    seconds: r.n(4)? as u32,
                    pages: r.n(4)? as u32,
                    sessions: r.n(4)? as u32,
                    periods: [0; 4],
                };
                for s in &mut sum.periods {
                    *s = r.n(4)? as u32;
                }
                let mut p = [0; 4];
                for s in &mut p {
                    *s = r.n(4)? as u32;
                }
                let mut w = [0; 7];
                for s in &mut w {
                    *s = r.n(4)? as u32;
                }
                o.push(u8::from(cross_ink_global_periods(b, &mut p)));
                for s in p {
                    put(&mut o, u64::from(s), 4);
                }
                o.push(u8::from(cross_ink_global_weekdays(b, &mut w)));
                for s in w {
                    put(&mut o, u64::from(s), 4);
                }
                o.push(u8::from(cross_ink_book_periods(b, &mut p)));
                for s in p {
                    put(&mut o, u64::from(s), 4);
                }
                o.push(u8::from(cross_ink_book_totals(b, &mut sum)));
                for s in [sum.seconds, sum.pages, sum.sessions] {
                    put(&mut o, u64::from(s), 4);
                }
                for s in sum.periods {
                    put(&mut o, u64::from(s), 4);
                }
            }
            6 => {
                let mut digest = [0; 32];
                digest.copy_from_slice(r.take(32)?);
                let head_bytes = r.blob()?;
                let archive_bytes = r.blob()?;
                let mut head = BookDays {
                    watermark: 99,
                    ..BookDays::default()
                };
                let mut archive = BookDayBlock {
                    block: 154,
                    ..BookDayBlock::default()
                };
                archive.seconds[0] = 11;
                o.push(u8::from(decode_book_days(head_bytes, &digest, &mut head)));
                o.push(u8::from(decode_book_archive(
                    archive_bytes,
                    &digest,
                    153,
                    &mut archive,
                )));
                o.extend_from_slice(&encode_book_days(&[7; 32], &head, 0));
                o.extend_from_slice(&encode_book_archive(&[7; 32], &archive, 0));
            }
            _ => return Err("unknown NRUST-19 operation"),
        }
        if r.at != payload.len() {
            return Err("trailing payload");
        }
        Ok(o)
    }
}
