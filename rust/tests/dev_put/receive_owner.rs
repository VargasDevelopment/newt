// SPDX-License-Identifier: MIT
//! The actual receive token around real lease exits; only the FFI is fake.
use super::*;
use core::sync::atomic::{AtomicU32, Ordering};
use std::{collections::VecDeque, sync::Mutex};
#[allow(dead_code)] // This check exercises only the scoped receive leaves.
#[path = "../../salamander/board/dev_usb.rs"]
mod usb;
static RX: AtomicU32 = AtomicU32::new(4);
static TEST: Mutex<()> = Mutex::new(());
static MICROS: AtomicU32 = AtomicU32::new(0);
static FIFO: Mutex<VecDeque<(u32, u8)>> = Mutex::new(VecDeque::new());
fn micros() -> u32 { MICROS.fetch_add(1, Ordering::SeqCst) }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_begin() -> u32 { RX.swap(0, Ordering::SeqCst) }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_end(previous: u32) { RX.store(previous, Ordering::SeqCst); }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_read_byte() -> i32 {
    assert_eq!(RX.load(Ordering::SeqCst), 0);
    let mut fifo = FIFO.lock().unwrap();
    if fifo.front().is_some_and(|&(at, _)| MICROS.load(Ordering::SeqCst) >= at) {
        i32::from(fifo.pop_front().unwrap().1)
    } else { -1 }
}

fn guarded(owner: &mut usb::ReceiveOwner, card: &mut FakeCard, host: &mut Host) -> Outcome {
    let mut receive = owner.receive();
    assert_eq!(RX.load(Ordering::SeqCst), 0);
    assert_eq!(receive.read_byte(micros), -1);
    lease(card, host)
}
#[test]
fn every_lease_exit_restores_rx() {
    let _test = TEST.lock().unwrap();
    // SAFETY: this single test owns the fake begun port on this thread.
    let mut owner = unsafe { usb::ReceiveOwner::acquired() };
    for exit in ["done", "physical", "timeout", "syntax", "card", "host-gone", "hello", "raw-physical", "raw-host-gone", "raw-timeout"] {
        RX.store(4, Ordering::SeqCst);
        let mut card = FakeCard::default();
        let mut host = Host::new();
        match exit {
            "done" => { host.put("dev", "runtime.bin", b"whole"); }
            "card" => { card.fail = Some("write"); host.put("dev", "runtime.bin", b"whole"); }
            "syntax" => { host.line("BOGUS"); }
            _ => {
                host.file("dev", "runtime.bin", &pattern(CHUNK_BYTES));
                if exit.starts_with("raw-") {
                    host.line(&format!("DATA 0 {CHUNK_BYTES}")).then(Event::Bytes(pattern(32)));
                }
                host.then(match exit {
                    "physical" | "raw-physical" => Event::Press,
                    "host-gone" | "raw-host-gone" => Event::Unplug,
                    "hello" => Event::Line("N1 HELLO\n".into()),
                    _ => Event::Line(String::new()),
                });
            }
        }
        guarded(&mut owner, &mut card, &mut host);
        assert_eq!(RX.load(Ordering::SeqCst), 4, "{exit}");
    }
    fn early(owner: &mut usb::ReceiveOwner) -> Result<(), ()> {
        let _receive = owner.receive();
        Err(())?;
        Ok(())
    }
    assert_eq!(early(&mut owner), Err(()));
    assert_eq!(RX.load(Ordering::SeqCst), 4, "early error return");
}

#[test]
fn packet_gaps_are_polled_but_a_stalled_sender_returns_promptly() {
    let _test = TEST.lock().unwrap();
    // SAFETY: the test lock owns the fake begun port and timer.
    let mut owner = unsafe { usb::ReceiveOwner::acquired() };
    MICROS.store(0, Ordering::SeqCst);
    FIFO.lock().unwrap().extend((0..192).map(|i| ((i / 64) * 125, i as u8)));
    let mut receive = owner.receive();
    for expected in 0..192 { assert_eq!(receive.read_byte(micros), expected); }
    assert!(MICROS.load(Ordering::SeqCst) < 500, "no scheduler sleep between packets");
    for start in [500, u32::MAX - 100] {
        MICROS.store(start, Ordering::SeqCst);
        assert_eq!(receive.read_byte(micros), -1);
        assert!(MICROS.load(Ordering::SeqCst).wrapping_sub(start) <= 251,
            "an empty poll returns to the lease's 3 s deadline");
    }
}
