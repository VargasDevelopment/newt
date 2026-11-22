// SPDX-License-Identifier: MIT
//! Small, typed defaults and compiler offsets; no navigation storage is exposed.
use super::{Navigation, Projection, ProgressInputs, Target, MAX_TARGETS, STRING_BYTES};
use core::mem::offset_of;

pub const TARGETS: usize = offset_of!(Navigation, targets);
pub const STRINGS: usize = offset_of!(Navigation, strings);
pub const PROJECTION: usize = offset_of!(Navigation, projection);
pub const USED: usize = offset_of!(Navigation, used);
pub const GENERIC_LABEL: usize = offset_of!(Navigation, generic_label);
pub const BIND_ISSUES: usize = offset_of!(Navigation, bind_issues);
pub const COUNT: usize = offset_of!(Navigation, count);
pub const INITIAL_USED: u16 = 1;

#[repr(transparent)]
#[derive(Default)]
pub struct TargetValue(Target);
#[repr(transparent)]
pub struct ProjectionValue(Projection);
impl Default for ProjectionValue {
    fn default() -> Self { Self(Projection::Progress(ProgressInputs::default())) }
}

// Adding a field requires updating the boundary's complete initialization list.
const _: fn(&Navigation) = |Navigation {
    targets: _, strings: _, projection: _, used: _, generic_label: _, bind_issues: _, count: _,
}| {};

impl Default for Navigation {
    fn default() -> Self {
        Self {
            targets: [Target::default(); MAX_TARGETS],
            strings: [0; STRING_BYTES],
            projection: ProjectionValue::default().0,
            used: INITIAL_USED,
            generic_label: 0,
            bind_issues: 0,
            count: 0,
        }
    }
}
