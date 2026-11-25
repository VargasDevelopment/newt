//! Receive timeout and diagnostic delivery are independent bounds. Neither
//! faster packet reception nor a temporarily missing reader may hide an exit.
use super::*;
use newt_core::dev_put::Wire;

struct Output<'a> { wire: &'a mut FakeWire, connected_after: u32 }
impl Wire for Output<'_> {
    fn connected(&mut self) -> bool { self.wire.clock >= self.connected_after }
    fn read_byte(&mut self) -> i32 { self.wire.read_byte() }
    fn write(&mut self, bytes: &[u8]) -> usize { self.wire.write(bytes) }
    fn now(&mut self) -> u32 { self.wire.now() }
    fn physical_input(&mut self) -> bool { self.wire.physical_input() }
    fn pause(&mut self) { self.wire.pause(); }
}

#[test]
fn stalled_sender_exits_and_its_diagnostic_output_is_bounded() {
    for mode in ["short", "reconnect", "blocked", "gone"] {
        let mut card = FakeCard::default(); card.put_file(TARGET, b"old");
        let mut host = Host::new();
        host.file("dev", "runtime.bin", &pattern(CHUNK_BYTES))
            .line(&format!("DATA 0 {CHUNK_BYTES}"))
            .then(Event::Bytes(vec![0x5a; 17]));
        let mut outcome = lease(&mut card, &mut host);
        assert_eq!(outcome.end, End::Cancelled);
        assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
        assert!((1_000 + IDLE_MS..1_000 + IDLE_MS + 10).contains(&outcome.wire.clock));
        assert_eq!(card.count("write"), 0);
        unchanged(&card, Some(b"old"));
        let mut counters = observation::Counters::default();
        for event in &outcome.wire.observations { counters.observe(outcome.wire.clock, *event); }
        let mut row = String::new(); counters.row(outcome.wire.clock, &mut row).unwrap();
        assert!(row.starts_with("PUT_STATS reason=4 "));
        let sent = outcome.wire.sent.len();
        let start = outcome.wire.clock;
        outcome.wire.write_limit = 3;
        outcome.wire.stalled = mode == "blocked";
        let connected_after = match mode { "gone" => u32::MAX, "reconnect" => start + 5, _ => start };
        observation::report(&mut Output { wire: &mut outcome.wire, connected_after }, row.as_bytes());
        let elapsed = outcome.wire.clock - start;
        assert!(elapsed <= 1_000, "output never extends its existing 1 s budget");
        if mode == "blocked" || mode == "gone" {
            assert_eq!(elapsed, 1_000);
            assert_eq!(outcome.wire.sent.len(), sent);
        } else {
            assert_eq!(&outcome.wire.sent[sent..], row.as_bytes(), "{mode}: one complete row, no replay");
            if mode == "reconnect" { assert_eq!(elapsed, 5); }
        }
    }
}
