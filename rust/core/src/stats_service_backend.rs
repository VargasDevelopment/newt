// SPDX-License-Identifier: MIT
//! Minimal service seam. Physical backends and their enumeration remain excluded.
use crate::reading_stats_store::StatsBackend;

pub trait NamespaceBackend: StatsBackend + Sized {
    /// Same order, short-circuit and failure semantics as forEachNamespace.
    /// Names must remain stable for this callback only. No transaction is open
    /// when calling it. A false callback stops enumeration and returns false.
    /// Implementors must lend an exclusive reborrow, never alias the visitor's
    /// backend access. Cursors may be local values; no self-referential cursor.
    fn for_each_namespace(
        &mut self,
        prefix: &str,
        visitor: &mut dyn FnMut(&mut Self, &str) -> bool,
    ) -> bool;
}
