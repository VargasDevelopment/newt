// SPDX-License-Identifier: MIT
use super::ReaderActionProfile;
use crate::reader_work_profile::{rows::emit, TraceSink};

// Keep formatting arguments below the shallow emission tail, outside reader work.
#[inline(never)]
pub fn trace_reader_action(sink: &mut (impl TraceSink + ?Sized), action: &ReaderActionProfile) {
    let mut mask = u32::from(summary(sink, action));
    if durations(sink, action) { mask |= 2; }
    if tail(sink, action) { mask |= 4; }
    emit(sink, 64, format_args!("Reader action rows: emitted={} expected=7\n", mask));
}

// One row's arguments at a time retain the frozen single 160-byte row buffer.
#[inline(never)]
fn summary(sink: &mut (impl TraceSink + ?Sized), action: &ReaderActionProfile) -> bool {
    emit(sink, 160, format_args!("Reader action: schema={} total={} residual={} flags={} actions={} route={} miss={}\n",
        ReaderActionProfile::SCHEMA, action.total(), action.residual(), action.faults(),
        action.actions(), action.routes(), action.miss_reason()))
}

#[inline(never)]
fn durations(sink: &mut (impl TraceSink + ?Sized), action: &ReaderActionProfile) -> bool {
    let e = action.elapsed();
    emit(sink, 160, format_args!("Reader action us: v={},{},{},{},{},{},{},{},{}\n",
        e[0].get(), e[1].get(), e[2].get(), e[3].get(), e[4].get(), e[5].get(),
        e[6].get(), e[7].get(), e[8].get()))
}

#[inline(never)]
fn tail(sink: &mut (impl TraceSink + ?Sized), action: &ReaderActionProfile) -> bool {
    let e = action.elapsed();
    if action.closing() {
        emit(sink, 160, format_args!("Reader close us: home={} progress={} release={} exit={} destination={} device={}\n",
            e[9].get(), e[10].get(), e[11].get(), e[12].get(), e[13].get(), e[14].get()))
    } else {
        emit(sink, 160, format_args!("Reader open us: select={} progress={} source={} bind={} commit={} device={}\n",
            e[9].get(), e[10].get(), e[11].get(), e[12].get(), e[13].get(), e[14].get()))
    }
}
