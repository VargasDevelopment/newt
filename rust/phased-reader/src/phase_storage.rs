//! MIT. Private casts form short-lived typed views of the two physical banks.
//! No allocation and no retained bank-derived references in reader state.
use crate::{canonical, resources::*, styles};
use core::mem::{align_of, size_of};
use newt_core::book_index_build::{Cursor, CURSORS, JoinState, POSITION_WORDS, WRITER_PREFIX};
use newt_core::member_check::Checked;
use newt_core::{
    chapter_layout::types::{self as layout, LineBreak},
    css::CssRule,
    inflate::{BitBuffer, State},
    package_parsers::{ManifestItem, TocEntry},
    xml_input::EntityFilter,
    xml_session::ParserBackend,
    zip_container::{Entry, InflateScratch, Status},
    zip_window::arena::Arena,
};

mod sleep_scratch;
mod scratch_value;
pub use scratch_value::{scratch_value, ScratchValue};
pub mod bounded;
pub mod bounded_chapters;

pub const BANK_BYTES: usize = 48 * 1024;
pub const INDEX_BYTES: usize = 8 * 1024;
/// Actual SDK lend/return, enclosing all parse views. The result cannot borrow
/// parse. Implementations must return/clear the native owner before returning R.
/// Panic=abort is required, as for Board::with_build_storage.
pub trait BuildLoan {
    fn with_parse<R>(
        &mut self,
        operation: impl for<'a> FnOnce(&'a mut [u8]) -> R,
    ) -> Result<R, Status>;
}

pub struct ParseResources<'a, B: ParserBackend + ?Sized> {
    pub backend: &'a mut B,
    pub decoder: Option<InflateScratch<'a>>,
    pub xml_buffer: Option<&'a mut [u8; 2048]>,
    pub filter: Option<&'a mut EntityFilter>,
}

// This cursor is private to this file, used only to initialize the concrete
// phase arrays below. Each aligned prefix is removed permanently: no reset,
// raw-pointer escape, or overlapping view. Dropless views use one/array;
// scratch_value instead keeps the byte loan until its initialized T is dropped.
struct Parts<'a>(&'a mut [u8]);
impl<'a> Parts<'a> {
    fn bytes(&mut self, count: usize, alignment: usize) -> Result<&'a mut [u8], Status> {
        let padding = self.0.as_ptr().align_offset(alignment);
        let total = padding.checked_add(count).ok_or(Status::OutOfMemory)?;
        if total > self.0.len() {
            return Err(Status::OutOfMemory);
        }
        let all = core::mem::take(&mut self.0);
        let (taken, tail) = all.split_at_mut(total);
        self.0 = tail;
        Ok(&mut taken[padding..])
    }
    fn array<T: Copy + Default>(&mut self, count: usize) -> Result<&'a mut [T], Status> {
        if count == 0 {
            return Ok(&mut []);
        }
        let bytes = self.bytes(
            size_of::<T>()
                .checked_mul(count)
                .ok_or(Status::OutOfMemory)?,
            align_of::<T>(),
        )?;
        let ptr = bytes.as_mut_ptr().cast::<T>();
        // SAFETY: disjoint removed prefix, aligned and exact length. Initialize
        // every value (CssDecl defaults and enum variants are NOT zero bytes).
        unsafe {
            for i in 0..count {
                ptr.add(i).write(T::default());
            }
            Ok(core::slice::from_raw_parts_mut(ptr, count))
        }
    }
    fn one<T>(&mut self, value: T) -> Result<&'a mut T, Status> {
        assert!(!core::mem::needs_drop::<T>());
        let bytes = self.bytes(size_of::<T>(), align_of::<T>())?;
        // SAFETY: same disjoint-prefix invariant; caller supplies a valid value.
        unsafe {
            let ptr = bytes.as_mut_ptr().cast::<T>();
            ptr.write(value);
            Ok(&mut *ptr)
        }
    }
    fn fixed<const N: usize>(&mut self) -> Result<&'a mut [u8; N], Status> {
        self.bytes(N, 1)?
            .try_into()
            .map_err(|_| Status::OutOfMemory)
    }
}
// Each piece is carved in place, so no 7.5 KB temporary lands on the stack.
fn carve_join_state<'a>(parts: &mut Parts<'a>) -> Result<JoinState<'a>, Status> {
    let positions = parts.array::<u64>(POSITION_WORDS)?.try_into().map_err(|_| Status::OutOfMemory)?;
    let cursors = parts.array::<Cursor>(CURSORS)?.try_into().map_err(|_| Status::OutOfMemory)?;
    let saved = parts.fixed::<WRITER_PREFIX>()?;
    Ok(JoinState::new(positions, cursors, saved))
}
// Only Parts initializes typed tail backing, using the same checked lane.
fn tail_array<'storage, T: Copy + Default>(
    arena: &mut Arena<'storage>,
    count: usize,
) -> Result<&'storage mut [T], Status> {
    if count == 0 {
        return Ok(&mut []);
    }
    let size = size_of::<T>()
        .checked_mul(count)
        .ok_or(Status::OutOfMemory)?;
    let bytes = arena
        .take_tail(size, align_of::<T>())
        .ok_or(Status::OutOfMemory)?;
    Parts(bytes).array(count)
}
pub(crate) fn with_menu_mapping<R>(
    arena: &mut Arena<'_>, count: usize, operation: impl FnOnce(&mut [u32]) -> R,
) -> Result<R, Status> {
    if count == 0 { return Ok(operation(&mut [])); }
    let size = count.checked_mul(size_of::<u32>()).ok_or(Status::OutOfMemory)?;
    arena.with_tail(size, align_of::<u32>(), |_, bytes| {
        Ok(operation(Parts(bytes).array::<u32>(count)?))
    }).ok_or(Status::OutOfMemory)?
}
pub(crate) struct ScopedToc;
impl newt_core::package_parsers::TocBacking for ScopedToc {
    fn with_toc<R>(
        &mut self,
        arena: &mut Arena<'_>,
        count: usize,
        operation: impl FnOnce(&mut Arena<'_>, &mut [TocEntry]) -> R,
    ) -> Result<R, Status> {
        // SDK fills an empty TOC even when no backing can be allocated.
        if count == 0 {
            return Ok(operation(arena, &mut []));
        }
        let size = size_of::<TocEntry>()
            .checked_mul(count)
            .ok_or(Status::OutOfMemory)?;
        arena
            .with_tail(size, align_of::<TocEntry>(), |prefix, bytes| {
                let entries = Parts(bytes).array::<TocEntry>(count)?;
                Ok(operation(prefix, entries))
            })
            .ok_or(Status::OutOfMemory)?
    }
}
#[derive(Clone, Copy)]
struct SlotState {
    count: usize,
    hash: u32,
    spine: u32,
    issues: u16,
    state: styles::State,
}
impl Default for SlotState {
    fn default() -> Self {
        Self {
            count: 0,
            hash: styles::ID,
            spine: 0,
            issues: 0,
            state: styles::State::Empty,
        }
    }
}
#[repr(C)]
struct Home {
    rules: [CssRule; styles::MAX_RULES],
    state: SlotState,
    seal: u32,
}
impl Home {
    fn with_slot<R>(&mut self, operation: impl FnOnce(&mut styles::Slot<'_>) -> R) -> R {
        let mut slot = styles::Slot {
            rules: &mut self.rules,
            count: self.state.count,
            hash: self.state.hash,
            spine: self.state.spine,
            issues: self.state.issues,
            state: self.state.state,
        };
        let result = operation(&mut slot);
        self.state = SlotState {
            count: slot.count,
            hash: slot.hash,
            spine: slot.spine,
            issues: slot.issues,
            state: slot.state,
        };
        self.seal = seal(self);
        result
    }
}
fn seal(home: &Home) -> u32 {
    // Hash values, never Rust padding. Rules contain no references; all fields
    // used by the sheet are covered, including signed sentinel declarations.
    let mut h = 2166136261u32;
    let mut mix = |value: u32| {
        for b in value.to_le_bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(16777619);
        }
    };
    mix(home.state.count as u32);
    mix(home.state.hash);
    mix(home.state.spine);
    mix(u32::from(home.state.issues));
    mix(home.state.state as u32);
    for rule in home.rules.iter().take(home.state.count) {
        mix(rule.elem_hash);
        mix(rule.class_hash);
        let d = rule.decl;
        mix(u32::from(d.size_pct));
        mix(u32::from(d.size_root_relative));
        mix(d.weight_bold as u32);
        mix(d.style_italic as u32);
        mix(d.align as u32);
        mix(d.text_indent_pct as u32);
        mix(d.margin_left_pct as u32);
        mix(d.margin_top_pct as u32);
        mix(d.margin_bottom_pct as u32);
        mix(d.display_none as u32);
        mix(d.underline as u32);
        mix(d.vert_align as u32);
    }
    h
}
fn top<T>(bytes: &mut [u8]) -> Result<(&mut [u8], &mut [u8]), Status> {
    let start = bytes
        .len()
        .checked_sub(size_of::<T>())
        .ok_or(Status::OutOfMemory)?;
    let address = bytes.as_ptr() as usize;
    let offset = (address + start) & !(align_of::<T>() - 1);
    let at = offset.checked_sub(address).ok_or(Status::OutOfMemory)?;
    Ok(bytes.split_at_mut(at))
}
fn initialized<T>(bytes: &mut [u8]) -> &mut T {
    // SAFETY: private callers supply either the aligned top::<Home> initialized
    // below, its intact held home, or a checked Parts prefix initialized by
    // State::initialize_storage. No view survives the enclosing phase callback.
    unsafe { &mut *bytes.as_mut_ptr().cast::<T>() }
}
fn init_home(bytes: &mut [u8]) -> &mut Home {
    let ptr = bytes.as_mut_ptr().cast::<Home>();
    // SAFETY: top::<Home> supplies extent/alignment. Initialize in place to avoid
    // a whole Home temporary; no zeroed enums or stale rules are read.
    unsafe {
        let rules = core::ptr::addr_of_mut!((*ptr).rules).cast::<CssRule>();
        for i in 0..styles::MAX_RULES {
            rules.add(i).write(CssRule::default());
        }
        core::ptr::addr_of_mut!((*ptr).state).write(SlotState::default());
        core::ptr::addr_of_mut!((*ptr).seal).write(0);
        let h = &mut *ptr;
        h.seal = seal(h);
        h
    }
}

include!("phase_source_record.rs");
include!("phase_watch.rs");

fn io<'a, B: ParserBackend + ?Sized>(
    parts: &mut Parts<'a>,
    backend: &'a mut B,
) -> Result<ParseResources<'a, B>, Status> {
    let bytes = parts.bytes(size_of::<State>(), align_of::<State>())?;
    // Match miniz's native register width, including the C3's 32-bit path.
    let mode = if cfg!(target_pointer_width = "64") { BitBuffer::Bits64 } else { BitBuffer::Bits32 };
    if !State::initialize_storage(bytes, mode) {
        return Err(Status::OutOfMemory);
    }
    let state = initialized::<State>(bytes);
    let dictionary = parts.fixed::<32768>()?;
    let input = parts.fixed::<2048>()?;
    let xml_buffer = Some(parts.fixed::<2048>()?);
    let filter = Some(parts.one(EntityFilter::new())?);
    Ok(ParseResources {
        backend,
        decoder: Some(InflateScratch {
            state,
            dictionary,
            input,
        }),
        xml_buffer,
        filter,
    })
}

#[path = "phase_storage/retained.rs"]
mod retained;
pub use retained::StorageState;

pub struct Storage<'a> {
    shared: &'a mut [u8; BANK_BYTES],
    held: bool,
}
impl<'a> Storage<'a> {
    pub fn new(shared: &'a mut [u8; BANK_BYTES]) -> Self {
        Self {
            shared,
            held: false,
        }
    }
    pub fn discard_styles(&mut self) {
        self.held = false;
    }
    pub fn styles_ready(&mut self, spine: u32) -> bool {
        if !self.held {
            return false;
        }
        let Ok((_, bytes)) = top::<Home>(self.shared) else {
            return false;
        };
        let h = initialized::<Home>(bytes);
        h.state.count <= styles::MAX_RULES
            && h.seal == seal(h)
            && h.state.spine == spine
            && h.state.state != styles::State::Empty
    }
    pub fn with_loan<R>(
        &mut self,
        lender: &mut impl BuildLoan,
        operation: impl for<'loan> FnOnce(&mut Phase<'loan>) -> Result<R, Status>,
    ) -> Result<R, Status> {
        lender.with_parse(|native| {
            let padding = native.as_ptr().align_offset(16);
            let end = padding.checked_add(BANK_BYTES).ok_or(Status::OutOfMemory)?;
            let parse = native.get_mut(padding..end).ok_or(Status::OutOfMemory)?;
            let mut phase = Phase {
                policy: None,
                shared: &mut *self.shared,
                parse,
                held: &mut self.held,
            };
            operation(&mut phase)
        })?
    }
}
/// Exists only inside the actual loan. Its constructors borrow sequentially;
/// metadata and layout can never coexist even through an Arena::reset.
pub struct Phase<'a> {
    policy: Option<&'a dyn newt_core::xml_session::SourcePolicy>,
    shared: &'a mut [u8],
    parse: &'a mut [u8],
    held: &'a mut bool,
}
impl Phase<'_> {
    /// The index builder's join and publish state, carved from the parse bank
    /// after the OPF pass. It borrows the whole Phase, and every inflate and
    /// XML resource is carved through `&mut self` too (the crate-private
    /// `stream`), so none can coexist with it. A doctest cannot name `stream`,
    /// so this proof shows the shared rule with a second `&mut self` loan:
    ///
    /// ```compile_fail,E0499
    /// fn overlap(phase: &mut newt_phased_reader::phase_storage::Phase<'_>) {
    ///     let state = phase.join_state();
    ///     let parse_again = phase.join_state();
    ///     drop((state, parse_again));
    /// }
    /// ```
    pub fn join_state(&mut self) -> Result<JoinState<'_>, Status> {
        carve_join_state(&mut Parts(&mut *self.parse))
    }
    pub(crate) fn stream<B: ParserBackend + ?Sized, R>(
        &mut self,
        backend: &mut B,
        operation: impl FnOnce(&mut ParseResources<'_, B>) -> R,
    ) -> Result<R, Status> {
        let mut parts = Parts(self.parse);
        let mut resources = io(&mut parts, backend)?;
        Ok(operation(&mut resources))
    }

    pub(crate) fn ready(&mut self, spine: u32) -> bool {
        if !*self.held {
            return false;
        }
        let Ok((_, bytes)) = top::<Home>(self.shared) else {
            return false;
        };
        let h = initialized::<Home>(bytes);
        h.state.count <= styles::MAX_RULES
            && h.seal == seal(h)
            && h.state.spine == spine
            && h.state.state != styles::State::Empty
    }
    pub(crate) fn edit_slot<R>(
        &mut self,
        operation: impl FnOnce(&mut styles::Slot<'_>) -> R,
    ) -> Result<R, Status> {
        if !*self.held {
            return Err(Status::OutOfMemory);
        }
        let (_, bytes) = top::<Home>(self.shared)?;
        Ok(initialized::<Home>(bytes).with_slot(operation))
    }
    pub(crate) fn admit<
        S: crate::zip_window::source::BookSource + ?Sized,
        B: ParserBackend + ?Sized,
    >(
        &mut self,
        source: &mut S,
        backend: &mut B,
        on_yield: newt_core::admission::Yield<'_>,
        observer: crate::admission::Observer,
    ) -> Result<Status, Status> {
        use crate::admission::{ScheduledSource, WorkKind, emit_to};
        *self.held = false;
        let mut scheduled = ScheduledSource::new(&mut *source, on_yield, self.policy);
        emit_to(on_yield, observer, WorkKind::InspectBegin, 0);
        let status = {
            let scan: &mut [u8; 4099] = (&mut self.shared[..4099])
                .try_into()
                .map_err(|_| Status::OutOfMemory)?;
            crate::admission::inspect(&mut scheduled, &mut Arena::new(self.parse), scan)
        };
        emit_to(on_yield, observer, WorkKind::InspectEnd, status as i32);
        if status != Status::Ok {
            return Ok(status);
        }
        self.metadata_counted(
            source,
            backend,
            true,
            on_yield,
            observer,
            |book, _, _, _, _, _| book.validate_selection(on_yield, observer),
        )
    }

    /// Parser-oracle counted admission/selection. The scratch lifetime ends at open,
    /// before style paths reclaim those parse bytes.
    pub(crate) fn metadata_counted<
        S: crate::zip_window::source::BookSource + ?Sized,
        B: ParserBackend + ?Sized,
        R,
    >(
        &mut self,
        source: &mut S,
        backend: &mut B,
        required: bool,
        on_yield: newt_core::admission::Yield<'_>,
        observer: crate::admission::Observer,
        operation: impl FnOnce(
            canonical::Book<'_, '_, crate::admission::ScheduledSource<'_, S>, B>,
            &mut StyleResources<'_, '_>,
            &mut styles::Sources<'_>,
            &mut usize,
            &mut usize,
            usize,
        ) -> R,
    ) -> Result<R, Status> {
        *self.held = false;
        let bank_base = self.shared.as_ptr() as usize;
        let (shared, _) = top::<Home>(self.shared)?;
        let (parse, record_bytes) = top::<SourceRecord>(self.parse)?;
        let record = Parts(record_bytes).one(SourceRecord::default())?;
        let mut shared = Parts(shared);
        let scan = shared.fixed::<4099>()?;
        let sequential = shared.fixed::<1024>()?;
        let directory = shared.fixed::<512>()?;
        let mut metadata = Arena::new(shared.0);
        let mut parse = Parts(parse);
        let chunk = Some(parse.fixed::<2048>()?);
        let mut io = io(&mut parse, backend)?;
        let mut scheduled = crate::admission::ScheduledSource::new(source, on_yield, self.policy);
        let book = {
            let mut hashes = Parts(&mut *parse.0);
            canonical::open_allocated(
                &mut scheduled,
                &mut metadata,
                canonical::OpenResources {
                policy: self.policy,
                    scan,
                    sequential,
                    backend: io.backend,
                    decoder: io.decoder.take(),
                    chunk,
                    xml_buffer: io.xml_buffer.take(),
                    filter: io.filter.take(),
                },
                required,
                on_yield,
                observer,
                tail_array::<Entry>,
                |arena, manifest, hash_count| {
                    Ok((
                        tail_array::<ManifestItem>(arena, manifest)?,
                        hashes.array::<u32>(hash_count)?,
                    ))
                },
                tail_array::<u16>,
            )?
        };
        // The independently inferred scratch lifetime ended above. Book owns
        // only retained shared-bank metadata, so paths reclaim the hash bytes.
        let mut paths = Arena::new(parse.0);
        let mut sr = StyleResources {
            directory,
            paths: &mut paths,
            build: None,
        };
        let (mut capacity, mut offset) = (0, 0);
        let result = record.with_text(&mut [], |sources| {
            operation(
                book,
                &mut sr,
                sources,
                &mut capacity,
                &mut offset,
                bank_base,
            )
        });
        record.capacity = capacity;
        record.offset = offset;
        Ok(result)
    }

    pub(crate) fn defer(&mut self, spine: u32) -> Result<(), Status> {
        let (_, bytes) = top::<Home>(self.shared)?;
        init_home(bytes).with_slot(|slot| slot.reset(spine, styles::State::Deferred, 0));
        *self.held = true;
        Ok(())
    }
    pub(crate) fn build_styles<
        S: crate::zip_window::source::BookSource + ?Sized,
        B: ParserBackend + ?Sized,
    >(
        &mut self,
        source: &mut S,
        backend: &mut B,
        chapter: &Checked,
        href: &[u8],
        spine: u32,
        on_yield: newt_core::admission::Yield<'_>,
    ) -> Result<Status, Status> {
        *self.held = false;
        let (shared, home_bytes) = top::<Home>(self.shared)?;
        let at = shared
            .len()
            .checked_sub(styles::EMBEDDED)
            .ok_or(Status::OutOfMemory)?;
        let (parse, record_bytes) = top::<SourceRecord>(self.parse)?;
        // SAFETY: metadata initialized this record, and no intervening parse
        // overwrite occurred. This function is private to the phase flow.
        let record = unsafe { &mut *record_bytes.as_mut_ptr().cast::<SourceRecord>() };
        // Metadata is dead. Move a complete short capture to the full text
        // region, or scan again with no Book and retain only its resolved links.
        let capacity = record.capacity;
        let recapture = record.issues & styles::RESOURCE != 0 && capacity < styles::EMBEDDED;
        if !recapture {
            let end = record
                .offset
                .checked_add(record.used)
                .ok_or(Status::OutOfMemory)?;
            let capture_end = record
                .offset
                .checked_add(capacity)
                .ok_or(Status::OutOfMemory)?;
            if record.used > capacity || capture_end > shared.len() || end > capture_end {
                return Err(Status::OutOfMemory);
            }
            // All metadata/typed views have died. Source may lie below the
            // ordinary text region; overlap is legal only in this byte phase.
            shared.copy_within(record.offset..end, at);
        }
        let (shared, text) = shared.split_at_mut(at);
        let mut shared = Parts(shared);
        let mut parse = Parts(parse);
        // The second record consumes parse scratch until build ends.
        let again = if recapture {
            Some(parse.one(SourceRecord::default())?)
        } else {
            None
        };
        let directory = if recapture {
            Some(parse.fixed::<512>()?)
        } else {
            None
        };
        let mut io = io(&mut parse, backend)?;
        let mut scheduled = crate::admission::ScheduledSource::new(source, on_yield, self.policy);
        if let Some(again) = again {
            let directory = directory.ok_or(Status::OutOfMemory)?;
            let mut paths = Arena::new(parse.0);
            let status = again.with_text(text, |sources| {
                styles::resolve(
                    &styles::Book {
                        manifest: &[],
                        arena: &Arena::default(),
                        catalog: None,
                    },
                    &mut scheduled,
                    chapter,
                    Some(href),
                    &mut *io.backend,
                    styles::ResolveScratch {
                        directory,
                        paths: &mut paths,
                        decoder: decoder(&mut io.decoder),
                        xml: crate::xml_session::Resources {
                            buffer: io.xml_buffer.as_deref_mut(),
                            filter: io.filter.as_deref_mut(),
                        },
                    },
                    on_yield,
                    sources,
                    self.policy,
                )
            });
            // Transport/OOM wins over topology comparison, as in buildStyles.
            if status != Status::Ok {
                return Ok(status);
            }
            if again.events != record.events || again.external_mask != record.external_mask {
                return Ok(Status::Stale);
            }
            again.links = record.links;
            core::mem::swap(record, again);
        }
        let status = record.with_text(text, |sources| -> Result<Status, Status> {
            let mut scratch = styles::BuildScratch {
                text: shared.fixed()?,
                marks: shared.fixed()?,
                batch: shared.fixed()?,
                sdk: shared
                    .array::<CssRule>(styles::SDK_BATCH)?
                    .try_into()
                    .map_err(|_| Status::OutOfMemory)?,
                pending: shared
                    .array::<CssRule>(styles::MAX_RULES)?
                    .try_into()
                    .map_err(|_| Status::OutOfMemory)?,
                chunk: shared.fixed()?,
            };
            Ok(init_home(home_bytes).with_slot(|slot| {
                styles::build(
                    &mut scheduled,
                    sources,
                    Some(&mut scratch),
                    decoder(&mut io.decoder),
                    on_yield,
                    slot,
                    spine,
                    self.policy,
                )
            }))
        })?;
        *self.held = status == Status::Ok;
        Ok(status)
    }
    pub(crate) fn layout<B: ParserBackend + ?Sized, R>(
        &mut self,
        backend: &mut B,
        breaker: &mut dyn LineBreak,
        operation: impl FnOnce(
            &mut styles::Slot<'_>,
            &mut LayoutResources<'_>,
            &mut ParseResources<'_, B>,
        ) -> R,
    ) -> Result<R, Status> {
        if !*self.held {
            return Err(Status::OutOfMemory);
        }
        let (_, old) = top::<Home>(self.shared)?;
        let (parse, loan_bytes) = top::<Home>(self.parse)?;
        // SAFETY: held home is initialized, banks are exclusive/disjoint and
        // Home contains no pointers. No view of either bank survives this move.
        unsafe {
            core::ptr::copy_nonoverlapping(
                old.as_ptr(),
                loan_bytes.as_mut_ptr(),
                size_of::<Home>(),
            );
        }
        *self.held = false;
        let mut shared = Parts(self.shared);
        let storage = layout::Storage {
            text: shared.fixed::<3072>()?,
            breaks: shared.fixed::<3072>()?,
            levels: shared.fixed::<3072>()?,
            spans: shared.array::<layout::Span>(96)?,
            lines: shared.array::<layout::Line>(176)?,
            elements: shared.array::<layout::Element>(16)?,
            runs: shared.array::<layout::Run>(304)?,
            images: shared.array::<layout::Image>(8)?,
            links: shared.array::<layout::Link>(16)?,
            page: shared.fixed::<7168>()?,
            style_text: shared.fixed::<3072>()?,
            paragraph_links: shared.array::<layout::ParLink>(6)?,
        };
        let path_scratch = shared.bytes(1024, 1)?;
        let mut lr = LayoutResources {
            storage,
            breaker,
            images: CataloglessImages {
                directory: &[],
                path_scratch,
            },
        };
        let mut parse = Parts(parse);
        let mut io = io(&mut parse, backend)?;
        let result = initialized::<Home>(loan_bytes).with_slot(|slot| operation(slot, &mut lr, &mut io));
        // End every layout/shared view BEFORE reconstructing the home there.
        drop(lr);
        let (_, destination) = top::<Home>(self.shared)?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                loan_bytes.as_ptr(),
                destination.as_mut_ptr(),
                size_of::<Home>(),
            );
        }
        *self.held = true;
        Ok(result)
    }
    pub(crate) fn settle<
        S: crate::zip_window::source::BookSource + ?Sized,
        B: ParserBackend + ?Sized,
    >(
        &mut self,
        source: &mut S,
        backend: &mut B,
        selected: &crate::Selection,
        spine: u32,
        verdict: crate::watch::Verdict,
        on_yield: newt_core::admission::Yield<'_>,
    ) -> Result<Status, Status> {
        let (_, home_bytes) = top::<Home>(self.shared)?;
        let before = initialized::<Home>(home_bytes).state.state;
        if verdict.state == styles::State::Pending {
            initialized::<Home>(home_bytes).with_slot(|slot| slot.limit_nesting());
            return Ok(Status::Ok);
        }
        *self.held = false;
        let (shared, _) = top::<Home>(self.shared)?;
        let at = shared.len() - styles::EMBEDDED;
        let (shared, text) = shared.split_at_mut(at);
        let (parse, record_bytes) = top::<SourceRecord>(self.parse)?;
        let record = Parts(record_bytes).one(SourceRecord::default())?;
        record.capacity = styles::EMBEDDED;
        record.offset = at;
        let mut shared = Arena::new(shared);
        let mut parse = Parts(parse);
        let directory = parse.fixed::<512>()?;
        let mut io = io(&mut parse, backend)?;
        let mut paths = Arena::new(parse.0);
        let empty = styles::Book {
            manifest: &[],
            arena: &mut shared,
            catalog: None,
        };
        let mut scheduled = crate::admission::ScheduledSource::new(&mut *source, on_yield, self.policy);
        let status = record.with_text(text, |sources| {
            styles::resolve(
                &empty,
                &mut scheduled,
                &selected.entry,
                Some(selected.href()),
                io.backend,
                styles::ResolveScratch {
                    directory,
                    paths: &mut paths,
                    decoder: decoder(&mut io.decoder),
                    xml: crate::xml_session::Resources {
                        buffer: io.xml_buffer.take(),
                        filter: io.filter.take(),
                    },
                },
                on_yield,
                sources,
                    self.policy,
                )
        });
        if status != Status::Ok {
            return Ok(status);
        }
        let status = self.build_styles(
            source,
            backend,
            &selected.entry,
            selected.href(),
            spine,
            on_yield,
        )?;
        if status != Status::Ok {
            return Ok(status);
        }
        let (_, bytes) = top::<Home>(self.shared)?;
        Ok(if initialized::<Home>(bytes).state.state <= before {
            Status::Unsupported
        } else {
            Status::Ok
        })
    }
}

struct IndexParts<'a>(Parts<'a>);
impl<'a> newt_core::page_cache::IndexAllocation<'a> for IndexParts<'a> {
    fn pages(&mut self, count: usize) -> Result<&'a mut [newt_core::page_cache::Entry], Status> {
        self.0.array(count)
    }
    fn anchors(&mut self, count: usize) -> Result<&'a mut [newt_core::page_cache::Anchor], Status> {
        self.0.array(count)
    }
}
pub use newt_core::page_cache::ReadScope;

/// Created only inside ReadScope::query; it cannot revalidate the raw source.
pub(crate) struct ScopedRead<'a>(pub(crate) &'a mut dyn newt_core::page_cache::CacheStorage);
impl newt_core::page_cache::CacheStorage for ScopedRead<'_> {
    fn file_size(&mut self, name: &[u8]) -> i64 { self.0.file_size(name) }
    fn read_at(&mut self, name: &[u8], at: u32, out: &mut [u8]) -> i32 { self.0.read_at(name, at, out) }
    fn remove(&mut self, _: &[u8]) -> bool { false }
    fn begin_write(&mut self, _: &[u8]) -> bool { false }
    fn write(&mut self, _: &[u8]) -> bool { false }
    fn end_write(&mut self) -> bool { false }
}
impl Storage<'_> {
    pub(crate) fn cache_page<C: ReadScope, R>(
        &mut self,
        lender: &mut impl BuildLoan,
        store: &mut C,
        name: &[u8],
        opening: bool,
        generation: u32,
        request: crate::CacheRequest,
        located: Option<newt_core::cache_names::segment::SegmentAnchor>,
        certified: Option<newt_core::cache_names::segment::ChapterSegment>,
        source: &mut dyn crate::zip_window::source::BookSource,
        bytes: u64,
        miss: &crate::reader::cache_show::Miss,
        after_return: impl FnOnce(
            &mut dyn crate::zip_window::source::BookSource,
            newt_core::chapter_layout::types::Page<'_>,
            u32,
            u32,
        ) -> R,
    ) -> Result<R, Status> {
        use newt_core::page_cache::PageMemory;
        // A cache probe borrows below the retained StyleHome.
        let shared = if self.held { top::<Home>(self.shared)?.0 } else { &mut self.shared[..] };
        let mut shared = Parts(shared);
        let records = PageMemory {
            runs: shared.array::<layout::Run>(304)?,
            images: shared.array::<layout::Image>(8)?,
            links: shared.array::<layout::Link>(16)?,
        };
        let mut memory = crate::CachedPageMemory {
            scratch: Arena::new(shared.0),
            records,
        };
        let memory_loan = &mut memory;
        let store_loan = &mut *store;
        let mut source_live = || super::reader::validate(source, bytes) == Status::Ok;
        let result = lender.with_parse(move |native| {
            // Consume these references so this is an FnOnce whose result may
            // borrow shared page memory, while NEVER borrowing native parse.
            let memory = memory_loan;
            let store = store_loan;
            query_lent(
                native, store, name, opening, generation, request, located, certified,
                &mut source_live, memory, miss,
            )
        }).map_err(|error| { miss.set(newt_core::reader_action_profile::CacheMissReason::Lease); error })??;
        // Capture checks the source after the parse return.
        let (page, pages, chars) = result;
        Ok(after_return(source, page, pages, chars))
    }
}

#[allow(clippy::too_many_arguments)]
fn query_lent<'page>(
    native: &mut [u8],
    store: &mut impl ReadScope,
    name: &[u8],
    opening: bool,
    generation: u32,
    request: crate::CacheRequest,
    located: Option<newt_core::cache_names::segment::SegmentAnchor>,
    certified: Option<newt_core::cache_names::segment::ChapterSegment>,
    source_live: &mut dyn FnMut() -> bool,
    memory: &'page mut crate::CachedPageMemory<'_>,
    miss: &crate::reader::cache_show::Miss,
) -> Result<(layout::Page<'page>, u32, u32), Status> {
    use newt_core::page_cache::PageCacheReader;
    let padding = native.as_ptr().align_offset(16);
    let end = padding.checked_add(BANK_BYTES).ok_or_else(|| {
        miss.set(newt_core::reader_action_profile::CacheMissReason::Lease); Status::OutOfMemory
    })?;
    let parse = native.get_mut(padding..end).ok_or_else(|| {
        miss.set(newt_core::reader_action_profile::CacheMissReason::Lease); Status::OutOfMemory
    })?;
    let (index_bytes, verified) = parse.split_at_mut(INDEX_BYTES);
    let mut index = Parts(index_bytes);
    let mut names = Arena::new(index.bytes(name.len().checked_add(1).ok_or(Status::OutOfMemory)?, 1)
        .map_err(|error| { miss.set(newt_core::reader_action_profile::CacheMissReason::Open); error })?);
    let mut index = IndexParts(index);
    let (page, finished) = store.query(name, opening, verified, source_live, move |inner| {
        let mut cache = PageCacheReader::default();
        // ReadScope supplies the verified native read; raw source authority
        // remains at the enclosing entry/exit, never on each index record.
        let mut read = ScopedRead(inner);
        match cache.open_allocated(&mut read, name, generation, &mut names, &mut index) {
            Ok(records) => super::reader::cache_show::query_open_page(
                &mut read,
                request,
                located,
                certified,
                &cache,
                &names,
                &records,
                memory,
                miss,
            ),
            Err(e) => { miss.set(newt_core::reader_action_profile::CacheMissReason::Open); Err(e) },
        }
    }).map_err(|error| { miss.set(newt_core::reader_action_profile::CacheMissReason::Attach); error })?;
    // All index/name references are local; no borrowed index field to unbind.
    // The returned page lifetime is 'page, and cannot contain a native pointer.
    if !finished {
        if page.is_ok() { miss.set(newt_core::reader_action_profile::CacheMissReason::Finish); }
        return Err(Status::Stale);
    }
    page
}

#[cfg(test)]
#[path = "phase_storage_empty_cases.rs"]
mod empty_array_tests;

#[cfg(test)]
#[path = "phase_storage_cache_cases.rs"]
mod cache_scope_tests;

#[cfg(test)]
mod join_state_tests {
    use super::*;
    #[test]
    fn the_index_join_state_leaves_most_of_the_parse_bank_free() {
        #[repr(align(16))]
        struct Bank([u8; BANK_BYTES]);
        let mut bank = Bank([0; BANK_BYTES]);
        let mut parts = Parts(&mut bank.0);
        assert!(carve_join_state(&mut parts).is_ok());
        // 4,096 B positions + 64 x 48 B cursors + 384 B writer prefix, no padding.
        assert_eq!(BANK_BYTES - parts.0.len(), newt_core::book_index_build::JOIN_STATE_BYTES);
        assert_eq!(parts.0.len(), 41_600);
    }
}

#[path = "producer_storage.rs"]
pub mod producer;

/// Temporary 8192-byte index loan from two disjoint existing Plan arrays.
/// The canonical Plan buffers are separate loans, so never pretend they form
/// one contiguous allocation. Each typed lane is independently 16-byte aligned.
pub(crate) fn with_writer_index<R>(
    plan: &mut newt_phased_sink::Plan<'_>,
    operation: impl FnOnce(&mut newt_core::page_cache::WriterMemory<'_>) -> R,
) -> Result<R, Status> {
    use newt_core::page_cache::{Anchor, IndexChunk, WriterMemory};
    const RUN_BYTES: usize = 2032;
    const TEXT_BYTES: usize = 6160;
    const _: () = assert!(RUN_BYTES + TEXT_BYTES == INDEX_BYTES);
    const _: () = assert!(size_of::<newt_phased_sink::Run>() == 8);
    const _: () = assert!(size_of::<IndexChunk>() == 1024);
    const _: () = assert!(size_of::<Anchor>() == 8);
    plan.with_unpublished_backing(|runs, text| {
        // SAFETY: exclusive prefix of repr(C) Run's initialized integer fields;
        // size 8 equals the sum of fields (no padding), all bit patterns valid,
        // no destructors. No run view is used during the byte loan. Parts creates
        // disjoint initialized POD arrays; none outlives this callback.
        let run_bytes = unsafe {
            core::slice::from_raw_parts_mut(runs.as_mut_ptr().cast::<u8>(), RUN_BYTES)
        };
        let mut run_parts = Parts(run_bytes);
        let anchor_bytes = run_parts.bytes(192 * size_of::<Anchor>(), 16)?;
        let anchors = Parts(anchor_bytes).array::<Anchor>(192)?;
        let mut text_parts = Parts(&mut text[..TEXT_BYTES]);
        let chunk_bytes = text_parts.bytes(6 * size_of::<IndexChunk>(), 16)?;
        let chunks = Parts(chunk_bytes).array::<IndexChunk>(6)?;
        Ok(operation(&mut WriterMemory { chunks, anchors }))
    })
}

#[path = "producer_cache_read.rs"]
mod producer_cache_read;

include!("phase_index.rs");

/// After the nav inflater returns, mapping and verified reads share the parse
/// bank. Keep 8 KiB spare for scalar/alignment changes, independently of TOC
/// strings and entries in the shared bank.
pub const CONTENTS_BATCH_ROWS: usize = 2000;
const CONTENTS_QUERY_BYTES: usize = 4160 + 4096;
const _: () = assert!(CONTENTS_BATCH_ROWS * (12 + 4) + CONTENTS_QUERY_BYTES + 8192 <= BANK_BYTES);
impl Phase<'_> {
    pub fn with_contents_mapping<R>(&mut self, count: usize,
        operation: impl FnOnce(Option<(&mut [u32], &mut [newt_reader_navigation::index_mapping::HashRow])>, &mut [u8]) -> R,
    ) -> Result<R, Status> {
        let mut parts = Parts(self.parse);
        let query = parts.bytes(CONTENTS_QUERY_BYTES, 16)?;
        if count > CONTENTS_BATCH_ROWS { return Ok(operation(None, query)); }
        let mapping = parts.array::<u32>(count)?;
        let rows = parts.array::<newt_reader_navigation::index_mapping::HashRow>(count)?;
        Ok(operation(Some((mapping, rows)), query))
    }
}

#[path = "phase_navigation_indexed.rs"]
mod navigation_indexed;
#[path = "phase_styles_indexed.rs"]
mod styles_indexed;
impl Phase<'_> {
    pub(crate) fn with_index_open<R>(&mut self,
        operation: impl FnOnce(&mut [u8], &mut [u8; 4099]) -> Result<R, Status>,
    ) -> Result<R, Status> {
        *self.held = false;
        let query = Parts(self.parse).bytes(CONTENTS_QUERY_BYTES, 16)?;
        let (shared, _) = top::<Home>(self.shared)?;
        let scan = Parts(shared).fixed::<4099>()?;
        operation(query, scan)
    }
}
