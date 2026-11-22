// SPDX-License-Identifier: MIT
//! Proposed private SDK/FreeRTOS ABI, not linked or checked against the target.
use crate::{Button, InputIo, PhysicalEvent, Producer, Shared, UiControl};
use core::{ffi::c_void, marker::PhantomData, ptr::NonNull};
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
#[path = "dev_gate.rs"]
mod dev_gate;
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
pub use dev_gate::with_dev_gate;

#[repr(C)]
#[derive(Default)]
struct Event {
    ticket: u32,
    button: u8,
}
unsafe extern "C" {
    fn np_update(input: *mut c_void);
    fn np_physical_power(input: *mut c_void) -> u8;
    fn np_logical_power(input: *mut c_void) -> u8;
    fn np_pressed(input: *mut c_void, button: u8) -> u8;
    #[cfg(not(feature = "rust-app"))]
    fn np_observe(gate: *mut c_void);
    fn np_tail(queue: *mut c_void, button: u8) -> u8;
    fn np_front_wait(queue: *mut c_void, button: u8) -> u8;
    fn np_receive(queue: *mut c_void, event: *mut Event) -> u8;
    fn np_peek(queue: *mut c_void, event: *mut Event) -> u8;
    fn np_pending(queue: *mut c_void) -> u8;
    #[cfg(feature = "dev-usb")]
    fn np_send_ticket(queue: *mut c_void, ticket: u32, button: u8) -> u8;
    fn np_delay_ms(ms: u32);
}
/// Sole initialized fixed-X3 InputManager authority, consumed by task binding.
/// No polling, pointer export, reconfiguration, clone or safe raw acquisition.
/// It is deliberately !Send/!Sync until native lifecycle proof is completed.
pub struct InitializedInput<'a> {
    input: NonNull<c_void>,
    _owner: PhantomData<(&'a mut (), *mut ())>,
}

/// Scoped exclusion of the fixed-X3 input task; suspension is not task death.
/// Only the unsafe platform scope below can create this non-Send/non-Sync proof.
pub struct QuiescedInput<'scope> {
    _scope: PhantomData<(&'scope mut (), *mut ())>,
}

/// # Safety
/// Ordinary sole UI task context. Exclude InputManager polling and every other
/// power-GPIO user; only operation may inspect/configure that GPIO. The native
/// input task is suspended, not started, or dead throughout operation;
/// no ISR/hook/foreign user can reenter it. Retain its objects and restore the
/// prior scheduling state after operation. After committed power changes, do
/// not resume it: remain in this scope until whole-program reset/abort instead.
pub unsafe fn with_quiesced_x3_input<R>(
    operation: impl for<'scope> FnOnce(QuiescedInput<'scope>) -> R,
) -> R {
    operation(QuiescedInput {
        _scope: PhantomData,
    })
}
impl<'a> InitializedInput<'a> {
    /// # Safety
    /// Sole live, initialized, aligned, immovable SDK InputManager for 'a.
    /// The complete fixed-X3/no-touch/no-hook contract of Board::acquire_x3
    /// applies. No second capability, native alias, async input task, begin or
    /// reconfiguration may access it, even if this capability is forgotten.
    /// No Wire/SPI/display/SD authority is transferred. Use only after begin.
    pub unsafe fn from_initialized_x3(input: *mut c_void) -> Option<Self> {
        Some(Self {
            input: NonNull::new(input)?,
            _owner: PhantomData,
        })
    }
}
pub struct TaskPorts<'a> {
    producer: Producer<'a>,
    input: NonNull<c_void>,
    queue: NonNull<c_void>,
    #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
    gate: *mut c_void,
    _owner: PhantomData<(&'a mut (), *mut ())>,
}
pub struct UiQueue<'a> {
    queue: NonNull<c_void>,
    _ui: PhantomData<(&'a mut (), *mut ())>,
}
/// USB tickets stay raw and must pass the existing ticket/acknowledgement gate.
/// Synthetic items never retire physical cancellation, including POWER.
pub enum QueueItem {
    Physical(PhysicalEvent),
    Synthetic { ticket: u32, button: u8 },
}

/// Bind the consumed initialized input to this queue and cancellation state.
/// # Safety
/// Queue is the live existing 32-item PressEvent FreeRTOS queue, with only this
/// physical producer, this receiver and the existing DEV USB producer. Gate is
/// the live ManualTakeoverGate in DEV, null in release. Queue/gate/counters and
/// their owners stay immovable until task death; no queue/gate destruction,
/// counter reset or foreign input alias can race. Only returned UiControl may
/// retire this queue's physical items. Target task creation remains unproved;
/// this function does not make the target endpoints Send/Sync.
pub unsafe fn split_initialized<'a>(
    input: InitializedInput<'a>,
    queue: *mut c_void,
    _gate: *mut c_void,
    shared: &'a mut Shared,
) -> Option<(TaskPorts<'a>, UiQueue<'a>, UiControl<'a>)> {
    let queue = NonNull::new(queue)?;
    let (producer, control) = shared.split();
    Some((
        TaskPorts {
            producer,
            input: input.input,
            queue,
            #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
            gate: _gate,
            _owner: PhantomData,
        },
        UiQueue {
            queue,
            _ui: PhantomData,
        },
        control,
    ))
}
// Host-only reviewed transport proposal. Native counters deliberately confer
// no target Send/Sync until native ABI and task lifetime/exclusion proof exists.
// SAFETY: split_initialized requires exclusive task ownership; only the synchronized
// queue and gate cross tasks. InputManager moves as authority, never as object.
#[cfg(not(any(target_arch = "riscv32", feature = "native-atomics")))]
unsafe impl Send for TaskPorts<'_> {}
struct PollIo {
    input: NonNull<c_void>,
    queue: NonNull<c_void>,
    #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
    gate: *mut c_void,
}
impl InputIo for PollIo {
    fn update(&mut self) {
        unsafe { np_update(self.input.as_ptr()) }
    }
    fn power_physically_pressed(&mut self) -> bool {
        unsafe { np_physical_power(self.input.as_ptr()) != 0 }
    }
    fn power_logically_pressed(&mut self) -> bool {
        unsafe { np_logical_power(self.input.as_ptr()) != 0 }
    }
    fn was_pressed(&mut self, button: Button) -> bool {
        unsafe { np_pressed(self.input.as_ptr(), button as u8) != 0 }
    }
    fn observe_physical(&mut self) {
        #[cfg(not(feature = "rust-app"))]
        unsafe { np_observe(self.gate) }
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        dev_gate::observe(self.gate);
    }
    fn try_send_tail(&mut self, event: &PhysicalEvent) -> bool {
        unsafe { np_tail(self.queue.as_ptr(), event.queue_code()) != 0 }
    }
    fn send_front_wait(&mut self, event: &PhysicalEvent) -> bool {
        unsafe { np_front_wait(self.queue.as_ptr(), event.queue_code()) != 0 }
    }
    fn delay_ms(&mut self, ms: u32) {
        unsafe { np_delay_ms(ms) }
    }
}
impl TaskPorts<'_> {
    /// Already-bound task body; creating its native task remains caller work.
    pub fn run(mut self) -> ! {
        loop {
            self.poll_once();
        }
    }
    /// The native queue is writable only by its bound policy producer.
    pub fn poll_once(&mut self) {
        self.producer.poll_once(&mut PollIo {
            input: self.input,
            queue: self.queue,
            #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
            gate: self.gate,
        });
    }
}
#[path = "native_queue.rs"]
mod queue;
