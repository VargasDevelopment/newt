// SPDX-License-Identifier: MIT
use std::collections::VecDeque;
#[allow(dead_code, unused_imports)]
#[path = "../../firmware/dev_usb.rs"]
mod dev_usb;
use dev_usb::{Bridge, ButtonEvent, Channel, Device, Diagnostics, PanelRing, Status, FRAME_BYTES};

// The real bridge includes its port adapter; this harness uses Wire instead.
#[allow(dead_code)]
mod serial_console {
    pub struct DevChannel<'a>(core::marker::PhantomData<&'a mut ()>);
    impl DevChannel<'_> {
        pub fn connected(&mut self) -> bool { panic!("unused native channel") }
        pub fn read_byte(&mut self) -> i32 { panic!("unused native channel") }
        pub fn write(&mut self, _: &[u8]) -> usize { panic!("unused native channel") }
        pub fn flush_tx_tail(&mut self) -> bool { panic!("unused native channel") }
        pub fn now(&mut self) -> u32 { panic!("unused native channel") }
    }
}

struct Wire { incoming: VecDeque<u8>, output: Vec<u8>, time: u32 }
impl Channel for Wire {
    fn connected(&mut self) -> bool { true }
    fn read_byte(&mut self) -> i32 { self.incoming.pop_front().map_or(-1, i32::from) }
    fn write(&mut self, bytes: &[u8]) -> usize { self.output.extend(bytes); bytes.len() }
    fn now(&mut self) -> u32 { self.time += 1; self.time }
}
struct View {
    #[cfg(feature = "wake-timing")]
    timeline: newt_core::wake_timing::Timeline,
}
impl Device for View {
    fn status(&mut self) -> Status { Status::default() }
    fn frame(&mut self) -> Option<&[u8; FRAME_BYTES]> { None }
    fn enqueue(&mut self, _: ButtonEvent) -> bool { false }
    fn panel_ring(&mut self) -> Option<&PanelRing> { None }
    fn diagnostics(&mut self) -> Diagnostics { Diagnostics::default() }
    #[cfg(feature = "wake-timing")]
    fn wake_timing(&self) -> Option<[u32; 24]> { self.timeline.snapshot() }
    fn put(&mut self, _: newt_core::dev_put::Session) -> Result<(), newt_core::dev_put::Busy> {
        Err(newt_core::dev_put::Busy::NotHome)
    }
}
fn exchange(bridge: &mut Bridge, wire: &mut Wire, view: &mut View, request: &str) -> String {
    wire.output.clear(); wire.incoming.extend(request.bytes());
    for _ in 0..100 { bridge.poll(wire, view, "0123456789ab", true); }
    let response = String::from_utf8(wire.output.clone()).unwrap();
    let (row, checksum) = response.trim_end().rsplit_once(" *").unwrap();
    assert_eq!(u32::from_str_radix(checksum, 16).unwrap(), newt_core::wire::crc32(row.as_bytes()));
    response
}
#[test]
fn explicit_snapshot_reads_freeze_while_default_firmware_rejects_the_command() {
    let mut bridge = Bridge::new(7);
    let mut wire = Wire { incoming: VecDeque::new(), output: Vec::new(), time: 0 };
    let mut view = View {
        #[cfg(feature = "wake-timing")]
        timeline: newt_core::wake_timing::Timeline::new(),
    };
    assert!(exchange(&mut bridge, &mut wire, &mut view, "N1 HELLO\n").contains("HELLO build="));
    let first = exchange(&mut bridge, &mut wire, &mut view, "N1 7 1 1 WAKE\n");
    #[cfg(not(feature = "wake-timing"))]
    assert!(first.contains("ERROR command"));
    #[cfg(feature = "wake-timing")]
    {
        assert!(first.contains("WAKE version=1 available=0"));
        view.timeline.begin(0, 160); view.timeline.page(1);
        view.timeline.display_start(100, 0); view.timeline.display_return(200); view.timeline.complete(210);
        let saved = view.timeline.snapshot().unwrap();
        let encoded: String = saved.iter().flat_map(|word| word.to_le_bytes())
            .map(|byte| format!("{byte:02x}")).collect();
        let second = exchange(&mut bridge, &mut wire, &mut view, "N1 7 1 2 WAKE\n");
        assert!(second.contains(&format!("WAKE version=1 available=1 data={encoded}")), "{second:?}");
        assert!(second.len() < 512);
        view.timeline.page(2); view.timeline.complete(999);
        let third = exchange(&mut bridge, &mut wire, &mut view, "N1 7 1 3 WAKE\n");
        assert!(third.contains(&encoded)); assert_eq!(view.timeline.snapshot(), Some(saved));
        assert!(exchange(&mut bridge, &mut wire, &mut view, "N1 7 1 3 WAKE\n").contains("ERROR stale-id"));
    }
}
