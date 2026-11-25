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

fn index_row(sink: &mut (impl TraceSink + ?Sized), p: &ReaderWorkProfile) {
    use core::fmt::Write;
    struct Row { bytes: [u8; 384], len: usize }
    impl core::fmt::Write for Row {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
            let out = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            out.copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut row = Row { bytes: [0; 384], len: 0 };
    if write!(row, "Reader index: outcome={} status={} census_ms={} central_ms={} opf_ms={} join_ms={} write_ms={} commit_ms={} members={} spine={} work_written={} work_read={} work_removed={}\n",
        p.index[0].get(), p.index[7].get(), p.index[1].get()/1000, p.index[2].get()/1000,
        p.index[3].get()/1000, p.index[4].get()/1000, p.index[5].get()/1000, p.index[6].get()/1000,
        p.index[8].get(), p.index[9].get(), p.index[11].get(), p.index[12].get(), p.index[10].get()).is_ok()
        && sink.available_for_write() >= row.len as i32 {
        sink.write(&row.bytes[..row.len]);
    }
}
