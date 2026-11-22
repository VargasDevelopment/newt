// SPDX-License-Identifier: MIT
//! Receipt selection and its required optional deferred-verification admission.
use super::{BookSource, Digest, ImportedRevision};

impl<'a> ImportedRevision<'a> {
    /// Frozen selectRevision: only a full source hash can accept a loaded
    /// receipt. Read/cancel failure never becomes permission to reimport.
    pub fn select_source(
        &mut self,
        source: &'a mut dyn BookSource,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
    ) -> Option<(bool, Digest)> {
        self.select_source_inner(source, scratch, allowed, None, None)
    }

    /// The actual caller may admit a loaded receipt for deferred verification.
    /// A refusal continues the same full-hash/reimport body without reloading.
    pub fn select_source_admitted(
        &mut self,
        source: &'a mut dyn BookSource,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
        admit: &mut dyn FnMut(&mut dyn BookSource, &Digest) -> bool,
    ) -> Option<(bool, Digest)> {
        self.select_source_inner(source, scratch, allowed, Some(admit), None)
    }

    /// The slow path can use the action's scratch before hashing or importing.
    /// Successful deferred admission leaves that scratch role untouched.
    pub fn select_source_admitted_before_work(
        &mut self,
        source: &'a mut dyn BookSource,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
        admit: &mut dyn FnMut(&mut dyn BookSource, &Digest) -> bool,
        before: &mut dyn FnMut(&mut [u8]),
    ) -> Option<(bool, Digest)> {
        self.select_source_inner(source, scratch, allowed, Some(admit), Some(before))
    }

    fn select_source_inner(
        &mut self,
        source: &'a mut dyn BookSource,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
        mut admit: Option<&mut dyn FnMut(&mut dyn BookSource, &Digest) -> bool>,
        mut before: Option<&mut dyn FnMut(&mut [u8])>,
    ) -> Option<(bool, Digest)> {
        self.begin_deferred_path_checks();
        let selected = self.lend_action_storage(scratch).and_then(|mut action| {
            let loaded = action.load(source);
            let mut current = [0; 32];
            if loaded && let Some(admit) = admit.as_deref_mut() {
                current = *action.digest();
                let admitted = action.core.source.as_deref_mut()
                    .is_some_and(|source| admit(source, &current));
                current.fill(0);
                if admitted {
                    return (allowed() && action.good()).then(|| (false, *action.digest()));
                }
            }
            if let Some(before) = before.as_deref_mut() {
                before(&mut *action.scratch);
            }
            if loaded && (!action.hash_complete_source(allowed, &mut current) || !allowed()) {
                return None;
            }
            if loaded && current == *action.digest() && action.good() {
                return Some((false, *action.digest()));
            }
            action.reimport().then(|| (true, *action.digest()))
        });
        let live = self.finish_deferred_path_checks();
        selected.filter(|_| live)
    }

    /// Re-enter the same selection policy using the retained genuine source;
    /// a reset receipt is never presented as an imported revision.
    pub fn select_held_source(
        &mut self,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
    ) -> Option<(bool, Digest)> {
        let source = self.core.source.take()?;
        self.select_source(source, scratch, allowed)
    }
}
