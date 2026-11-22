// SPDX-License-Identifier: MIT
//! Standalone optional application glue crate; no new Cargo package or runner.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg(feature = "native-input-task")]
#[cfg(not(panic = "abort"))]
compile_error!("native input owner requires panic=abort for every Rust frame");
#[cfg(not(any(target_arch = "riscv32", feature = "native-atomics")))]
compile_error!("native input owner requires the native atomic backend");

use core::{ffi::c_void, marker::PhantomData};
use newt_c3_board::{Board, Error, Probe};
use newt_input_policy::{
    NativeAtomicU32, Shared, UiControl,
    native::{QuiescedInput, TaskPorts, UiQueue, split_initialized, with_quiesced_x3_input},
};
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
#[path = "app_gate.rs"]
mod app_gate;
#[cfg(feature = "rust-app")]
#[path = "deferred_start.rs"]
mod deferred_start;
#[cfg(feature = "rust-app")]
pub use deferred_start::{DeferredApplication, InputStart, run_acquired_deferred, run_deferred};

unsafe extern "C" {
    fn np_owner_with(
        body: extern "C" fn(*mut c_void, *mut c_void) -> u32,
        argument: *mut c_void,
    ) -> u32;
    fn np_owner_queue(owner: *mut c_void) -> *mut c_void;
    fn np_owner_gate(owner: *mut c_void) -> *mut c_void;
    fn np_owner_state(owner: *mut c_void) -> *mut NativeAtomicU32;
    fn np_owner_dropped(owner: *mut c_void) -> *mut NativeAtomicU32;
    fn np_owner_start(
        owner: *mut c_void,
        entry: extern "C" fn(*mut c_void),
        context: *mut c_void,
    ) -> u8;
    fn np_owner_suspend(owner: *mut c_void) -> u8;
    fn np_owner_resume(owner: *mut c_void);
    #[cfg(not(feature = "rust-app"))]
    fn np_owner_consume(
        owner: *mut c_void,
        bridge: *mut c_void,
        ticket: u32,
        connected: u8,
        now: u32,
    ) -> u8;
    #[cfg(not(feature = "rust-app"))]
    fn np_owner_drain(owner: *mut c_void, bridge: *mut c_void);
}

#[derive(Debug)]
pub enum StartFailure {
    Queue,
    Board(Error),
    Task,
    Contract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskNotRunning;

/// Sole UI authority over the already-running native task. No extra native data.
pub struct InputTaskControl<'a> {
    owner: *mut c_void,
    _ui: PhantomData<(&'a mut (), *mut ())>,
}
struct ResumeInputTask(*mut c_void);
impl Drop for ResumeInputTask {
    fn drop(&mut self) {
        // Native scope and task handle still live; acquire succeeded exactly once.
        unsafe { np_owner_resume(self.0) }
    }
}
impl InputTaskControl<'_> {
    pub fn gate(&mut self) -> DevGate<'_> {
        DevGate {
            #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
            owner: self.owner,
            _ui: PhantomData,
        }
    }

    /// Resume on ordinary return, including an error value or a forgotten token.
    /// The short gate view allows USB retirement without lending this controller
    /// back into operation, which would permit a nested suspend/early resume.
    pub fn with_suspended<R>(
        &mut self,
        operation: impl for<'scope> FnOnce(DevGate<'scope>, QuiescedInput<'scope>) -> R,
    ) -> Result<R, TaskNotRunning> {
        if unsafe { np_owner_suspend(self.owner) } == 0 {
            return Err(TaskNotRunning);
        }
        let _resume = ResumeInputTask(self.owner);
        // Fixed-X3 ownership/run contracts exclude other input/native hooks.
        // The private resume guard cannot escape or be forgotten by operation.
        Ok(unsafe {
            with_quiesced_x3_input(|input| {
                operation(
                    DevGate {
                        #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
                        owner: self.owner,
                        _ui: PhantomData,
                    },
                    input,
                )
            })
        })
    }
}

/// UI-only access to the same native DEV gate used by the producer.
pub struct DevGate<'a> {
    #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
    owner: *mut c_void,
    _ui: PhantomData<(&'a mut (), *mut ())>,
}
impl DevGate<'_> {
    /// Shorten this same gate loan without exposing task scheduling authority.
    pub fn reborrow(&mut self) -> DevGate<'_> {
        DevGate {
            #[cfg(any(not(feature = "rust-app"), feature = "dev-usb"))]
            owner: self.owner,
            _ui: PhantomData,
        }
    }
}
#[cfg(not(feature = "rust-app"))]
impl DevGate<'_> {
    /// # Safety
    /// bridge is the UI's sole live native DEV Bridge; no concurrent Bridge
    /// access. Feed synthetic tickets only; physical queue retirement stays in
    /// UiControl. Release builds do not dereference bridge and return false.
    pub unsafe fn consume(
        &mut self,
        bridge: *mut c_void,
        ticket: u32,
        connected: bool,
        now: u32,
    ) -> bool {
        unsafe { np_owner_consume(self.owner, bridge, ticket, u8::from(connected), now) != 0 }
    }
    /// # Safety
    /// Same sole live UI Bridge contract as consume.
    pub unsafe fn drain(&mut self, bridge: *mut c_void) {
        unsafe { np_owner_drain(self.owner, bridge) }
    }
}

/// A function pointer cannot capture a shorter-lived SDK owner. The entry
/// contract still requires the native objects to outlive the whole application.
pub type Application =
    for<'a> fn(Board<'a>, UiQueue<'a>, UiControl<'a>, InputTaskControl<'a>, Probe) -> !;
struct Bootstrap {
    display: *mut c_void,
    input: *mut c_void,
    application: Application,
    error: Option<Error>,
}

/// Returns only a startup failure. Success stays inside application until reset.
/// # Safety
/// Entire Board::acquire_x3 and split_initialized fixed-X3 contracts apply.
/// Native display/InputManager, SDK globals/esp_timer and their exclusion stay
/// valid until program abort/reset. Exactly one entry; all Rust dependencies
/// (including the application) use panic=abort. No longjmp/foreign unwinding,
/// external task suspension/deletion, input alias or counter reset. Suspend
/// only through InputTaskControl; committed sleep retains every owner until
/// reset/rejection restart. After power changes, its operation must not return.
/// Native owner/task APIs must implement task_owner.h, including no ready task
/// on a failed start. This raw task boundary manually audits cross-task access;
/// it does not grant Send/Sync to any endpoint, counter or Board.
pub unsafe fn run(
    display: *mut c_void,
    input: *mut c_void,
    application: Application,
) -> StartFailure {
    let mut bootstrap = Bootstrap {
        display,
        input,
        application,
        error: None,
    };
    // bootstrap is not accessed or moved during the foreign lexical scope.
    let result = unsafe { np_owner_with(body, (&raw mut bootstrap).cast()) };
    match result {
        1 => StartFailure::Queue,
        2 => bootstrap
            .error
            .map_or(StartFailure::Contract, StartFailure::Board),
        3 => StartFailure::Task,
        _ => StartFailure::Contract,
    }
}

extern "C" fn body(owner: *mut c_void, argument: *mut c_void) -> u32 {
    // SAFETY: sole synchronous invocation, argument lives in run's active frame.
    let bootstrap = unsafe { &mut *argument.cast::<Bootstrap>() };
    let (board, input, probe) =
        match unsafe { Board::acquire_x3_optional_sd(bootstrap.display, bootstrap.input) } {
            Ok(value) => value,
            Err(error) => {
                bootstrap.error = Some(error);
                return 2;
            }
        };
    with_task::<core::convert::Infallible>(owner, input, |queue, control, task| {
        (bootstrap.application)(board, queue, control, task, probe)
    })
}

// Keep elimination generic: the pinned compiler rejects an empty match on
// the call expression as unreachable code. No successful return value exists.
trait NeverReturns {
    fn finish(self) -> !;
}
impl NeverReturns for core::convert::Infallible {
    fn finish(self) -> ! { match self {} }
}

fn with_task<R: NeverReturns>(
    owner: *mut c_void,
    input: newt_input_policy::native::InitializedInput<'_>,
    application: impl for<'task> FnOnce(
        UiQueue<'task>, UiControl<'task>, InputTaskControl<'task>,
    ) -> R,
) -> u32 {
    // These two zero-initialized native atoms belong to the immovable C++ scope.
    // Scope return is allowed only if no task started and all Rust loans ended.
    let Some(mut shared) =
        (unsafe { Shared::from_native(np_owner_state(owner), np_owner_dropped(owner)) })
    else {
        return 4;
    };
    let Some((task, queue, control)) = (unsafe {
        split_initialized(
            input,
            np_owner_queue(owner),
            np_owner_gate(owner),
            &mut shared,
        )
    }) else {
        return 4;
    };
    let mut context = Some(task);
    // The address is formed only after final initialization. The input task may
    // run immediately and take the Option. UI never touches context afterwards.
    if unsafe { np_owner_start(owner, task_entry, (&raw mut context).cast()) } == 0 {
        // Failure guarantees no task/context access: ordinary lexical cleanup.
        return 3;
    }
    let task = InputTaskControl {
        owner,
        _ui: PhantomData,
    };
    // This call cannot return. panic=abort prevents *any* destructors on panic.
    // shared/context/native owners remain in active frames until reset/abort.
    application(queue, control, task).finish()
}

extern "C" fn task_entry(context: *mut c_void) {
    // SAFETY: one task entry, sole access, initialized stable Option in body.
    // No invented 'static reference: the erased lifetime is the still-active
    // body scope; its referents cannot end while this never-returning task runs.
    let context = unsafe { &mut *context.cast::<Option<TaskPorts<'_>>>() };
    match context.take() {
        Some(task) => task.run(),
        None => loop {
            core::hint::spin_loop();
        }, // Invalid double entry; fail closed.
    }
}
