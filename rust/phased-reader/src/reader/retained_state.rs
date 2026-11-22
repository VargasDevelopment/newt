// SPDX-License-Identifier: MIT
//! Retain scalar reader state between operation-scoped backing loans.
use super::{Observer, PageInfo, PhasedReader, Proofs, Section, Selection};
use newt_phased_sink::Plan;

/// Opaque reader state with no borrowed backing or operation callback.
pub struct ReaderState {
    pending: Option<(u32, u32, u32, newt_phased_sink::State)>,
    pending_cached: bool,
    pending_undeclared: Option<bool>,
    active: usize,
    source_bytes: u64,
    landscape: bool,
    opened: bool,
    has_page: bool,
    info: PageInfo,
    proofs: Proofs,
    styles_ready: bool,
    undeclared_styles: bool,
    malformed_utf8: bool,
    empty_chapter: bool,
    work: Observer,
}

/// Read-only projection of retained state and its caller-owned selections.
pub struct ReaderView<'a> {
    state: &'a ReaderState,
    selections: &'a [Selection; 2],
}

impl ReaderState {
    pub fn new(landscape: bool) -> Self {
        Self {
            pending: None,
            pending_cached: false,
            pending_undeclared: None,
            active: 0,
            source_bytes: 0,
            landscape,
            opened: false,
            has_page: false,
            info: PageInfo::default(),
            proofs: Proofs::default(),
            styles_ready: false,
            undeclared_styles: false,
            malformed_utf8: false,
            empty_chapter: false,
            work: None,
        }
    }

    /// Return the resulting state before lending the same backing again.
    pub fn lend<'a, 'p>(
        self,
        selections: &'a mut [Selection; 2],
        plan: Option<&'a mut Plan<'p>>,
    ) -> PhasedReader<'a, 'p> {
        let Self {
            pending, pending_cached, pending_undeclared, active, source_bytes,
            landscape, opened, has_page, info, proofs, styles_ready,
            undeclared_styles, malformed_utf8, empty_chapter, work,
        } = self;
        PhasedReader {
            pending, pending_cached, pending_undeclared, selections, plan,
            active, source_bytes, landscape, opened, has_page, info, proofs,
            styles_ready, undeclared_styles, malformed_utf8, empty_chapter,
            on_yield: None,
            work,
        }
    }

    pub fn view<'a>(&'a self, selections: &'a [Selection; 2]) -> ReaderView<'a> {
        ReaderView { state: self, selections }
    }
}

impl PhasedReader<'_, '_> {
    /// End backing and callback loans; retain only the resulting scalar state.
    pub fn into_state(self) -> ReaderState {
        let Self {
            pending, pending_cached, pending_undeclared, selections: _, plan: _,
            active, source_bytes, landscape, opened, has_page, info, proofs,
            styles_ready, undeclared_styles, malformed_utf8, empty_chapter,
            on_yield: _, work,
        } = self;
        ReaderState {
            pending, pending_cached, pending_undeclared, active, source_bytes,
            landscape, opened, has_page, info, proofs, styles_ready,
            undeclared_styles, malformed_utf8, empty_chapter, work,
        }
    }
}

impl ReaderView<'_> {
    pub fn info(&self) -> PageInfo {
        self.state.info
    }

    pub fn opened(&self) -> bool {
        self.state.opened
    }

    pub fn has_page(&self) -> bool {
        self.state.has_page
    }

    pub fn selection(&self) -> &Selection {
        &self.selections[self.state.active]
    }

    pub fn current_section(&self) -> Section<'_> {
        super::projection::current_section(self.has_page(), self.info(), self.selection())
    }

    pub fn malformed_utf8(&self) -> bool {
        self.state.malformed_utf8
    }
}
