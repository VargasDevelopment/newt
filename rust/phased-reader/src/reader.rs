use crate::{
    admission::{self, ChapterSource, Observer, ScheduledSource, WorkPhase, WorkScope},
    canonical,
    package_parsers::{self as package, TocEntry, TocKind, Workflow},
    resources::{self, LayoutResources, Lease, StyleResources},
    styles::{self, Slot, Sources, State as StyleState},
    watch::Watch,
    xml_session::{self, Attributes, Handler, ParserBackend, XmlStream},
    zip_container::{Status, hash_path},
    zip_window::{arena::Arena, source::BookSource},
};
use newt_core::{
    member_check::Checked,
    chapter_layout::{
        engine::Engine,
        session::{ChapterSession, LayoutHandler},
        types::Params,
    },
    css::{CssStylesheetBuilder, TextAlign},
};
pub use newt_core::phased_metadata::BookLanguage;
pub use newt_core::preferences::PageLayout;
use newt_phased_sink::{Frame, Output, Plan, SelectingSink, Target};
use newt_reader_atlas::{Geometry, ReaderAtlas};
use newt_reader_navigation::{MenuInfo, MenuRow, NO_TOC, Navigation, Section, metadata::Metadata};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PageInfo {
    pub spine: u32,
    pub page: u32,
    pub page_count: u32,
    pub char_start: u32,
    pub total_chars: u32,
    pub book_char_start: u64,
    pub book_total_chars: u64,
    pub overall_percent: u8,
    pub overall_known: bool,
    pub overall_estimated: bool,
}
/// Initialize directly in caller storage; no owned arena or selection allocation.
pub struct Selection {
    /// Checked when the chapter was selected; the producer and cache reuse it.
    pub entry: Checked,
    href: [u8; 512],
    title: [u8; 256],
    pub language: BookLanguage,
    pub spines: u32,
    pub navigation: Navigation,
}
#[path = "reader/selection_initialization.rs"]
pub mod selection_initialization;
#[path = "reader/scoped_yield.rs"]
mod scoped_yield;
#[path = "reader/action_trace.rs"]
mod action_trace;
#[path = "reader/retained_state.rs"]
mod retained_state;
pub use retained_state::{ReaderState, ReaderView};

impl Selection {
    pub fn href(&self) -> &[u8] {
        crate::xml_input::c_bytes(&self.href)
    }
    pub fn title(&self) -> &[u8] {
        crate::xml_input::c_bytes(&self.title)
    }
    fn copy_from(&mut self, other: &Self) {
        self.entry = other.entry;
        self.href.copy_from_slice(&other.href);
        self.title.copy_from_slice(&other.title);
        self.spines = other.spines;
        self.language = other.language;
        self.navigation.clone_from(&other.navigation);
    }
    fn clear(&mut self) {
        self.entry = Checked::default();
        self.href.fill(0);
        self.title.fill(0);
        self.spines = 0;
        self.language = BookLanguage::default();
        self.navigation.reset();
    }
}
#[derive(Default, Debug)]
pub(crate) struct Proofs {
    pub plan: bool,
    pub chapter: bool,
    pub selection: bool,
}
impl Proofs {
    pub fn invalidate(&mut self) {
        self.plan = false;
        self.chapter = false;
        self.selection = false;
    }
}

/// Borrowed view of the inactive menu projection; labels are navigation-owned.
pub struct ChapterMenuView<'a> {
    navigation: &'a Navigation,
}
impl ChapterMenuView<'_> {
    pub fn fallback(&self) -> crate::ContentsFallback { self.navigation.contents_fallback() }
    pub fn info(&self) -> MenuInfo {
        self.navigation.menu_info()
    }
    pub fn row(&self, index: u32) -> MenuRow<'_> {
        self.navigation.menu_row(index)
    }
}
impl<'a> ChapterMenuView<'a> {
    pub(crate) fn from_navigation(navigation: &'a Navigation) -> Self {
        Self { navigation }
    }
}

/// Source capabilities are operation-scoped. The revision owner binds these
/// calls to the same immutable revision until close (including unread suffixes).
/// Retained byte count/probes detect live failures, not same-size substitution.
/// Callbacks may schedule, never replace bytes, reenter, or change any loan.
/// Reentrant calls into a live owner are unsupported.
pub struct PhasedReader<'a, 'p> {
    pending: Option<(u32, u32, u32, newt_phased_sink::State)>,
    pending_cached: bool,
    pending_undeclared: Option<bool>,
    selections: &'a mut [Selection; 2],
    plan: Option<&'a mut Plan<'p>>,
    active: usize,
    source_bytes: u64,
    page_layout: PageLayout,
    opened: bool,
    has_page: bool,
    info: PageInfo,
    proofs: Proofs,
    styles_ready: bool,
    undeclared_styles: bool,
    pub malformed_utf8: bool,
    pub empty_chapter: bool,
    pub on_yield: newt_core::admission::Yield<'a>,
    pub work: Observer,
    #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
    last_layout_pages: u32,
    #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
    total_layout_pages: u64,
}
impl<'a, 'p> PhasedReader<'a, 'p> {
    pub fn new(
        selections: &'a mut [Selection; 2],
        plan: Option<&'a mut Plan<'p>>,
        page_layout: PageLayout,
    ) -> Self {
        ReaderState::new(page_layout).lend(selections, plan)
    }
    /// Owner closes/checkpoints before a confirmed settings reflow.
    pub fn set_closed_layout(&mut self, page_layout: PageLayout) {
        if !self.opened {
            self.page_layout = page_layout;
        }
    }
    pub fn info(&self) -> PageInfo {
        self.info
    }
    pub fn opened(&self) -> bool {
        self.opened
    }
    pub fn has_page(&self) -> bool {
        self.has_page
    }
    pub fn book_language(&self) -> Option<BookLanguage> {
        self.opened.then(|| self.selection().language)
    }
    pub fn selection(&self) -> &Selection {
        &self.selections[self.active]
    }
    pub fn staged_selection(&self) -> &Selection {
        &self.selections[self.active ^ 1]
    }
    #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
    pub fn last_layout_pages(&self) -> u32 {
        self.last_layout_pages
    }
    #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
    pub fn total_layout_pages(&self) -> u64 {
        self.total_layout_pages
    }
    #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
    pub fn reset_layout_pages(&mut self) {
        self.last_layout_pages = 0;
        self.total_layout_pages = 0;
    }
    #[inline]
    pub(crate) fn reset_last_layout_pages(&mut self) {
        #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
        {
            self.last_layout_pages = 0;
        }
    }
    /// Composition only. Drop caller parse views after return, before repaint.
    /// An unpublished capture owns the inactive selection until publish.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_chapter_menu<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        requested_focus: u32,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        directory: &mut [u8; 512],
        mapping: &mut [u32],
        lease: Option<&Lease>,
    ) -> Status {
        if !self.opened || !self.has_page || self.plan.is_none() || self.pending.is_some() {
            return Status::Unsupported;
        }
        let (left, right) = self.selections.split_at_mut(1);
        let (selected, staged) = if self.active == 0 {
            (&left[0], &mut right[0])
        } else {
            (&right[0], &mut left[0])
        };
        let current = selected.navigation.section(self.info.char_start);
        let navigation = &mut staged.navigation;
        if navigation.reuse_menu(current, requested_focus) {
            return Status::Ok;
        }
        navigation.invalidate_menu();
        let held = match lease {
            Some(lease) => match lease.hold() {
                Ok(held) => held,
                Err(e) => return e,
            },
            None => return Status::OutOfMemory,
        };
        let mut status = validate(source, self.source_bytes);
        self.styles_ready = false;
        metadata.reset();
        parse.reset();
        {
            let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Metadata);
            if status == Status::Ok {
                status = match bind_menu_source(
                    source,
                    selected.spines,
                    navigation,
                    current,
                    requested_focus,
                    metadata,
                    r,
                    toc,
                    directory,
                    mapping,
                    self.on_yield,
                    self.work,
                ) {
                    Ok(()) => Status::Ok,
                    Err(e) => e,
                };
            }
        }
        // No CanonicalBook, XML session or arena-backed menu labels survive.
        metadata.reset();
        parse.reset();
        drop(held);
        if status == Status::Ok {
            status = validate(source, self.source_bytes);
        }
        if status != Status::Ok {
            navigation.invalidate_menu();
        }
        status
    }
    /// Borrowed observation for plan/text comparison; candidate data is not a
    /// publication receipt and must not be used after a failed checked exit.
    pub fn page_plan(&self) -> Option<&Plan<'p>> {
        if self.pending.is_some() || self.proofs.plan {
            self.plan.as_deref()
        } else {
            None
        }
    }
    pub fn has_retained_page_plan(&self) -> bool {
        self.plan.is_some() && self.proofs.plan
    }
    pub fn close(&mut self) {
        self.pending = None;
        self.pending_cached = false;
        self.pending_undeclared = None;
        self.proofs.invalidate();
        self.styles_ready = false;
        self.undeclared_styles = false;
        self.opened = false;
        self.has_page = false;
        self.source_bytes = 0;
        self.active = 0;
        self.info = PageInfo::default();
        self.malformed_utf8 = false;
        self.empty_chapter = false;
        #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
        {
            self.last_layout_pages = 0;
            self.total_layout_pages = 0;
        }
        for s in self.selections.iter_mut() {
            s.clear();
        }
    }
    fn failed(&mut self, status: Status) -> Status {
        self.pending = None;
        self.pending_cached = false;
        self.pending_undeclared = None;
        self.selections[self.active ^ 1]
            .navigation
            .invalidate_menu();
        self.proofs.invalidate();
        status
    }
    fn layout_failed(&mut self, resolving: bool, status: Status) -> Status {
        if resolving {
            self.proofs.chapter = false;
            self.proofs.selection = false;
            self.selections[self.active ^ 1]
                .navigation
                .invalidate_menu();
            status
        } else {
            self.failed(status)
        }
    }
    pub fn source_available<S: BookSource + ?Sized>(&mut self, source: &mut S) -> bool {
        self.validate(source) == Status::Ok
    }
    fn validate<S: BookSource + ?Sized>(&self, source: &mut S) -> Status {
        validate(source, self.source_bytes)
    }
    /// Canonical resources and typed style storage are caller-owned. No Book is
    /// alive during reset, style resolution, lease return or publication.
    #[allow(clippy::too_many_arguments)]
    pub fn open<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        self.close();
        if self.plan.is_some() && lease.is_none() {
            return Status::OutOfMemory;
        }
        let bytes = source.size();
        if bytes == 0 {
            return Status::NotZip;
        }
        if bytes > admission::EXTERNAL_MAX_BYTES {
            return Status::Unsupported;
        }
        let held = match lease.map(Lease::hold).transpose() {
            Ok(h) => h,
            Err(e) => return e,
        };
        let status = {
            let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Admission);
            admission::admit(source, metadata, parse, r, self.on_yield, self.work)
        };
        if status != Status::Ok {
            return status;
        }
        let result = select(
            source,
            0,
            &mut self.selections[self.active ^ 1],
            metadata,
            r,
            toc,
            sr,
            sources,
            slot,
            self.on_yield,
            self.work,
        );
        metadata.reset();
        parse.reset();
        let undeclared = match result {
            Ok(u) => u,
            Err(e) => return e,
        };
        drop(held);
        let status = validate(source, bytes);
        if status != Status::Ok {
            return status;
        }
        self.active ^= 1;
        self.source_bytes = bytes;
        self.opened = true;
        self.styles_ready = slot.state != StyleState::Empty;
        self.undeclared_styles = undeclared;
        self.proofs.selection = true;
        Status::Ok
    }
    /// mode=Page or Anchor for ordinary turns. TOC modes are driven by
    /// show_chapter so a hash-only target can never publish an exact-ID jump.
    #[allow(clippy::too_many_arguments)]
    pub fn show<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        target: u32,
        mode: Target,
        frame: &mut Frame,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        self.reset_last_layout_pages();
        if matches!(mode, Target::TocResolve | Target::TocPage) {
            return self.failed(Status::Unsupported);
        }
        let status = self.layout(
            source,
            spine,
            target,
            mode,
            Some(frame),
            metadata,
            parse,
            r,
            toc,
            sr,
            sources,
            slot,
            lr,
            lease,
        );
        if status == Status::Ok {
            self.publish(source, frame)
        } else {
            status
        }
    }
    pub fn capture<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        target: u32,
        mode: Target,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        self.reset_last_layout_pages();
        if self.plan.is_none() {
            return Status::Unsupported;
        }
        if matches!(mode, Target::TocResolve | Target::TocPage) {
            return self.failed(Status::Unsupported);
        }
        let status = self.layout(
            source, spine, target, mode, None, metadata, parse, r, toc, sr, sources, slot, lr,
            lease,
        );
        status
    }
    #[allow(clippy::too_many_arguments)]
    fn layout<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        target: u32,
        mode: Target,
        frame: Option<&mut Frame>,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        self.pending = None;
        self.pending_cached = false;
        self.pending_undeclared = None;
        let mut frame = frame;
        let resolving = mode == Target::TocResolve;
        let required = if resolving {
            target as u16
        } else if mode == Target::TocPage {
            self.selections[self.active ^ 1]
                .navigation
                .staged_required_toc()
        } else {
            NO_TOC
        };
        if !resolving {
            self.proofs.plan = false;
        }
        self.malformed_utf8 = false;
        self.empty_chapter = false;
        if !self.opened {
            return self.layout_failed(resolving, Status::NotFound);
        }
        if self.plan.is_some() && lease.is_none() {
            return self.layout_failed(resolving, Status::OutOfMemory);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.layout_failed(resolving, status);
        }
        let reuse = self.proofs.chapter && self.has_page && self.info.spine == spine;
        if reuse && mode == Target::Page && target >= self.info.page_count {
            return self.layout_failed(resolving, Status::NotFound);
        }
        let held = match lease.map(Lease::hold).transpose() {
            Ok(h) => h,
            Err(e) => return self.layout_failed(resolving, e),
        };
        let inactive = self.active ^ 1;
        if !matches!(mode, Target::TocResolve | Target::TocPage)
            && self.proofs.selection
            && self.info.spine == spine
            && (self.styles_ready && slot.spine == spine && slot.state != StyleState::Empty
                || self.undeclared_styles)
        {
            let (left, right) = self.selections.split_at_mut(1);
            if self.active == 0 {
                right[0].copy_from(&left[0]);
            } else {
                left[0].copy_from(&right[0]);
            }
            if !(self.styles_ready && slot.spine == spine && slot.state != StyleState::Empty) {
                slot.reset(spine, StyleState::Deferred, 0);
                self.styles_ready = true;
            }
        } else {
            self.styles_ready = false;
            self.selections[inactive]
                .navigation
                .stage_required_toc(required);
            let result = select(
                source,
                spine,
                &mut self.selections[inactive],
                metadata,
                r,
                toc,
                sr,
                sources,
                slot,
                self.on_yield,
                self.work,
            );
            metadata.reset();
            parse.reset();
            match result {
                Ok(u) => {
                    self.styles_ready = true;
                    self.undeclared_styles = u;
                }
                Err(e) => return self.layout_failed(resolving, e),
            }
        }
        let mut passes = 0;
        let outcome = loop {
            passes += 1;
            if passes > 3
                || !self.styles_ready
                || slot.spine != spine
                || slot.state == StyleState::Empty
            {
                break Err(Status::OutOfMemory);
            }
            let selected = &mut self.selections[inactive];
            if !reuse || matches!(mode, Target::TocResolve | Target::TocPage) {
                selected.navigation.begin_anchors();
            }
            let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Layout);
            let result = pass(
                source,
                selected,
                target,
                mode,
                self.page_layout,
                frame.as_deref_mut(),
                self.plan.as_deref_mut(),
                slot,
                lr,
                r,
                reuse,
                self.on_yield,
            );
            // All live XML, inflate, Engine and sink loans have ended here.
            metadata.reset();
            parse.reset();
            let (mut status, mut pages, mut chars, complete, state, verdict) = match result {
                Ok(v) => v,
                Err(e) => break Err(e),
            };
            #[cfg(any(test, feature = "page-turn-trace", not(target_os = "none")))]
            {
                self.last_layout_pages = self.last_layout_pages.saturating_add(pages);
                self.total_layout_pages = self.total_layout_pages.saturating_add(u64::from(pages));
            }
            if verdict.stopped {
                status = settle_stopped_style(
                    source,
                    selected,
                    spine,
                    metadata,
                    r,
                    sr,
                    sources,
                    slot,
                    verdict,
                    self.on_yield,
                );
                if status != Status::Ok {
                    self.styles_ready = false;
                    break Err(status);
                }
                continue;
            }
            if status == Status::Ok && slot.issues & styles::MALFORMED != 0 {
                self.styles_ready = false;
                break Err(Status::OutOfMemory);
            }
            if status == Status::Ok && verdict.cleared {
                slot.state = StyleState::Settled;
            }
            if status != Status::Ok {
                break Err(status);
            }
            if reuse {
                if pages > self.info.page_count
                    || complete && (pages != self.info.page_count || chars != self.info.total_chars)
                {
                    break Err(Status::Stale);
                }
                pages = self.info.page_count;
                chars = self.info.total_chars;
            }
            if !complete && !reuse {
                break Err(Status::Truncated);
            }
            if pages == 0 {
                self.empty_chapter = true;
                break Err(Status::NotFound);
            }
            if !state.found {
                break Err(Status::NotFound);
            }
            if resolving {
                let section = selected.navigation.target_by_toc(required);
                if !section.target_known || !section.anchor_known {
                    break Err(Status::NotFound);
                }
                selected.navigation.stage_resolved_page(state.page_index);
            } else {
                if mode == Target::TocPage {
                    let section = selected.navigation.target_by_toc(required);
                    if !section.target_known || !section.anchor_known {
                        break Err(Status::NotFound);
                    }
                }
                self.malformed_utf8 = state.malformed;
                if state.malformed {
                    break Err(Status::ParseError);
                }
            }
            break Ok((pages, chars, state));
        };
        drop(held); // actual parser loans have ended BEFORE repaint or publication
        let (pages, chars, state) = match outcome {
            Ok(v) => v,
            Err(e) => return self.layout_failed(resolving, e),
        };
        if resolving {
            return Status::Ok;
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.layout_failed(resolving, status);
        }
        self.pending = Some((spine, pages, chars, state));
        Status::Ok
    }
    /// Call only after dropping actual parse resources. The frame may now reuse
    /// their physical owner; no parse loan or source loan is retained here.
    pub fn publish<S: BookSource + ?Sized>(&mut self, source: &mut S, frame: &mut Frame) -> Status {
        let Some((spine, pages, chars, state)) = self.pending.take() else {
            return Status::NotFound;
        };
        let status = self.validate(source);
        if status != Status::Ok {
            return self.failed(status);
        }
        if let Some(plan) = self.plan.as_deref() {
            plan.paint(frame, self.page_layout.landscape, &ReaderAtlas, &mut ReaderAtlas);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.failed(status);
        }
        self.active ^= 1;
        self.info = PageInfo {
            spine,
            page: state.page_index,
            page_count: pages,
            char_start: state.char_start,
            total_chars: chars,
            ..PageInfo::default()
        };
        self.estimate();
        self.has_page = true;
        self.proofs.plan = self.plan.is_some();
        self.proofs.chapter = !self.pending_cached;
        if self.pending_cached {
            self.styles_ready = false;
            if let Some(undeclared) = self.pending_undeclared.take() {
                self.undeclared_styles = undeclared;
            }
        }
        self.pending_cached = false;
        self.proofs.selection = true;
        Status::Ok
    }
    fn estimate(&mut self) {
        let estimate = self
            .selection()
            .navigation
            .progress_inputs()
            .estimate(self.info.spine, self.info.total_chars);
        if let Some(percent) = estimate.percent(self.info.char_start) {
            self.info.overall_percent = percent;
            self.info.overall_known = true;
            self.info.overall_estimated = true;
        }
    }
    /// Caller must pass the receipt from the immediately previous publication.
    pub fn discard_chapter_publication(&mut self, previous: PageInfo, had_page: bool) {
        self.pending = None;
        self.pending_cached = false;
        self.pending_undeclared = None;
        self.active ^= 1;
        self.info = previous;
        self.has_page = had_page;
        self.malformed_utf8 = false;
        self.empty_chapter = false;
        self.proofs.invalidate();
        self.styles_ready = false;
    }
    /// Returns Unsupported when there is no retained plan; the caller can use
    /// show(Page) with its actual resources for the original layout fallback.
    pub fn repaint<S: BookSource + ?Sized>(&mut self, source: &mut S, frame: &mut Frame) -> Status {
        if !self.opened || !self.has_page {
            return self.failed(Status::NotFound);
        }
        if !self.proofs.plan {
            return self.failed(Status::Unsupported);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.failed(status);
        }
        if let Some(plan) = self.plan.as_deref() {
            plan.paint(frame, self.page_layout.landscape, &ReaderAtlas, &mut ReaderAtlas);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.failed(status);
        }
        Status::Ok
    }
}
pub(crate) use crate::zip_window::source::validate;

struct NavigationSource<'a, S: BookSource + ?Sized> {
    source: &'a mut S,
    bytes: u64,
    failed: bool,
}
impl<S: BookSource + ?Sized> BookSource for NavigationSource<'_, S> {
    fn size(&self) -> u64 {
        self.source.size()
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let n = self.source.read_at(at, out);
        self.failed |= n < 0 || (!out.is_empty() && at < self.bytes && n == 0);
        n
    }
}
impl<S: BookSource + ?Sized> NavigationSource<'_, S> {
    fn guard(&mut self) -> Status {
        if self.size() != self.bytes {
            return Status::Stale;
        }
        let n = self.read_at(0, &mut []);
        if self.size() != self.bytes {
            Status::Stale
        } else if self.failed || n != 0 {
            Status::IoError
        } else {
            Status::Ok
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_menu_source<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    expected_spines: u32,
    navigation: &mut Navigation,
    current: Section<'_>,
    requested_focus: u32,
    arena: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    directory: &mut [u8; 512],
    mapping: &mut [u32],
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
) -> Result<(), Status> {
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let book = canonical::open(&mut scheduled, arena, r, false, on_yield, observer)?;
    bind_menu_book(book, expected_spines, navigation,
        directory, toc, |book, navigation| {
            bind_menu_projection(book, navigation, current, requested_focus, mapping)
        })
}

/// An optional TOC failure ends the open only when the book source itself
/// failed. Anything about the TOC member (damaged, unparsable, too big)
/// degrades to no TOC, like a missing one. Exhaustive so a new status needs a
/// decision here.
pub(crate) fn optional_toc_ends_open(status: Status) -> bool {
    match status {
        Status::IoError | Status::Stale => true,
        Status::Ok
        | Status::NotZip
        | Status::NotEpub
        | Status::Encrypted
        | Status::Truncated
        | Status::Unsupported
        | Status::OutOfMemory
        | Status::ParseError
        | Status::NotFound
        | Status::MetadataTooLarge
        | Status::PathTooLong
        | Status::Damaged => false,
    }
}

pub(crate) fn contents_fallback(authored: bool, status: Status) -> crate::ContentsFallback {
    use crate::ContentsFallback;
    if !authored || status == Status::Ok { ContentsFallback::None }
    else if status == Status::OutOfMemory { ContentsFallback::Capacity }
    else { ContentsFallback::Invalid }
}

// Both physical and disjoint storage use canonical counted TOC parsing and the
// same optional-navigation degradation/live-source policy on the same Book.
fn bind_menu_book<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    book: canonical::Book<'_, '_, S, B>, expected_spines: u32,
    navigation: &mut Navigation,
    directory: &mut [u8; 512], toc_backing: &mut (impl package::TocBacking + ?Sized),
    mut bind: impl FnMut(&Metadata<'_, '_>, &mut Navigation) -> bool,
) -> Result<(), Status> {
    if book.package().spine_count != expected_spines as usize { return Err(Status::Stale); }
    let authored = book.navigation_item().is_some();
    let declaration = book.navigation_item().and_then(|m| m.href)
        .and_then(|h| book.arena().c_string(h)).and_then(|h| book.catalog().find(h))
        .map(|(_, e)| (e, book.navigation_is_epub3()));
    let package = *book.package();
    let entry_count = book.catalog().len();
    let canonical::BookParts { mut workflow, arena, entries, manifest, spine, directory_offset, .. } =
        book.into_parts();
    let mark = arena.mark();
    let bytes = workflow.source.size();
    let mut guarded = NavigationSource { source: &mut *workflow.source, bytes, failed: false };
    let live = guarded.guard();
    if live != Status::Ok { return Err(live); }
    let mut project = |arena: &Arena<'_>, toc: &[TocEntry]| {
        bind(&Metadata { arena, package: &package, manifest: &manifest[..package.manifest_count],
            spine: &spine[..package.spine_count], toc, entries: &entries[..entry_count],
            directory_offset }, navigation)
    };
    let optional = if let Some((entry, epub3)) = declaration {
        let href = entry.name.and_then(|n| arena.c_string(n)).map(crate::xml_input::c_bytes);
        // A damaged TOC member degrades like an unparsable one.
        let checked = canonical::CatalogView::new(&entries[..entry_count], arena, directory_offset)
            .check(&mut guarded, &entry);
        if let Err(status) = checked {
            Err(status)
        } else if let (Ok(entry), true) =
            (checked, href.is_some_and(|h| crate::xml_input::dir_name(h, directory)))
        {
            let mut nav = Workflow {
                policy: workflow.policy, source: &mut guarded, backend: &mut *workflow.backend,
                decoder: resources::decoder(&mut workflow.decoder), buffer: workflow.buffer.as_deref_mut(),
                filter: workflow.filter.as_deref_mut() };
            let kind = if epub3 { TocKind::Nav } else { TocKind::Ncx };
            package::count_toc(&mut nav, &entry, crate::xml_input::c_bytes(directory), arena, kind)
                .and_then(|count| toc_backing.with_toc(arena, count, |arena, toc| {
                    package::fill_toc(&mut nav, &entry, crate::xml_input::c_bytes(directory), arena, kind, toc)
                        .map(|used| project(arena, &toc[..used]))
                })?)
        } else { Err(Status::Unsupported) }
    } else { Err(Status::Unsupported) };
    let optional = optional.and_then(|bound| bound.then_some(()).ok_or(Status::OutOfMemory));
    let (bound, optional) = match optional {
        Ok(()) => (true, Status::Ok),
        Err(status) => (project(arena, &[]), status),
    };
    arena.release(mark);
    let live = guarded.guard();
    if live != Status::Ok { return Err(live); }
    if optional_toc_ends_open(optional) { return Err(optional); }
    if !bound { navigation.invalidate_menu(); return Err(Status::Unsupported); }
    navigation.set_contents_fallback(contents_fallback(authored, optional));
    Ok(())
}
fn bind_menu_projection(
    book: &Metadata<'_, '_>, navigation: &mut Navigation, current: Section<'_>,
    focus: u32, mapping: &mut [u32],
) -> bool {
    mapping.len() >= book.toc.len() && navigation.bind_menu(book, current, focus, mapping)
}

#[allow(clippy::too_many_arguments)]
fn select<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    spine: u32,
    selected: &mut Selection,
    arena: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    slot: &mut Slot<'_>,
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
) -> Result<bool, Status> {
    select_mode(
        source, spine, selected, arena, r, toc, sr, sources, slot, on_yield, observer, false,
    )
}

fn select_mode<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    spine: u32,
    selected: &mut Selection,
    arena: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    slot: &mut Slot<'_>,
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
    plain: bool,
) -> Result<bool, Status> {
    let _work = WorkScope::new_scoped(on_yield, observer, WorkPhase::Metadata);
    let (undeclared, resolved) = select_metadata(
        source, spine, selected, arena, r, toc, sr, sources, on_yield, observer, plain,
    )?;
    if plain || !resolved {
        slot.reset(spine, StyleState::Empty, 0);
    } else if undeclared {
        slot.reset(spine, StyleState::Deferred, 0);
    } else {
        let mut scheduled = ScheduledSource::new(source, on_yield, None);
        let status = styles::build(
            &mut scheduled,
            sources,
            sr.build.as_mut(),
            resources::decoder(&mut r.decoder),
            on_yield,
            slot,
            spine,
                    r.policy,
                );
        if status == Status::OutOfMemory {
            slot.reset(spine, StyleState::Empty, 0);
        } else if status != Status::Ok {
            return Err(status);
        }
    }
    Ok(undeclared)
}

// Observation-only adapters: actual sink/watch remain the semantic authority.
struct ObservedSink<'a, 'b, 'p> {
    sink: &'a mut SelectingSink<'b, 'p>,
    reached: &'a core::cell::Cell<bool>,
}
impl newt_core::chapter_layout::types::Sink for ObservedSink<'_, '_, '_> {
    fn anchor(&mut self, hash: u32, chars: u32) {
        newt_core::chapter_layout::types::Sink::anchor(self.sink, hash, chars);
    }
    fn page(&mut self, page: newt_core::chapter_layout::types::Page<'_>) -> bool {
        let result = newt_core::chapter_layout::types::Sink::page(self.sink, page);
        self.reached.set(self.sink.state.reached);
        result
    }
}
struct ObservedWatch<'a, 'b, S: BookSource + ?Sized> {
    watch: &'a mut Watch<'b, S>,
    settled: &'a core::cell::Cell<bool>,
}
impl<S: BookSource + ?Sized> BookSource for ObservedWatch<'_, '_, S> {
    fn size(&self) -> u64 {
        self.watch.size()
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> i32 {
        let result = self.watch.read_at(offset, out);
        self.settled.set(self.watch.settled());
        result
    }
}

type PassResult = (
    Status,
    u32,
    u32,
    bool,
    newt_phased_sink::State,
    crate::watch::Verdict,
);
#[allow(clippy::too_many_arguments)]
fn pass<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    selected: &mut Selection,
    target: u32,
    mode: Target,
    page_layout: PageLayout,
    frame: Option<&mut Frame>,
    plan: Option<&mut Plan<'_>>,
    slot: &Slot<'_>,
    lr: &mut LayoutResources<'_>,
    r: &mut canonical::Resources<'_, B>,
    reuse: bool,
    on_yield: newt_core::admission::Yield<'_>,
) -> Result<PassResult, Status> {
    let mut io = crate::phase_storage::ParseResources {
        backend: r.backend,
        decoder: resources::decoder(&mut r.decoder),
        xml_buffer: r.xml_buffer.as_deref_mut(),
        filter: r.filter.as_deref_mut(),
    };
    pass_stream(
        source, selected, target, mode, page_layout, frame, plan, slot, lr, &mut io, reuse, on_yield,
    )
}

#[allow(clippy::too_many_arguments)]
fn pass_stream<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    selected: &mut Selection,
    target: u32,
    mode: Target,
    page_layout: PageLayout,
    frame: Option<&mut Frame>,
    plan: Option<&mut Plan<'_>>,
    slot: &Slot<'_>,
    lr: &mut LayoutResources<'_>,
    r: &mut crate::phase_storage::ParseResources<'_, B>,
    reuse: bool,
    on_yield: newt_core::admission::Yield<'_>,
) -> Result<PassResult, Status> {
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let mut chapter = ChapterSource::open(
        &mut scheduled,
        &selected.entry,
        resources::decoder(&mut r.decoder),
        on_yield,
        None,
    )?;
    let mut watch = Watch::begin(&mut chapter, slot.state);
    let raw = Checked::raw(selected.entry.uncompressed_size);
    let mut metrics = ReaderAtlas;
    let atlas = ReaderAtlas;
    let output = match plan {
        Some(p) => Output::Plan(p),
        None => Output::Frame(frame.ok_or(Status::Unsupported)?),
    };
    let mut sink = SelectingSink::new(
        target,
        mode,
        page_layout.landscape,
        &mut metrics,
        &atlas,
        output,
        Some(&mut selected.navigation),
    )
    .map_err(|_| Status::Unsupported)?;
    let params = reader_parameters(page_layout, Some(slot), selected.language);
    let (status, pages, chars, complete) = {
        let reached = core::cell::Cell::new(false);
        let settled = core::cell::Cell::new(watch.settled());
        let mut observed = ObservedSink {
            sink: &mut sink,
            reached: &reached,
        };
        let mut font = ReaderAtlas;
        let mut backend = newt_core::chapter_layout::types::SplitBackend {
            font: &mut font,
            sink: &mut observed,
        };
        let mut input = ObservedWatch {
            watch: &mut watch,
            settled: &settled,
        };
        let builder = CssStylesheetBuilder::default();
        let mut images = lr.images.for_chapter(&selected.href);
        let mut engine = Engine::new(
            params,
            resources::storage(&mut lr.storage),
            &mut backend,
            lr.breaker,
            &mut images,
            None,
            builder,
        )
        .map_err(|_| Status::OutOfMemory)?;
        let stream = XmlStream::new(&mut input, None, r.backend);
        let mut session = ChapterSession::new(stream);
        admission::layout_begin(on_yield);
        let mut status = session.begin(
            &raw,
            xml_session::Resources {
                buffer: r.xml_buffer.as_deref_mut(),
                filter: r.filter.as_deref_mut(),
            },
            &engine,
        );
        while status == Status::Ok && !session.done() {
            status = session.step(
                &mut LayoutHandler {
                    engine: &mut engine,
                },
                1,
            );
            // Only a certified chapter can stop at the actual sink boundary.
            // A watch that settles during this pass is sufficient.
            if reuse
                && !matches!(mode, Target::TocResolve | Target::TocPage)
                && reached.get()
                && settled.get()
            {
                break;
            }
            if status == Status::Ok && !session.done() {
                if let Some(f) = on_yield {
                    f.yield_now();
                }
            }
        }
        let complete = session.done();
        let pages = engine.pages;
        let chars = engine.chars;
        session.abort();
        admission::layout_end(on_yield, pages);
        (status, pages, chars, complete)
    };
    let state = sink.state;
    drop(sink);
    let verdict = watch.verdict();
    drop(watch);
    let mut status = status;
    if state.overflow {
        status = Status::OutOfMemory;
    }
    if status == Status::Ok && complete && !chapter.finish() {
        status = Status::Truncated;
    }
    Ok((status, pages, chars, complete, state, verdict))
}

struct ExactAnchor<'a> {
    wanted: &'a [u8],
    hash: u32,
    seen: bool,
    duplicate: bool,
    collision: bool,
}
impl ExactAnchor<'_> {
    fn exact(&self) -> bool {
        self.seen && !self.duplicate && !self.collision
    }
}
impl Handler for ExactAnchor<'_> {
    fn start_element(&mut self, _: &[u8], attributes: &mut dyn Attributes) {
        while let Some((key, value)) = attributes.next() {
            if !crate::xml_input::local_is(key, b"id") {
                continue;
            }
            if value == self.wanted {
                self.duplicate |= self.seen;
                self.seen = true;
            } else if hash_path(value) == self.hash {
                self.collision = true;
            }
        }
    }
}
/// Pinned standalone exact-ID validation; a parser/I/O error precedes missing ID.
pub fn validate_exact_anchor<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    entry: &Checked,
    wanted: &[u8],
    r: &mut canonical::Resources<'_, B>,
    on_yield: newt_core::admission::Yield<'_>,
) -> Status {
    let mut io = crate::phase_storage::ParseResources {
        backend: r.backend,
        decoder: resources::decoder(&mut r.decoder),
        xml_buffer: r.xml_buffer.as_deref_mut(),
        filter: r.filter.as_deref_mut(),
    };
    validate_exact_anchor_stream(source, entry, wanted, &mut io, on_yield)
}
pub(crate) fn validate_exact_anchor_stream<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    entry: &Checked,
    wanted: &[u8],
    r: &mut crate::phase_storage::ParseResources<'_, B>,
    on_yield: newt_core::admission::Yield<'_>,
) -> Status {
    if wanted.is_empty() {
        return Status::Ok;
    }
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let mut chapter = match ChapterSource::open(
        &mut scheduled,
        entry,
        resources::decoder(&mut r.decoder),
        on_yield,
        None,
    ) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let raw = Checked::raw(entry.uncompressed_size);
    let mut handler = ExactAnchor {
        wanted,
        hash: hash_path(wanted),
        seen: false,
        duplicate: false,
        collision: false,
    };
    let status = {
        let mut stream = XmlStream::new(&mut chapter, None, r.backend);
        let mut status = stream.open(
            &raw,
            xml_session::Resources {
                buffer: r.xml_buffer.as_deref_mut(),
                filter: r.filter.as_deref_mut(),
            },
            true,
        );
        let mut eof = false;
        while status == Status::Ok && !eof {
            status = stream.feed_chunk(&mut handler, Some(&mut eof));
            if status == Status::Ok && !eof {
                if let Some(f) = on_yield {
                    f.yield_now();
                }
            }
        }
        stream.close();
        status
    };
    if status != Status::Ok {
        status
    } else if !chapter.finish() {
        Status::Truncated
    } else if !handler.exact() {
        Status::NotFound
    } else {
        Status::Ok
    }
}
impl PhasedReader<'_, '_> {
    fn confirm_chapter_anchor<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        toc_index: u16,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        lease: Option<&Lease>,
    ) -> Result<u32, Status> {
        let held = match lease.map(Lease::hold).transpose() {
            Ok(h) => h,
            Err(e) => return Err(e),
        };
        let status = self.validate(source);
        if status != Status::Ok {
            return Err(status);
        }
        let selected = &self.selections[self.active ^ 1];
        let page = selected.navigation.staged_resolved_page();
        let target = selected.navigation.target_by_toc(toc_index);
        let status =
            validate_exact_anchor(source, &selected.entry, target.fragment, r, self.on_yield);
        metadata.reset();
        parse.reset();
        drop(held);
        if status != Status::Ok {
            return Err(status);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return Err(status);
        }
        self.selections[self.active ^ 1]
            .navigation
            .stage_required_toc(toc_index);
        Ok(page)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn show_chapter<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        toc_index: u16,
        frame: &mut Frame,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        if toc_index == NO_TOC {
            return self.show(
                source,
                spine,
                0,
                Target::Anchor,
                frame,
                metadata,
                parse,
                r,
                toc,
                sr,
                sources,
                slot,
                lr,
                lease,
            );
        }
        self.reset_last_layout_pages();
        let status = self.layout(
            source,
            spine,
            u32::from(toc_index),
            Target::TocResolve,
            Some(&mut *frame),
            metadata,
            parse,
            r,
            toc,
            sr,
            sources,
            slot,
            lr,
            lease,
        );
        if status != Status::Ok {
            return status;
        }
        let page = match self.confirm_chapter_anchor(source, toc_index, metadata, parse, r, lease) {
            Ok(page) => page,
            Err(error) => return error,
        };
        let status = self.layout(
            source,
            spine,
            page,
            Target::TocPage,
            Some(frame),
            metadata,
            parse,
            r,
            toc,
            sr,
            sources,
            slot,
            lr,
            lease,
        );
        if status == Status::Ok {
            self.publish(source, frame)
        } else {
            status
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn capture_chapter<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        toc_index: u16,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        self.reset_last_layout_pages();
        if toc_index != NO_TOC && self.plan.is_none() {
            return Status::Unsupported;
        }
        if toc_index == NO_TOC {
            return self.capture(
                source,
                spine,
                0,
                Target::Anchor,
                metadata,
                parse,
                r,
                toc,
                sr,
                sources,
                slot,
                lr,
                lease,
            );
        }
        let status = self.layout(
            source,
            spine,
            u32::from(toc_index),
            Target::TocResolve,
            None,
            metadata,
            parse,
            r,
            toc,
            sr,
            sources,
            slot,
            lr,
            lease,
        );
        if status != Status::Ok {
            return status;
        }
        let page = match self.confirm_chapter_anchor(source, toc_index, metadata, parse, r, lease) {
            Ok(page) => page,
            Err(error) => return error,
        };
        let status = self.layout(
            source,
            spine,
            page,
            Target::TocPage,
            None,
            metadata,
            parse,
            r,
            toc,
            sr,
            sources,
            slot,
            lr,
            lease,
        );
        status
    }
}

/// Trusted complete-table projection from the immutable revision owner.
/// Full digest binding is checked explicitly; no truncated ID or filename key.
#[derive(Clone, Copy)]
pub struct SemanticProjection {
    pub digest: [u8; 32],
    pub canonical_spines: u32,
    pub selected_spine: u32,
    pub chapter_chars: u32,
    pub prefix: u64,
    pub total: u64,
}
impl PhasedReader<'_, '_> {
    pub fn clear_semantic_projection(&mut self) {
        self.info.book_char_start = 0;
        self.info.book_total_chars = 0;
        self.info.overall_percent = 0;
        self.info.overall_known = false;
        self.info.overall_estimated = false;
        self.estimate();
    }
    pub fn apply_semantic_projection<S: BookSource + ?Sized>(
        &mut self,
        source: &mut S,
        digest: &[u8; 32],
        p: SemanticProjection,
    ) -> bool {
        self.clear_semantic_projection();
        if !self.opened
            || !self.has_page
            || p.digest != *digest
            || p.canonical_spines == 0
            || p.canonical_spines > 1024
            || p.canonical_spines != self.selection().spines
            || p.selected_spine != self.info.spine
            || p.selected_spine >= p.canonical_spines
            || p.chapter_chars != self.info.total_chars
            || self.info.char_start > p.chapter_chars
            || p.total == 0
            || p.total > u64::from(p.canonical_spines) * u64::from(u32::MAX)
            || p.prefix > u64::from(p.selected_spine) * u64::from(u32::MAX)
            || p.prefix > p.total
            || u64::from(p.chapter_chars) > p.total - p.prefix
            || p.total - p.prefix - u64::from(p.chapter_chars)
                > u64::from(p.canonical_spines - p.selected_spine - 1) * u64::from(u32::MAX)
            || self.validate(source) != Status::Ok
        {
            return false;
        }
        self.info.book_char_start = p.prefix + u64::from(self.info.char_start);
        self.info.book_total_chars = p.total;
        self.info.overall_percent = (self.info.book_char_start * 100 / p.total) as u8;
        self.info.overall_known = true;
        self.info.overall_estimated = false;
        true
    }
}

include!("reader_semantic.rs");

pub(crate) fn reader_parameters<'s>(page_layout: PageLayout, slot: Option<&'s Slot<'_>>, language: BookLanguage) -> Params<'s> {
    let g = Geometry::reader(page_layout.landscape);
    Params {
        width: g.width as i16,
        height: g.height as i16,
        left: g.margin as i16,
        right: g.margin as i16,
        top: g.margin as i16,
        bottom: (g.height - g.content_bottom) as i16,
        base_size: page_layout.text_size.px(),
        align: TextAlign::Justify,
        embedded_styles: false,
        inline_ascent: true,
        language: Some(language.tag()),
        stylesheet: slot.filter(|slot| slot.count != 0).map(Slot::sheet),
        ..Params::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn settle_stopped_style<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    selected: &Selection,
    spine: u32,
    metadata: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    slot: &mut Slot<'_>,
    verdict: crate::watch::Verdict,
    on_yield: newt_core::admission::Yield<'_>,
) -> Status {
    let mut status = Status::Ok;
    let before = slot.state;
    if verdict.state == StyleState::Pending {
        slot.limit_nesting();
    } else {
        let empty = styles::Book {
            manifest: &[],
            arena: metadata,
            catalog: None,
        };
        let mut scheduled = ScheduledSource::new(source, on_yield, None);
        status = styles::resolve(
            &empty,
            &mut scheduled,
            &selected.entry,
            Some(selected.href()),
            &mut *r.backend,
            styles::ResolveScratch {
                directory: sr.directory,
                paths: sr.paths,
                decoder: resources::decoder(&mut r.decoder),
                xml: xml_session::Resources {
                    buffer: r.xml_buffer.as_deref_mut(),
                    filter: r.filter.as_deref_mut(),
                },
            },
            on_yield,
            sources,
                    r.policy,
                );
        if status == Status::Ok {
            status = styles::build(
                &mut scheduled,
                sources,
                sr.build.as_mut(),
                resources::decoder(&mut r.decoder),
                on_yield,
                slot,
                spine,
                    r.policy,
                );
        }
    }
    if status != Status::Ok {
        return status;
    }
    if slot.state <= before {
        return Status::Unsupported;
    }
    Status::Ok
}

#[path = "cache_build.rs"]
mod cache_build;
pub use cache_build::{BuildResult, CacheBuild};

#[path = "session_adapter.rs"]
mod session_adapter;
pub use session_adapter::{cached, CachePresence, OperationCache, OperationReader, PresenceStore, producer};

#[cfg(test)]
pub use session_adapter::presence_cases::{actual_reader_cases, actual_sha_cases};

#[path = "cache_show.rs"]
pub(crate) mod cache_show;
pub use cache_show::{CacheRequest, CachedPageMemory, CapturedPage, ShowResult};

include!("reader_phased.rs");
include!("reader_indexed.rs");


impl PhasedReader<'_, '_> {
    /// Reconstruct previously validated pixels after SDK scratch erased the
    /// native bank. Not a new publication or source verification. This keeps
    /// first-paint pixels on failed optional-menu retry without clearing the
    /// owner failure latch or resuming its tracker.
    pub fn restore_retained_frame(&self, frame: &mut Frame) -> Status {
        if !self.has_page || !self.proofs.plan { return Status::Unsupported; }
        let Some(plan) = self.plan.as_deref() else { return Status::Unsupported; };
        plan.paint(frame, self.page_layout.landscape, &ReaderAtlas, &mut ReaderAtlas);
        Status::Ok
    }
}
#[path = "phase_session.rs"]
pub mod phase_session;

#[cfg(all(test, feature = "native-host"))]
pub use cache_build::physical_engine_cases::{actual_physical_engine_cases, actual_staged_cache_case};

#[path = "reader/projection.rs"]
mod projection;

#[cfg(test)]
#[path = "cache_page_limit_cases.rs"]
mod cache_page_limit_cases;
