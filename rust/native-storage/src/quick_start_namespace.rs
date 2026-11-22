// SPDX-License-Identifier: MIT
//! Quick Start namespace admission (`newt_quick`) beside Core's routing grammar.
//! Both existing router and SD-store admission use this predicate. Core keeps
//! its immutable legacy grammar; no backend or parallel routing is introduced.
pub(super) use newt_core::reading_state_namespace::{numbered_namespace, stats_namespace};
pub(super) fn reading_state_namespace(name: &str) -> bool {
    name == "newt_quick" || newt_core::reading_state_namespace::reading_state_namespace(name)
}
