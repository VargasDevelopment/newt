// SPDX-License-Identifier: MIT
//! The embedded reader's one owned Storage allocation. No source, parser,
//! backend, SDK frame or callback is retained between short sample operations.
use core::{mem::{MaybeUninit, size_of}, ptr::{addr_of_mut, drop_in_place, write_bytes}};
use newt_core::{preferences::ViewSettings, progress::{Progress, initial_progress}, tracker::ReadingTracker};
use newt_phased_reader::{bounded::{self, BOOK_BYTES, LAYOUT_BYTES, PARSE_BYTES},
    phase_storage::{bounded::MetadataState, bounded_chapters::Work},
    xml_session::ParserBackend, zip_container::Status};
use newt_sdk_heap::AlignedRegion;

#[repr(C, align(16))]
struct Storage {
    book: [u8; BOOK_BYTES],
    layout: [u8; LAYOUT_BYTES],
    parse: [u8; PARSE_BYTES],
    reader: bounded::Reader,
    metadata: MetadataState,
    tracker: ReadingTracker,
}
pub const STORAGE_BYTES: usize = size_of::<Storage>();

pub struct SampleMemory { region: AlignedRegion<STORAGE_BYTES, 16> }
pub struct View<'a> {
    pub reader: &'a bounded::Reader,
    pub tracker: &'a ReadingTracker,
    pub title: Option<&'a [u8]>,
    pub spines: u32,
}
pub struct Loan<'a, 'book, 'storage> {
    pub reader: &'a mut bounded::Reader,
    pub metadata: &'a bounded::Metadata<'book, 'storage>,
    pub layout: &'a mut [u8; LAYOUT_BYTES],
    pub parse: &'a mut [u8; PARSE_BYTES],
    pub tracker: &'a mut ReadingTracker,
}

impl SampleMemory {
    pub const fn allocation_bytes() -> Option<usize> {
        AlignedRegion::<STORAGE_BYTES, 16>::allocation_bytes()
    }
    fn storage_mut(&mut self) -> &mut Storage {
        // SAFETY: construction initializes this private extent once; mutable
        // self excludes every view, operation and deallocation for this loan.
        unsafe { &mut *self.region.uninit().as_mut_ptr().cast::<Storage>() }
    }

    pub fn with_tracker<R>(&mut self, operation: impl FnOnce(&mut ReadingTracker) -> R) -> R {
        operation(&mut self.storage_mut().tracker)
    }

    /// The parser/page operation has ended before sleep lends this same bank.
    pub fn with_layout_scratch<R>(&mut self, operation: impl FnOnce(&mut [u8]) -> R) -> R {
        operation(&mut self.storage_mut().layout)
    }
    /// Identity is the real embedded source's required identity, never a
    /// replacement digest or a default supplied after failed source admission.
    pub fn try_new(saved: Progress, settings: ViewSettings, identity: u64) -> Option<Self> {
        let mut region = AlignedRegion::try_new()?;
        let storage = region.uninit().as_mut_ptr().cast::<Storage>();
        // SAFETY: one correctly aligned Storage extent, not yet published.
        // Banks become valid byte arrays and scalar fields are initialized
        // individually, avoiding a whole 70 KB constructor temporary.
        unsafe {
            write_bytes(addr_of_mut!((*storage).book).cast::<u8>(), 0, BOOK_BYTES);
            write_bytes(addr_of_mut!((*storage).layout).cast::<u8>(), 0, LAYOUT_BYTES);
            write_bytes(addr_of_mut!((*storage).parse).cast::<u8>(), 0, PARSE_BYTES);
            initialize_reader(
                &mut *addr_of_mut!((*storage).reader).cast::<MaybeUninit<bounded::Reader>>(), settings);
            addr_of_mut!((*storage).metadata).write(MetadataState::default());
            addr_of_mut!((*storage).tracker).write(ReadingTracker::new(initial_progress(saved, identity)));
        }
        Some(Self { region })
    }

    pub fn load<B: ParserBackend + ?Sized>(&mut self, epub: &[u8], backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        let Storage { book, layout, parse, reader, metadata, .. } = self.storage_mut();
        reader.close();
        let status = metadata.load(book, parse, &mut bounded::source::Source(epub), backend);
        if status != Status::Ok { return status; }
        // SAFETY: load just initialized these typed arrays in this same bank;
        // only this owning allocation can lend or mutate it.
        let indexed = unsafe { metadata.with_metadata(book, |metadata| {
            let mut chapters = Work::new(epub, layout, parse, backend, breaker);
            reader.index(metadata, &mut chapters)
        }) };
        match indexed { Ok(status) | Err(status) => status }
    }

    pub fn view(&self) -> View<'_> {
        // SAFETY: the private Storage stays initialized from construction to
        // Drop; shared self excludes load, mutable loans and deallocation.
        let storage = unsafe { &*self.region.uninit_ref().as_ptr().cast::<Storage>() };
        View { reader: &storage.reader, tracker: &storage.tracker,
            title: if storage.reader.opened() { storage.metadata.title(&storage.book) } else { None },
            spines: if storage.reader.opened() { storage.metadata.spine_count() } else { 0 } }
    }

    /// Operations borrow the real reader, tracker and original metadata while
    /// the same layout/parse banks are available for each chapter call.
    pub fn with<R>(&mut self, operation: impl for<'a, 'book, 'storage>
        FnOnce(Loan<'a, 'book, 'storage>) -> R,
    ) -> Result<R, Status> {
        let Storage { book, reader, metadata, layout, parse, tracker } = self.storage_mut();
        // SAFETY: state and typed bank never leave their one owning allocation.
        unsafe { metadata.with_metadata(book, |metadata|
            operation(Loan { reader, metadata, layout, parse, tracker })) }
    }
}
impl Drop for SampleMemory {
    fn drop(&mut self) {
        // SAFETY: exactly the initialized Storage; every loan ended before
        // exclusive Drop, and the region frees its raw allocation afterwards.
        unsafe { drop_in_place(self.region.uninit().as_mut_ptr().cast::<Storage>()); }
    }
}

/// Avoid materializing the two navigation projections on the loop-task stack.
#[inline(never)]
fn initialize_reader(output: &mut MaybeUninit<bounded::Reader>, settings: ViewSettings) {
    use bounded::{initialization as fields, MAX_ENTRIES};
    let reader = output.as_mut_ptr().cast::<u8>();
    // SAFETY: this is one exclusive, aligned Reader slot. Exhaustive field and
    // type assertions accompany its compiler offsets; no Reader is lent early.
    unsafe {
        write_bytes(reader.add(fields::CHAPTER_CHARACTERS).cast::<u32>(), 0, MAX_ENTRIES);
        reader.add(fields::BOOK_CHARACTERS).cast::<u64>().write(0);
        reader.add(fields::SETTINGS).cast::<ViewSettings>().write(settings);
        reader.add(fields::INFO).cast::<newt_phased_reader::PageInfo>().write(Default::default());
        let navigation = reader.add(fields::NAVIGATION).cast::<MaybeUninit<fields::Navigation>>();
        for index in 0..2 { super::selection_init::initialize_navigation(&mut *navigation.add(index)); }
        reader.add(fields::ACTIVE).cast::<usize>().write(0);
        reader.add(fields::OPENED).cast::<bool>().write(false);
    }
}
const _: () = assert!(!core::mem::needs_drop::<bounded::Reader>());

#[cfg(test)]
mod tests {
    use super::*;
    use newt_core::preferences::Orientation;

    #[test]
    fn placed_reader_matches_constructor_on_poisoned_storage() {
        for orientation in [Orientation::Portrait, Orientation::LandscapeCcw] {
            for slim_progress_bar in [false, true] {
                for refresh_pages in [1, 5, 10] {
                    let settings = ViewSettings { orientation, slim_progress_bar, refresh_pages };
                    let mut slot = MaybeUninit::<bounded::Reader>::uninit();
                    // SAFETY: bytes are poisoned while still uninitialized;
                    // the complete initializer establishes Reader afterwards.
                    unsafe { write_bytes(slot.as_mut_ptr().cast::<u8>(), 0xa5, size_of::<bounded::Reader>()); }
                    initialize_reader(&mut slot, settings);
                    // SAFETY: initialize_reader initialized every typed field.
                    let actual = unsafe { slot.assume_init() };
                    let expected = bounded::Reader::new(settings);
                    assert_eq!(actual.settings(), expected.settings());
                    assert_eq!(actual.info(), expected.info());
                    assert_eq!(actual.opened(), expected.opened());
                    assert_eq!(actual.current_section(), expected.current_section());
                    let (a, b) = (actual.progress_inputs(), expected.progress_inputs());
                    assert_eq!((a.before, a.current, a.total, a.index, a.count, a.valid),
                        (b.before, b.current, b.total, b.index, b.count, b.valid));
                    assert_eq!(actual.chapter_menu().info(), expected.chapter_menu().info());
                }
            }
        }
    }
}
