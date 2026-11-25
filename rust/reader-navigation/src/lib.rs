// MIT; see rust/THIRD_PARTY.md.
//! Owned navigation projections. Input metadata borrows end before publication.
#![no_std]
#![forbid(unsafe_code)]

mod menu;
mod identity;
pub use identity::resolve_identity;
pub mod metadata;
pub mod index_mapping;
pub mod index_view;
pub mod probe;
pub use menu::{MenuInfo, MenuRow};
use newt_core::{utf8, zip_container::hash_path};

pub const MAX_TARGETS: usize = 12;
pub const STRING_BYTES: usize = 640;
pub const TITLE_BYTES: usize = 96;
pub const NO_TOC: u16 = u16::MAX;
pub const NO_SPINE: u32 = u32::MAX;
pub const FOCUS_CURRENT: u32 = u32::MAX;
pub const MAX_EXPANDED: u64 = 64 * 1024 * 1024;
pub const MAX_MEMBER: u32 = 4 * 1024 * 1024;
pub const RECORD_OVERFLOW: u16 = 1 << 0;
pub const STRING_OVERFLOW: u16 = 1 << 1;
pub const UNRESOLVED_ANCHOR: u16 = 1 << 2;
pub const AMBIGUOUS_ANCHOR: u16 = 1 << 3;
pub const UNMAPPED_TARGET: u16 = 1 << 4;
pub const INVALID_WEIGHTS: u16 = 1 << 5;
pub const TITLE_TRUNCATED: u16 = 1 << 6;
pub const INVALID_SPINE: u16 = 1 << 7;
pub const UNORDERED: u16 = 1 << 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentsFallback { #[default] None, Capacity, Invalid }

const FRAGMENT: u8 = 1;
const RESOLVED: u8 = 2;
const AMBIGUOUS: u8 = 4;
const INHERITED: u8 = 8;
const DECLARED_AMBIGUOUS: u8 = 16;
const MENU_GENERIC: u8 = 32;
const MENU_CURRENT: u8 = 64;
const NO_STRING: u16 = u16::MAX;

pub(crate) fn c_bytes(text: &[u8]) -> &[u8] {
    &text[..text.iter().position(|b| *b == 0).unwrap_or(text.len())]
}
fn c_eq(left: &[u8], right: &[u8]) -> bool {
    let common = left.len().min(right.len());
    for at in 0..common {
        if left[at] != right[at] {
            return false;
        }
        if left[at] == 0 {
            return true;
        }
    }
    left.get(common).copied().unwrap_or(0) == right.get(common).copied().unwrap_or(0)
}
fn text_item(item: Item<'_>) -> bool {
    item.href.is_some()
        && item
            .media
            .is_some_and(|m| c_eq(m, b"application/xhtml+xml") || c_eq(m, b"text/html"))
}
#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub href: Option<&'a [u8]>,
    pub media: Option<&'a [u8]>,
}
#[derive(Clone, Copy, Debug)]
pub struct Declaration<'a> {
    pub title: &'a [u8],
    pub href: &'a [u8],
    pub fragment: &'a [u8],
    pub depth: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub method: u16,
    pub uncompressed: u32,
}
/// Normalize a borrowed query once, before repeated metadata comparisons.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Href<'a>(&'a [u8]);
impl<'a> Href<'a> {
    pub fn new(bytes: &'a [u8]) -> Self { Self(c_bytes(bytes)) }
    pub fn as_bytes(self) -> &'a [u8] { self.0 }
}
/// Adapter methods must represent one immutable metadata snapshot. No method
/// allocates or reads the book payload; required admission already happened.
pub trait BookView {
    fn spine_count(&self) -> usize;
    fn spine_item(&self, index: usize) -> Option<Item<'_>>;
    fn toc_count(&self) -> usize;
    fn toc_entry(&self, index: usize) -> Option<Declaration<'_>>;
    fn member(&self, href: &[u8]) -> Option<Member>;
    /// Index-backed views can map all declarations in one member-table walk.
    /// The default preserves the parser oracle's per-row rule.
    fn declared_spine(&self, row: usize) -> u32 {
        declared_spine_rule(self, row)
    }
    fn progress_inputs(&self, selected: u32) -> ProgressInputs {
        progress_inputs(self, selected)
    }
    fn declared_spines(&self, mapping: &mut [u32]) {
        for (row, mapped) in mapping.iter_mut().enumerate() {
            *mapped = declared_spine(self, row);
        }
    }
    /// Field access avoids resolving unrelated arena strings on lookup paths.
    fn toc_title(&self, index: usize) -> Option<&[u8]> {
        self.toc_entry(index).map(|entry| entry.title)
    }
    fn toc_href(&self, index: usize) -> Option<&[u8]> {
        self.toc_entry(index).map(|entry| entry.href)
    }
    /// Equality queries already borrow content before the first NUL.
    fn toc_href_eq(&self, index: usize, href: Href<'_>) -> bool {
        self.toc_href(index).is_some_and(|h| c_eq(h, href.as_bytes()))
    }
    fn spine_href_eq(&self, index: usize, href: Href<'_>) -> bool {
        self.spine_item(index)
            .and_then(|item| item.href)
            .is_some_and(|h| c_eq(h, href.as_bytes()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Estimate {
    pub before: u64,
    pub current: u64,
    pub total: u64,
    pub spine: u32,
    pub chapter_chars: u32,
}
impl Estimate {
    pub fn valid(self) -> bool {
        self.total != 0
            && self.total <= MAX_EXPANDED
            && self.chapter_chars != 0
            && self.before <= self.total
            && self.current <= self.total - self.before
    }
    /// The original divides by both fields; reject invalid inputs explicitly.
    pub fn percent(self, character: u32) -> Option<u8> {
        if !self.valid() {
            return None;
        }
        let within = self.current * u64::from(character.min(self.chapter_chars))
            / u64::from(self.chapter_chars);
        Some(((self.before + within) * 100 / self.total).min(100) as u8)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressInputs {
    pub before: u64,
    pub current: u64,
    pub total: u64,
    pub index: u32,
    pub count: u32,
    pub valid: bool,
}
impl ProgressInputs {
    pub fn estimate(self, spine: u32, chapter_chars: u32) -> Estimate {
        if !self.valid || self.index != spine {
            return Estimate::default();
        }
        Estimate {
            before: self.before,
            current: self.current,
            total: self.total,
            spine,
            chapter_chars,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section<'a> {
    pub label: &'a [u8],
    pub href: &'a [u8],
    pub fragment: &'a [u8],
    pub spine: u32,
    pub char_start: u32,
    pub toc: u16,
    pub depth: u8,
    pub generic: bool,
    pub inherited: bool,
    pub target_known: bool,
    pub anchor_known: bool,
}
impl Default for Section<'_> {
    fn default() -> Self {
        Self {
            label: b"",
            href: b"",
            fragment: b"",
            spine: 0,
            char_start: 0,
            toc: 0,
            depth: 0,
            generic: true,
            inherited: false,
            target_known: false,
            anchor_known: false,
        }
    }
}

/// Same bounded, NUL-terminated snprintf result, including truncation writes.
/// Increment follows the original unsigned-long width (32 on RV32, 64 on host).
pub fn generic_label(spine: u32, output: &mut [u8]) -> bool {
    if output.is_empty() {
        return false;
    }
    let mut digits = [0; 10];
    let mut at = digits.len();
    let mut value = (spine as usize).wrapping_add(1) as u64;
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let mut label = [0; 18];
    label[..8].copy_from_slice(b"Section ");
    let len = 8 + digits.len() - at;
    label[8..len].copy_from_slice(&digits[at..]);
    let copied = len.min(output.len() - 1);
    output[..copied].copy_from_slice(&label[..copied]);
    output[copied] = 0;
    len < output.len()
}
pub fn copy_label(label: Option<&[u8]>, output: &mut [u8]) -> bool {
    if output.is_empty() {
        return false;
    }
    output[0] = 0;
    let Some(label) = label
        .map(c_bytes)
        .filter(|s| !s.is_empty() && s.len() < output.len())
    else {
        return false;
    };
    output[..label.len()].copy_from_slice(label);
    output[label.len()] = 0;
    true
}
fn title_prefix(text: &[u8]) -> usize {
    let (mut at, mut complete, mut scalar) = (0, 0, 0);
    while at < text.len()
        && text[at] != 0
        && utf8::next(Some(text), &mut at, &mut scalar)
        && at < TITLE_BYTES
    {
        complete = at;
    }
    complete
}
pub fn usable_entry(book: &(impl BookView + ?Sized), toc: usize) -> bool {
    toc < book.toc_count()
        && toc < usize::from(NO_TOC)
        && book
            .toc_title(toc)
            .is_some_and(|title| title_prefix(title) != 0)
}
pub fn readable_spine(book: &(impl BookView + ?Sized), href: &[u8]) -> u32 {
    let href = Href::new(href);
    if href.as_bytes().is_empty() {
        return NO_SPINE;
    }
    let mut found = NO_SPINE;
    for i in 0..book.spine_count() {
        if !book.spine_href_eq(i, href) {
            continue;
        }
        let Some(item) = book.spine_item(i) else {
            continue;
        };
        let entry = text_item(item).then(|| book.member(href.as_bytes())).flatten();
        if found != NO_SPINE
            || entry.is_none_or(|e| !matches!(e.method, 0 | 8) || e.uncompressed > MAX_MEMBER)
        {
            return NO_SPINE;
        }
        found = i as u32;
    }
    found
}
pub fn declared_spine(book: &(impl BookView + ?Sized), toc: usize) -> u32 {
    book.declared_spine(toc)
}
fn declared_spine_rule(book: &(impl BookView + ?Sized), toc: usize) -> u32 {
    if !usable_entry(book, toc) {
        return NO_SPINE;
    }
    let spine = readable_spine(book, book.toc_href(toc).expect("usable declaration"));
    if (spine as usize) < book.spine_count() {
        spine
    } else {
        NO_SPINE
    }
}
fn progress_inputs(book: &(impl BookView + ?Sized), selected: u32) -> ProgressInputs {
    let mut p = ProgressInputs { index: selected, count: book.spine_count() as u32,
        ..ProgressInputs::default() };
    let mut valid = true;
    for i in 0..book.spine_count() {
        let item = book.spine_item(i);
        if item.is_some_and(|it| !text_item(it)) {
            continue;
        }
        let member = item
            .filter(|it| text_item(*it))
            .and_then(|it| book.member(it.href.expect("text href")));
        let Some(member) =
            member.filter(|e| matches!(e.method, 0 | 8) && e.uncompressed <= MAX_MEMBER)
        else {
            valid = false;
            continue;
        };
        // Same uint64 accumulation domain; bind rejects >u32 spine counts.
        let size = u64::from(member.uncompressed);
        p.total += size;
        if i < p.index as usize {
            p.before += size;
        }
        if i == p.index as usize {
            p.current = size;
        }
    }
    p.valid = valid && p.total > 0 && p.total <= MAX_EXPANDED;
    p
}
fn title_key(book: &impl BookView, selected: usize) -> u16 {
    let title = book.toc_title(selected).expect("usable declaration");
    (0..selected)
        .find(|&i| {
            usable_entry(book, i) && c_eq(book.toc_title(i).expect("usable declaration"), title)
        })
        .unwrap_or(selected) as u16
}
fn ambiguous_declaration(book: &impl BookView, selected: usize) -> bool {
    let candidate = book.toc_entry(selected).expect("usable declaration");
    let fragment = c_bytes(candidate.fragment);
    let href = Href::new(candidate.href);
    (0..book.toc_count()).any(|i| {
        if i == selected || !usable_entry(book, i) {
            return false;
        }
        if !book.toc_href_eq(i, href) {
            return false;
        }
        let other = book.toc_entry(i).expect("usable declaration");
        let peer = c_bytes(other.fragment);
        (fragment == peer && candidate.depth == other.depth && !c_eq(candidate.title, other.title))
            || (!fragment.is_empty()
                && !peer.is_empty()
                && fragment != peer
                && hash_path(fragment) == hash_path(peer))
    })
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Target {
    char_start: u32,
    hash: u32,
    spine: u32,
    label: u16,
    href: u16,
    fragment: u16,
    toc: u16,
    title_key: u16,
    depth: u8,
    flags: u8,
}
const _: () = assert!(core::mem::size_of::<Target>() == 24);
#[derive(Clone, Copy, Default)]
struct MenuState {
    natural: u32,
    focus: u32,
    count: u32,
    first: u32,
    current_spine: u32,
    current_toc: u16,
    current_generic: bool,
    authored: bool,
    current_exact: bool,
    fallback: ContentsFallback,
}
#[derive(Clone, Copy)]
enum Projection {
    Progress(ProgressInputs),
    Menu(MenuState),
    RequiredToc(u16),
    ResolvedPage(u32),
}
/// Value copies own all projected strings. An outstanding Section borrow
/// statically prevents a rebind/reset, rather than returning a dangling view.
#[derive(Clone)]
pub struct Navigation {
    targets: [Target; MAX_TARGETS],
    strings: [u8; STRING_BYTES],
    projection: Projection,
    used: u16,
    generic_label: u16,
    bind_issues: u16,
    count: u8,
}
pub mod initialization;

impl Navigation {
    pub fn set_contents_fallback(&mut self, reason: ContentsFallback) {
        if let Projection::Menu(menu) = &mut self.projection { menu.fallback = reason; }
    }
    pub fn contents_fallback(&self) -> ContentsFallback {
        match self.projection {
            Projection::Menu(menu) => menu.fallback,
            _ => ContentsFallback::None,
        }
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn stage_required_toc(&mut self, toc: u16) {
        self.projection = Projection::RequiredToc(toc);
    }
    pub fn staged_required_toc(&self) -> u16 {
        if let Projection::RequiredToc(toc) = self.projection {
            toc
        } else {
            NO_TOC
        }
    }
    pub fn stage_resolved_page(&mut self, page: u32) {
        self.projection = Projection::ResolvedPage(page);
    }
    pub fn staged_resolved_page(&self) -> u32 {
        if let Projection::ResolvedPage(page) = self.projection {
            page
        } else {
            0
        }
    }
    pub fn invalidate_menu(&mut self) {
        if matches!(self.projection, Projection::Menu(_)) {
            self.reset();
        }
    }
    pub fn progress_inputs(&self) -> ProgressInputs {
        match self.projection {
            Projection::Progress(p) => p,
            _ => ProgressInputs::default(),
        }
    }
    fn store(&mut self, text: &[u8], title: bool) -> u16 {
        let mut text = c_bytes(text);
        if title {
            let len = title_prefix(text);
            if len != text.len() {
                self.bind_issues |= TITLE_TRUNCATED;
            }
            text = &text[..len];
        }
        if text.is_empty() {
            return 0;
        }
        let mut offset = 1;
        while offset < usize::from(self.used) {
            let stored = c_bytes(&self.strings[offset..usize::from(self.used)]);
            if stored == text {
                return offset as u16;
            }
            offset += stored.len() + 1;
        }
        let at = usize::from(self.used);
        if text.len() + 1 > self.strings.len() - at {
            self.bind_issues |= STRING_OVERFLOW;
            return NO_STRING;
        }
        self.strings[at..at + text.len()].copy_from_slice(text);
        self.strings[at + text.len()] = 0;
        self.used = (at + text.len() + 1) as u16;
        at as u16
    }
    fn append(&mut self, book: &impl BookView, toc: usize, spine: u32, inherited: bool) -> bool {
        if usize::from(self.count) == MAX_TARGETS {
            self.bind_issues |= RECORD_OVERFLOW;
            return false;
        }
        let entry = book.toc_entry(toc).expect("usable declaration");
        let mark = self.used;
        let mut target = Target::default();
        target.label = self.store(entry.title, true);
        target.href = self.store(entry.href, false);
        target.fragment = self.store(entry.fragment, false);
        // Preserve the original partial unpublished slot on failed insertion.
        self.targets[usize::from(self.count)] = target;
        if [target.label, target.href, target.fragment].contains(&NO_STRING) {
            self.used = mark;
            return false;
        }
        target.spine = spine;
        target.toc = toc as u16;
        target.title_key = title_key(book, toc);
        target.depth = entry.depth;
        target.flags = if inherited { INHERITED } else { 0 };
        if target.fragment != 0 {
            target.flags |= FRAGMENT;
            target.hash = hash_path(self.string(target.fragment));
        } else {
            target.flags |= RESOLVED;
        }
        if ambiguous_declaration(book, toc) {
            target.flags |= AMBIGUOUS | DECLARED_AMBIGUOUS;
        }
        self.targets[usize::from(self.count)] = target;
        self.count += 1;
        true
    }
    fn bind_weights(&mut self, book: &impl BookView) {
        let mut p = book.progress_inputs(self.progress_inputs().index);
        if !p.valid {
            self.bind_issues |= INVALID_WEIGHTS;
            p.before = 0;
            p.current = 0;
            p.total = 0;
        }
        self.projection = Projection::Progress(p);
    }
    pub fn bind(&mut self, book: &impl BookView, selected: u32) -> bool {
        let required_toc = self.staged_required_toc();
        self.reset();
        if selected as usize >= book.spine_count() || book.spine_count() > NO_SPINE as usize {
            self.bind_issues |= INVALID_SPINE;
            return false;
        }
        self.projection = Projection::Progress(ProgressInputs {
            index: selected,
            count: book.spine_count() as u32,
            ..ProgressInputs::default()
        });
        self.generic_label = self.used;
        if !generic_label(selected, &mut self.strings[usize::from(self.used)..]) {
            self.bind_issues |= STRING_OVERFLOW;
            return false;
        }
        self.used += (c_bytes(&self.strings[usize::from(self.used)..]).len() + 1) as u16;
        self.bind_weights(book);
        let mut preceding = book.toc_count();
        let mut preceding_spine = NO_SPINE;
        let mut last = None;
        for i in 0..book.toc_count() {
            if !usable_entry(book, i) {
                if let Some(title) = book.toc_title(i) {
                    let title = c_bytes(title);
                    if !title.is_empty() && title_prefix(title) != title.len() {
                        self.bind_issues |= TITLE_TRUNCATED;
                    }
                }
                continue;
            }
            let spine = declared_spine(book, i);
            if spine == NO_SPINE {
                self.bind_issues |= UNMAPPED_TARGET;
                continue;
            }
            if last.is_some_and(|n| spine < n) {
                self.bind_issues |= UNORDERED;
            }
            last = Some(spine);
            if spine < selected && (preceding_spine == NO_SPINE || spine >= preceding_spine) {
                preceding = i;
                preceding_spine = spine;
            }
        }
        let required =
            required_toc != NO_TOC && declared_spine(book, usize::from(required_toc)) == selected;
        let mut required_stored = false;
        if required && self.append(book, usize::from(required_toc), selected, false) {
            self.targets[MAX_TARGETS - 1] = self.targets[0];
            self.count = 0;
            required_stored = true;
        }
        if preceding != book.toc_count()
            && self.bind_issues & UNORDERED == 0
            && preceding != usize::from(required_toc)
        {
            self.append(book, preceding, preceding_spine, true);
        }
        for i in 0..book.toc_count() {
            if !usable_entry(book, i) {
                continue;
            }
            if i == usize::from(required_toc) {
                if required_stored && usize::from(self.count) < MAX_TARGETS {
                    self.targets[usize::from(self.count)] = self.targets[MAX_TARGETS - 1];
                    self.count += 1;
                    required_stored = false;
                }
                continue;
            }
            if declared_spine(book, i) != selected {
                continue;
            }
            if required_stored && usize::from(self.count) + 1 >= MAX_TARGETS {
                self.bind_issues |= RECORD_OVERFLOW;
                continue;
            }
            self.append(book, i, selected, false);
        }
        true
    }
    pub fn begin_anchors(&mut self) {
        for target in &mut self.targets[..usize::from(self.count)] {
            if target.flags & INHERITED != 0 {
                continue;
            }
            if target.flags & DECLARED_AMBIGUOUS == 0 {
                target.flags &= !AMBIGUOUS;
            }
            if target.flags & FRAGMENT != 0 {
                target.flags &= !RESOLVED;
                target.char_start = 0;
            }
        }
    }
    pub fn observe_anchor(&mut self, hash: u32, start: u32) {
        for target in &mut self.targets[..usize::from(self.count)] {
            if target.flags & FRAGMENT != 0 && target.flags & INHERITED == 0 && target.hash == hash
            {
                if target.flags & RESOLVED != 0 && target.char_start != start {
                    target.flags |= AMBIGUOUS;
                }
                target.flags |= RESOLVED;
                target.char_start = start;
            }
        }
        for i in 0..usize::from(self.count) {
            let (earlier, rest) = self.targets.split_at_mut(i);
            let first = &mut rest[0];
            if first.flags & RESOLVED == 0 || first.flags & INHERITED != 0 {
                continue;
            }
            for second in earlier {
                if second.flags & RESOLVED != 0
                    && second.flags & INHERITED == 0
                    && first.char_start == second.char_start
                    && first.depth == second.depth
                    && first.title_key != second.title_key
                {
                    first.flags |= AMBIGUOUS;
                    second.flags |= AMBIGUOUS;
                }
            }
        }
    }
    pub fn issues(&self) -> u16 {
        if !matches!(self.projection, Projection::Progress(_)) {
            return 0;
        }
        self.targets[..usize::from(self.count)].iter().fold(
            self.bind_issues,
            |mut issues, target| {
                if target.flags & AMBIGUOUS != 0 {
                    issues |= AMBIGUOUS_ANCHOR;
                }
                if target.flags & FRAGMENT != 0 && target.flags & (RESOLVED | INHERITED) == 0 {
                    issues |= UNRESOLVED_ANCHOR;
                }
                issues
            },
        )
    }
    fn string(&self, offset: u16) -> &[u8] {
        c_bytes(&self.strings[usize::from(offset)..])
    }
    fn generic_section(&self) -> Section<'_> {
        Section {
            label: self.string(self.generic_label),
            spine: self.progress_inputs().index,
            ..Section::default()
        }
    }
    fn view(&self, target: &Target) -> Section<'_> {
        let known = target.flags & AMBIGUOUS == 0;
        Section {
            label: self.string(target.label),
            href: self.string(target.href),
            fragment: self.string(target.fragment),
            spine: target.spine,
            char_start: target.char_start,
            toc: target.toc,
            depth: target.depth,
            generic: target.label == 0,
            inherited: target.flags & INHERITED != 0,
            target_known: known,
            anchor_known: known && target.flags & RESOLVED != 0,
        }
    }
    pub fn target_count(&self) -> usize {
        usize::from(self.count)
    }
    pub fn target(&self, index: usize) -> Section<'_> {
        if index < self.target_count() {
            self.view(&self.targets[index])
        } else {
            self.generic_section()
        }
    }
    pub fn target_by_toc(&self, toc: u16) -> Section<'_> {
        if matches!(self.projection, Projection::Menu(_)) {
            return Section::default();
        }
        self.targets[..self.target_count()]
            .iter()
            .find(|t| t.toc == toc)
            .map(|t| self.view(t))
            .unwrap_or_default()
    }
    pub fn section(&self, anchor: u32) -> Section<'_> {
        if matches!(self.projection, Projection::Menu(_)) {
            return Section::default();
        }
        if matches!(self.projection, Projection::Progress(_))
            && self.bind_issues & (RECORD_OVERFLOW | STRING_OVERFLOW) != 0
        {
            return self.generic_section();
        }
        let mut choice = Choice::default();
        for (i, target) in self.targets[..self.target_count()].iter().enumerate() {
            if !choice.consider(
                i,
                target.char_start,
                target.title_key,
                target.depth,
                target.flags & INHERITED != 0,
                target.flags & AMBIGUOUS == 0,
                target.flags & RESOLVED != 0,
                anchor,
            ) {
                return self.generic_section();
            }
        }
        match choice
            .index
            .map(|i| &self.targets[i])
            .filter(|t| t.label != 0 && t.flags & AMBIGUOUS == 0)
        {
            Some(target) => self.view(target),
            None => self.generic_section(),
        }
    }
}
#[derive(Default)]
struct Choice {
    index: Option<usize>,
    start: u32,
    title_key: u16,
    depth: u8,
    inherited: bool,
}
impl Choice {
    #[allow(clippy::too_many_arguments)]
    fn consider(
        &mut self,
        index: usize,
        start: u32,
        title_key: u16,
        depth: u8,
        inherited: bool,
        target: bool,
        anchor: bool,
        page: u32,
    ) -> bool {
        if inherited {
            if self.index.is_none() {
                *self = Self {
                    index: Some(index),
                    start,
                    title_key,
                    depth,
                    inherited: true,
                };
            }
            return true;
        }
        if !target || !anchor {
            return false;
        }
        if start > page {
            return true;
        }
        if self.index.is_none()
            || self.inherited
            || start > self.start
            || (start == self.start && depth > self.depth)
        {
            *self = Self {
                index: Some(index),
                start,
                title_key,
                depth,
                inherited: false,
            };
        } else if start == self.start && depth == self.depth && title_key != self.title_key {
            return false;
        }
        true
    }
}

#[derive(Clone, Copy)]
pub struct Identity<'a> {
    pub label: &'a [u8],
    pub projection_spine: u32,
    pub char_start: u32,
    pub title_key: u16,
    pub depth: u8,
    pub inherited: bool,
    pub target_known: bool,
    pub anchor_known: bool,
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
