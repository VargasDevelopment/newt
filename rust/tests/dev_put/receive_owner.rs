// SPDX-License-Identifier: MIT
//! The actual receive token around real lease exits; only the FFI is fake.
use super::*;
use core::sync::atomic::{AtomicU32, Ordering};
#[allow(dead_code)] // This check exercises only the scoped receive leaves.
#[path = "../../salamander/board/dev_usb.rs"]
mod usb;
static RX: AtomicU32 = AtomicU32::new(4);
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_begin() -> u32 { RX.swap(0, Ordering::SeqCst) }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_end(previous: u32) { RX.store(previous, Ordering::SeqCst); }
#[unsafe(no_mangle)]
extern "C" fn nsc_put_rx_read_byte() -> i32 {
    assert_eq!(RX.load(Ordering::SeqCst), 0);
    -1
}

fn guarded(owner: &mut usb::ReceiveOwner, card: &mut FakeCard, host: &mut Host) -> Outcome {
    let mut receive = owner.receive();
    assert_eq!(RX.load(Ordering::SeqCst), 0);
    assert_eq!(receive.read_byte(), -1);
    lease(card, host)
}
#[test]
fn every_lease_exit_restores_rx() {
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
