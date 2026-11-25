// SPDX-License-Identifier: MIT
//! The actual receive token around real lease exits; only the FFI is fake.
use super::*;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{collections::VecDeque, sync::Mutex};
#[allow(dead_code)] // This check exercises only the scoped receive leaves.
#[path = "../../salamander/board/dev_usb.rs"]
mod usb;
static RX: AtomicU32 = AtomicU32::new(4);
static TEST: Mutex<()> = Mutex::new(());
static MICROS: AtomicU32 = AtomicU32::new(0);
static MICROS_CALLS: AtomicU32 = AtomicU32::new(0);
static UNMASKED_READ: AtomicBool = AtomicBool::new(false);
static STALE_EMPTY: AtomicBool = AtomicBool::new(false);
static QUEUED: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
static FIFO: Mutex<VecDeque<(u32, u8)>> = Mutex::new(VecDeque::new());
fn micros() -> u32 {
    MICROS_CALLS.fetch_add(1, Ordering::SeqCst);
    MICROS.fetch_add(1, Ordering::SeqCst)
}
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_begin() -> u32 { RX.swap(0, Ordering::SeqCst) }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_end(previous: u32) { RX.store(previous, Ordering::SeqCst); }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_read(out: *mut u8, max_len: usize, queue_empty: *mut u8) -> usize {
    if RX.load(Ordering::SeqCst) != 0 { UNMASKED_READ.store(true, Ordering::SeqCst); }
    if out.is_null() || max_len == 0 { return 0; }
    // SAFETY: caller provides a valid writable slice of `max_len` bytes.
    let buf = unsafe { core::slice::from_raw_parts_mut(out, max_len) };
    let mut got = 0;
    let empty_flag = unsafe { queue_empty.as_mut() };
    let mut drained = empty_flag.as_deref().copied().unwrap_or(0) != 0;
    {
        let mut queued = QUEUED.lock().unwrap();
        if drained {
            if !queued.is_empty() { STALE_EMPTY.store(true, Ordering::SeqCst); }
        } else {
            while got < max_len {
                let Some(byte) = queued.pop_front() else { break; };
                buf[got] = byte;
                got += 1;
            }
            if queued.is_empty() {
                drained = true;
                if let Some(flag) = empty_flag { *flag = 1; }
            }
        }
    }
    if drained && got < max_len {
        let now = MICROS.load(Ordering::SeqCst);
        let mut fifo = FIFO.lock().unwrap();
        while got < max_len && fifo.front().is_some_and(|&(at, _)| now >= at) {
            buf[got] = fifo.pop_front().unwrap().1;
            got += 1;
        }
    }
    got
}

fn guarded(owner: &mut usb::ReceiveOwner, card: &mut FakeCard, host: &mut Host) -> Outcome {
    QUEUED.lock().unwrap().clear();
    FIFO.lock().unwrap().clear();
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
    QUEUED.lock().unwrap().clear();
    FIFO.lock().unwrap().clear();
    MICROS.store(0, Ordering::SeqCst);
    MICROS_CALLS.store(0, Ordering::SeqCst);
    FIFO.lock().unwrap().extend((0..192).map(|i| ((i / 64) * 125, i as u8)));
    let mut receive = owner.receive();
    let mut out = [0u8; 192];
    let mut total = 0;
    // First 64-byte packet is already at t=0: fast path must drain it with zero timer calls.
    let first = receive.read(&mut out[..], micros);
    assert_eq!(first, 64, "ready packet drains in one bulk read");
    assert_eq!(MICROS_CALLS.load(Ordering::SeqCst), 0, "ready bytes must not poll micros");
    total += first;
    while total < out.len() {
        let got = receive.read(&mut out[total..], micros);
        assert!(got > 0, "packet gap within quiet bound must return bytes");
        total += got;
    }
    assert!(out.iter().enumerate().all(|(i, &b)| b == i as u8));
    assert!(MICROS.load(Ordering::SeqCst) < 500, "packet gaps return bytes before the quiet bound");
    for start in [500, u32::MAX - 100] {
        MICROS.store(start, Ordering::SeqCst);
        let mut scratch = [0u8; 64];
        assert_eq!(receive.read(&mut scratch, micros), 0);
        assert!(MICROS.load(Ordering::SeqCst).wrapping_sub(start) <= 251,
            "an empty poll returns to the lease's 3 s deadline");
        MICROS.store(start, Ordering::SeqCst);
        assert_eq!(receive.read_byte(micros), -1);
        assert!(MICROS.load(Ordering::SeqCst).wrapping_sub(start) <= 251,
            "an empty byte poll returns to the lease's 3 s deadline");
    }
}

#[test]
fn queued_bytes_precede_fifo_and_reset_across_leases() {
    let _test = TEST.lock().unwrap();
    // SAFETY: the test lock owns the fake begun port and timer.
    let mut owner = unsafe { usb::ReceiveOwner::acquired() };
    RX.store(4, Ordering::SeqCst);
    MICROS.store(0, Ordering::SeqCst);
    UNMASKED_READ.store(false, Ordering::SeqCst);
    STALE_EMPTY.store(false, Ordering::SeqCst);
    QUEUED.lock().unwrap().clear();
    FIFO.lock().unwrap().clear();

    // Lease 1: queued bytes precede FIFO bytes, then queue_empty stays valid while RX is masked.
    QUEUED.lock().unwrap().extend([1u8, 2]);
    FIFO.lock().unwrap().extend([(0, 3u8), (0, 4)]);
    {
        let mut receive = owner.receive();
        assert_eq!(receive.read_byte(micros), 1);
        let mut buf = [0u8; 8];
        let got = receive.read(&mut buf, micros);
        assert_eq!(got, 3);
        assert_eq!(&buf[..3], &[2, 3, 4]);
        assert!(!UNMASKED_READ.load(Ordering::SeqCst), "RX ISR must stay masked during PutReceive");
        assert!(!STALE_EMPTY.load(Ordering::SeqCst), "queued byte present while queue_empty was set");
    }
    assert_eq!(RX.load(Ordering::SeqCst), 4, "Drop restores RX mask");

    // Between leases (RX unmasked), the SDK ISR can enqueue new bytes into rx_queue.
    // Lease 2 must start with queue_empty=0 and drain those queued bytes before new FIFO bytes.
    QUEUED.lock().unwrap().extend([10u8, 11]);
    FIFO.lock().unwrap().extend([(0, 12u8), (0, 13)]);
    {
        let mut receive = owner.receive();
        let mut buf = [0u8; 8];
        let got = receive.read(&mut buf, micros);
        assert!(!STALE_EMPTY.load(Ordering::SeqCst), "queued byte present while queue_empty was set");
        assert!(!UNMASKED_READ.load(Ordering::SeqCst), "RX ISR must stay masked during PutReceive");
        assert_eq!(got, 4);
        assert_eq!(&buf[..4], &[10, 11, 12, 13]);
    }
    assert_eq!(RX.load(Ordering::SeqCst), 4);
}

#[test]
fn bulk_read_never_writes_past_slice_and_leaves_trailing_bytes() {
    let _test = TEST.lock().unwrap();
    // SAFETY: the test lock owns the fake begun port and timer.
    let mut owner = unsafe { usb::ReceiveOwner::acquired() };
    RX.store(4, Ordering::SeqCst);
    MICROS.store(0, Ordering::SeqCst);
    QUEUED.lock().unwrap().clear();
    FIFO.lock().unwrap().clear();
    FIFO.lock().unwrap().extend((0..10u8).map(|b| (0, b + 1)));
    let mut receive = owner.receive();
    let mut buf = [0xCCu8; 12];
    assert_eq!(receive.read(&mut buf[4..8], micros), 4);
    assert_eq!(&buf[..4], &[0xCC; 4]);
    assert_eq!(&buf[4..8], &[1, 2, 3, 4]);
    assert_eq!(&buf[8..], &[0xCC; 4]);
    let mut rest = [0u8; 8];
    assert_eq!(receive.read(&mut rest, micros), 6);
    assert_eq!(&rest[..6], &[5, 6, 7, 8, 9, 10]);
}
