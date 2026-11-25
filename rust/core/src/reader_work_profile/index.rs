//! Attempt-local counters, released before the reader operation begins.
use super::{Scalar, TraceSink, rows::emit};
use crate::admission::{WorkEvent, WorkKind};
#[repr(C)]
pub struct IndexProfile {
    index: [Scalar<u32>; 13],
    index_stage: Scalar<u8>,
    index_started: Scalar<u32>,
}
impl Default for IndexProfile {
    fn default() -> Self {
        Self {
            index: core::array::from_fn(|i| Scalar::new(if i == 0 { u32::MAX } else { 0 })),
            index_stage: Scalar::new(0),
            index_started: Scalar::new(0),
        }
    }
}
impl IndexProfile {
    pub fn event(&self, event: &WorkEvent, us: u32) {
        match event.kind {
            WorkKind::IndexStage => {
                let next = event.result as u8;
                if next != self.index_stage.get() {
                    let previous = usize::from(self.index_stage.get());
                    if (1..=6).contains(&previous) {
                        self.index[previous].set(self.index[previous].get().saturating_add(us.wrapping_sub(self.index_started.get())));
                    }
                    self.index_stage.set(next);
                    self.index_started.set(us);
                }
            }
            WorkKind::IndexResult => {
                let previous = usize::from(self.index_stage.get());
                if (1..=6).contains(&previous) {
                    self.index[previous].set(self.index[previous].get().saturating_add(us.wrapping_sub(self.index_started.get())));
                }
                self.index_stage.set(0);
                self.index[0].set(event.result as u32);
                self.index[7].set(event.compressed_bytes);
                self.index[10].set(u32::from(event.method));
            }
            WorkKind::IndexIo => {
                self.index[11].set(self.index[11].get().saturating_add(event.compressed_bytes));
                self.index[12].set(self.index[12].get().saturating_add(event.expanded_bytes));
            }
            WorkKind::IndexCounts => {
                self.index[8].set(event.compressed_bytes);
                self.index[9].set(event.expanded_bytes);
            }
            _ => {},
        }
    }
    pub fn trace(&self, sink: &mut (impl TraceSink + ?Sized)) { index_row(sink, self); }
}
const _: () = assert!(core::mem::size_of::<IndexProfile>() == 60);

// Two rows so each fits emit's 160 B line (worst case 154 and 122 B with
// u32::MAX values). Keep formatting frames out of the pass's counter frame.
#[inline(never)]
fn index_row(sink: &mut (impl TraceSink + ?Sized), p: &IndexProfile) {
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
        let p = IndexProfile::default();
        let mut sink = Sink::default();
        p.trace(&mut sink);
        assert!(!std::str::from_utf8(&sink.0).unwrap().contains("Reader index"));
        p.event(&WorkEvent::new(WorkKind::IndexStage, 1), 10);
        sink.0.clear();
        p.trace(&mut sink);
        assert!(!std::str::from_utf8(&sink.0).unwrap().contains("Reader index"));
        p.event(&WorkEvent::new(WorkKind::IndexResult, 0), 20);
        sink.0.clear();
        p.trace(&mut sink);
        let rows = std::str::from_utf8(&sink.0).unwrap();
        assert!(rows.contains("Reader index: outcome=0 "));
        assert!(rows.contains("Reader index io: "));
    }
}
