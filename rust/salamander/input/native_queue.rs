// SPDX-License-Identifier: MIT
//! Short operations on the existing synchronized UI queue.
use super::{Event, PhysicalEvent, QueueItem, UiQueue, np_pending, np_receive, np_peek};
#[cfg(feature = "dev-usb")]
use super::np_send_ticket;

impl UiQueue<'_> {
    /// Observe the existing queue without taking its next event.
    pub fn pending(&self) -> bool {
        // SAFETY: the live endpoint holds the same synchronized FreeRTOS queue;
        // this scalar query neither removes an event nor retains a loan.
        unsafe { np_pending(self.queue.as_ptr()) != 0 }
    }

    /// Observe the existing head item without consuming a physical or DEV press.
    pub fn peek_button(&self) -> Option<u8> {
        let mut event = Event::default();
        // SAFETY: the synchronized queue remains live; native copies named
        // fields into this private output and never removes the queued item.
        (unsafe { np_peek(self.queue.as_ptr(), &mut event) } != 0).then_some(event.button)
    }

    /// One unchanged DEV ticket sent to the existing 32-entry input queue.
    #[cfg(feature = "dev-usb")]
    pub fn enqueue_ticket(&mut self, ticket: u32, button: u8) -> bool {
        // Ticket zero belongs exclusively to the physical input producer.
        if ticket == 0 { return false; }
        // SAFETY: this sole UI endpoint owns a live queue and the native call
        // copies the same fixed ticket/button event without retaining a loan.
        unsafe { np_send_ticket(self.queue.as_ptr(), ticket, button) != 0 }
    }

    pub fn try_receive(&mut self) -> Option<QueueItem> {
        let mut event = Event::default();
        // SAFETY: private initialized output; bridge writes named fields only.
        if unsafe { np_receive(self.queue.as_ptr(), &mut event) } == 0 {
            return None;
        }
        Some(if event.ticket == 0 {
            // SAFETY: only the bound physical producer can enqueue ticket zero,
            // and this received item has not previously been reconstructed.
            QueueItem::Physical(unsafe { PhysicalEvent::from_physical_queue(event.button) })
        } else {
            QueueItem::Synthetic {
                ticket: event.ticket,
                button: event.button,
            }
        })
    }
}
