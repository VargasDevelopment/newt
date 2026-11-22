// SPDX-License-Identifier: MIT
//! Y restores durable/retained state before input begin, queue and task start.
use super::{InputTaskControl, StartFailure, with_task};
use core::{convert::Infallible, ffi::c_void, marker::PhantomData};
use newt_c3_board::{Board, PendingInput, Probe};
use newt_input_policy::{UiControl, native::UiQueue};

// SAFETY: the native scope holds the same existing immovable Owner. The queue
// allocation is the frozen xQueueCreate, deferred until Rust finishes restore.
unsafe extern "C" {
    fn np_owner_with_deferred(
        body: extern "C" fn(*mut c_void, *mut c_void) -> u32,
        argument: *mut c_void,
    ) -> u32;
    fn np_owner_create_queue(owner: *mut c_void) -> u8;
}

/// Sole right to start the existing input queue/task inside its native scope.
/// It holds one pointer, without another input owner, queue or allocation.
pub struct InputStart<'owner> {
    owner: *mut c_void,
    _scope: PhantomData<(&'owner mut (), *mut ())>,
}
impl InputStart<'_> {
    /// The application calls this after durable/RTC restore. Success never
    /// returns, preserving the task's native owner and Rust context frames.
    pub fn run(
        self,
        input: PendingInput<'_>,
        application: impl for<'task> FnOnce(
            UiQueue<'task>, UiControl<'task>, InputTaskControl<'task>,
        ) -> Infallible,
    ) -> StartFailure {
        let input = match input.begin() {
            Ok(input) => input,
            Err(error) => return StartFailure::Board(error),
        };
        // SAFETY: self is the sole token for the active native Owner. No task
        // or queue exists; consuming self prevents a second queue/start call.
        if unsafe { np_owner_create_queue(self.owner) } == 0 {
            return StartFailure::Queue;
        }
        match with_task::<Infallible>(self.owner, input, application) {
            3 => StartFailure::Task,
            _ => StartFailure::Contract,
        }
    }
}

/// Returns only on failure before task launch; application success stays live.
pub type DeferredApplication = for<'sdk, 'owner> fn(
    Board<'sdk>, PendingInput<'sdk>, InputStart<'owner>, Probe,
) -> StartFailure;

struct Bootstrap<F> {
    application: Option<F>,
    failure: Option<StartFailure>,
}

/// # Safety
/// The full task_owner::run fixed-X3, exclusive native owner and panic=abort
/// contracts apply. Restore and input-start work stay in the native scope.
/// The callback may return only without a live task. No foreign unwind.
pub unsafe fn run_deferred<F>(
    display: *mut c_void,
    input: *mut c_void,
    application: F,
) -> StartFailure
where
    F: for<'sdk, 'owner> FnOnce(
        Board<'sdk>, PendingInput<'sdk>, InputStart<'owner>, Probe,
    ) -> StartFailure,
{
    with_owner(|start| {
        // SAFETY: the enclosing entry grants Board's fixed-X3 ownership contract.
        match unsafe { Board::acquire_x3_optional_sd_deferred_input(display, input) } {
            Ok((board, input, probe)) => application(board, input, start, probe),
            Err(error) => StartFailure::Board(error),
        }
    })
}

/// Enter the existing deferred native owner after panel acquisition and any
/// update-mode decision. The Board/input pair moves here without reacquisition.
/// Queue creation and input/task startup still occur only in InputStart::run.
///
/// # Safety
/// The full run_deferred task lifetime, exclusive SDK and panic=abort contracts
/// apply. Board and PendingInput must be the pair from the sole acquisition;
/// neither input nor a queue/task may have been started independently. The
/// callback may return only without a live task. No foreign unwind.
pub unsafe fn run_acquired_deferred<'sdk, F>(
    board: Board<'sdk>,
    input: PendingInput<'sdk>,
    probe: Probe,
    application: F,
) -> StartFailure
where
    F: for<'owner> FnOnce(
        Board<'sdk>, PendingInput<'sdk>, InputStart<'owner>, Probe,
    ) -> StartFailure,
{
    with_owner(|start| application(board, input, start, probe))
}

fn with_owner<F>(application: F) -> StartFailure
where
    F: for<'owner> FnOnce(InputStart<'owner>) -> StartFailure,
{
    let mut bootstrap = Bootstrap { application: Some(application), failure: None };
    // SAFETY: one synchronous callback; bootstrap cannot move or be accessed
    // outside its scope. Both public entries require the exclusive native/task
    // contract and panic=abort; InputStart alone may create the queue/task.
    let _ = unsafe { np_owner_with_deferred(body::<F>, (&raw mut bootstrap).cast()) };
    bootstrap.failure.unwrap_or(StartFailure::Contract)
}

extern "C" fn body<F>(owner: *mut c_void, argument: *mut c_void) -> u32
where
    F: for<'owner> FnOnce(InputStart<'owner>) -> StartFailure,
{
    // SAFETY: with_owner's sole callback; argument is its live Bootstrap.
    let bootstrap = unsafe { &mut *argument.cast::<Bootstrap<F>>() };
    bootstrap.failure = Some(match bootstrap.application.take() {
        Some(application) => application(InputStart { owner, _scope: PhantomData }),
        None => StartFailure::Contract,
    });
    4
}
