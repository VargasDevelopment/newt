use super::{rows::emit, ReaderWorkProfile};

/// The existing Serial admission/acknowledgement contract. Both are required;
/// fmt::Write alone cannot observe a short write or available TX capacity.
pub trait TraceSink {
    fn available_for_write(&mut self) -> i32;
    fn write(&mut self, bytes: &[u8]) -> usize;
}

pub fn trace_reader_work(sink: &mut (impl TraceSink + ?Sized), profile: &ReaderWorkProfile) -> bool {
    let [a, m, l] = profile.phases();
    emit(sink, 160, format_args!("Reader work: a={},{},{},{} m={},{},{},{} l={},{},{},{}\n",
        a.milliseconds.get(), a.callbacks.get(), a.sleeps.get(), a.wait_milliseconds.get(),
        m.milliseconds.get(), m.callbacks.get(), m.sleeps.get(), m.wait_milliseconds.get(),
        l.milliseconds.get(), l.callbacks.get(), l.sleeps.get(), l.wait_milliseconds.get()))
}

fn detail(sink: &mut (impl TraceSink + ?Sized), mask: &mut u32, row: u32, args: core::fmt::Arguments<'_>) {
    if emit(sink, 160, args) { *mask |= 1 << row; }
}

/// Frozen order and mask, including best-effort final mask emission. Never
/// called from an observation callback; each row is one bounded stack write.
pub fn trace_reader_details(sink: &mut (impl TraceSink + ?Sized), profile: &ReaderWorkProfile, work_emitted: bool) {
    let mut mask = u32::from(work_emitted);
    let o = profile.opening();
    detail(sink, &mut mask, 12, format_args!("Reader route: flags={} terminal={} status={} cause={} ready={} elapsed={} cancel={} active={}\n",
        o.flags.get(), o.terminal.get() as u8, o.status.get(), o.cause_status.get(), u8::from(o.ready.get()), o.elapsed.get(), o.cancellation.get(), u8::from(o.active.get())));
    let a = profile.admission();
    detail(sink, &mut mask, 1, format_args!("Reader stages: inspect={},{} catalog={},{} compare={},{} n={}\n",
        a[0].microseconds.get(), a[0].calls.get(), a[1].microseconds.get(), a[1].calls.get(), a[2].microseconds.get(), a[2].calls.get(), profile.comparisons()));
    let m = profile.members();
    detail(sink, &mut mask, 2, format_args!("Reader members: n={},{},{},{} image={},{},{} fail={}\n",
        m.started.get(), m.completed.get(), m.stored.get(), m.deflated.get(), m.image_named.get(), m.image_microseconds.get(), m.image_produced.get(), m.failed.get()));
    let s = profile.streams();
    detail(sink, &mut mask, 3, format_args!("Reader streams: calls={} us={} out={} io={} read={}\n",
        s.calls.get(), s.microseconds.get(), s.produced.get(), s.io_microseconds.get(), s.read_bytes.get()));
    let h = profile.hashing();
    detail(sink, &mut mask, 4, format_args!("Reader hash: us={} calls={} update={},{} bytes={} wait={},{}\n",
        h.microseconds.get(), h.calls.get(), h.update_microseconds.get(), h.updates.get(), h.bytes.get(), h.wait_microseconds.get(), h.waits.get()));
    for (i, b) in profile.io_buckets().iter().enumerate() {
        detail(sink, &mut mask, 5 + i as u32, format_args!("Reader io: bucket={} size={},{} pos={},{} seek={},{} read={},{} bytes={}\n",
            i, b.calls[0].get(), b.microseconds[0].get(), b.calls[1].get(), b.microseconds[1].get(), b.calls[2].get(), b.microseconds[2].get(), b.calls[3].get(), b.microseconds[3].get(), b.read_bytes.get()));
    }
    for (role, record) in [profile.current_member(), profile.maximum_member()].iter().enumerate() {
        detail(sink, &mut mask, 10 + role as u32, format_args!("Reader member: role={} data={},{},{},{},{},{},{},{},{},{},{} active={}\n",
            role, record.ordinal.get(), record.method.get(), record.category.get(), record.compressed.get(), record.expanded.get(),
            record.stream.calls.get(), record.stream.microseconds.get(), record.stream.produced.get(), record.stream.io_microseconds.get(),
            record.stream.read_bytes.get(), record.result.get(), u8::from(role == 0 && profile.member_active())));
    }
    index_row(sink, profile);
    emit(sink, 64, format_args!("Reader rows: emitted={} expected=8191\n", mask));
}

// Two rows so each fits emit's 160 B line (worst case 154 and 122 B with
// u32::MAX values). Kept out of line: the open profile inlines this module's
// rows, and its local frame has a 672 B ceiling (tools/rust_open_stack.py).
#[inline(never)]
fn index_row(sink: &mut (impl TraceSink + ?Sized), p: &ReaderWorkProfile) {
    if p.index[0].get() == u32::MAX { return; }
    let ms = |slot: usize| p.index[slot].get() / 1000;
    emit(sink, 160, format_args!("Reader index: outcome={} status={} census_ms={} central_ms={} opf_ms={} join_ms={} write_ms={} commit_ms={}\n",
        p.index[0].get(), p.index[7].get(), ms(1), ms(2), ms(3), ms(4), ms(5), ms(6)));
    emit(sink, 160, format_args!("Reader index io: members={} spine={} work_written={} work_read={} work_removed={}\n",
        p.index[8].get(), p.index[9].get(), p.index[11].get(), p.index[12].get(), p.index[10].get()));
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::admission::{WorkEvent, WorkKind};
    #[test]
    fn index_rows_require_a_result_including_for_a_hit() {
        #[derive(Default)]
        struct Sink(std::vec::Vec<u8>);
        impl TraceSink for Sink {
            fn available_for_write(&mut self) -> i32 { 4096 }
            fn write(&mut self, bytes: &[u8]) -> usize {
                self.0.extend_from_slice(bytes); bytes.len()
            }
        }
        let p = ReaderWorkProfile::default();
        let mut sink = Sink::default();
        trace_reader_details(&mut sink, &p, true);
        assert!(!std::str::from_utf8(&sink.0).unwrap().contains("Reader index"));
        p.event(&WorkEvent::new(WorkKind::IndexStage, 1), 10, 0);
        sink.0.clear();
        trace_reader_details(&mut sink, &p, true);
        assert!(!std::str::from_utf8(&sink.0).unwrap().contains("Reader index"));
        p.event(&WorkEvent::new(WorkKind::IndexResult, 0), 20, 0);
        sink.0.clear();
        trace_reader_details(&mut sink, &p, true);
        let rows = std::str::from_utf8(&sink.0).unwrap();
        assert!(rows.contains("Reader index: outcome=0 "));
        assert!(rows.contains("Reader index io: "));
    }
}
