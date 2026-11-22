// SPDX-License-Identifier: MIT
//! Short input views keep scheduling authority outside a suspended operation.
use newt_input_policy::native::QuiescedInput;
use newt_native_input_owner::{DevGate, InputTaskControl, TaskNotRunning};

pub enum TaskAccess<'loan, 'task> {
    Running(&'loan mut InputTaskControl<'task>),
    Suspended(DevGate<'loan>),
}
impl<'task> TaskAccess<'_, 'task> {
    pub fn gate(&mut self) -> DevGate<'_> {
        match self {
            Self::Running(task) => task.gate(),
            Self::Suspended(gate) => gate.reborrow(),
        }
    }
    pub fn reborrow(&mut self) -> TaskAccess<'_, 'task> {
        match self {
            Self::Running(task) => TaskAccess::Running(task),
            Self::Suspended(gate) => TaskAccess::Suspended(gate.reborrow()),
        }
    }
    /// Only the outer running controller may suspend. The callback receives
    /// its gate and the existing exclusion token, never the resume controller.
    pub fn with_suspended<R>(&mut self, operation: impl for<'scope> FnOnce(
        TaskAccess<'scope, 'task>, QuiescedInput<'scope>,
    ) -> R) -> Result<R, TaskNotRunning> {
        match self {
            Self::Running(task) => task.with_suspended(|gate, input| {
                operation(TaskAccess::Suspended(gate), input)
            }),
            Self::Suspended(_) => Err(TaskNotRunning),
        }
    }
}
