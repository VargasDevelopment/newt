// SPDX-License-Identifier: MIT
//! The four frozen ExternalStorage allocations, owned between short loans.
use core::{mem::{ManuallyDrop, MaybeUninit, align_of, size_of}, ptr::{addr_of_mut, drop_in_place, write_bytes}};
use newt_phased_reader::{PageLayout, PhasedReader, ReaderState, ReaderView, Selection,
    phase_storage::{BANK_BYTES, Storage, StorageState}};
use newt_phased_sink::{Plan, PlanState, Run, RUNS, TEXT};
use newt_reader_ui::{ExternalOwner, ReaderUiState};
use newt_sdk_heap::AlignedRegion;

/// Actual operations share a short view of the original backing.
pub struct Runtime<'m, 'p> {
    pub reader: PhasedReader<'m, 'p>,
    pub owner: ExternalOwner,
    pub ui: ReaderUiState,
}
impl<'m, 'p> Runtime<'m, 'p> {
    pub fn new(selections: &'m mut [Selection; 2], plan: &'m mut Plan<'p>) -> Self {
        Self { reader: PhasedReader::new(selections, Some(plan), PageLayout::PORTRAIT),
            owner: ExternalOwner::default(), ui: ReaderUiState::default() }
    }
}

// Destruction order: selections, plan, shared, then the enclosing allocation.
const SELECTION_BYTES: usize = size_of::<[Selection; 2]>();
struct Regions {
    selections: AlignedRegion<SELECTION_BYTES, 16>,
    plan: AlignedRegion<9_616, 16>,
    shared: AlignedRegion<BANK_BYTES, 16>,
}
struct Retained {
    reader: ReaderState,
    owner: ExternalOwner,
    ui: ReaderUiState,
    plan: PlanState,
    storage: StorageState,
}
const RUNTIME_ALIGN: usize = align_of::<Runtime<'_, '_>>();
const VALUE_ALIGN: usize = if align_of::<Retained>() > RUNTIME_ALIGN { align_of::<Retained>() } else { RUNTIME_ALIGN };
const VALUE_AT: usize = (size_of::<Regions>() + VALUE_ALIGN - 1) & !(VALUE_ALIGN - 1);
const VALUE_BYTES: usize = if size_of::<Retained>() > size_of::<Runtime<'_, '_>>() {
    size_of::<Retained>()
} else { size_of::<Runtime<'_, '_>>() };
const OCCUPIED: usize = VALUE_AT + VALUE_BYTES;
pub const OUTER_BYTES: usize = if OCCUPIED > 400 { OCCUPIED } else { 400 };

/// Compatibility loan for callers consuming an owner once.
pub struct Memory<'bank, 'loan, 'm, 'p> {
    pub bank: &'bank mut [u8; BANK_BYTES],
    pub runtime: &'loan mut Runtime<'m, 'p>,
}
/// No backing reference or callback survives lend().
pub struct Loan<'m, 'p> {
    pub storage: Storage<'m>,
    pub runtime: &'m mut Runtime<'m, 'p>,
}
pub struct View<'a> {
    pub reader: ReaderView<'a>,
    pub owner: &'a ExternalOwner,
    pub ui: ReaderUiState,
}

const fn region<const N: usize, const A: usize>() -> usize {
    // An impossible overflow yields usize::MAX, so the const sum below fails to compile.
    match AlignedRegion::<N, A>::allocation_bytes() { Some(bytes) => bytes, None => usize::MAX }
}
/// Frozen reader_admission.h for this borrowed-parse layout: the heap bytes
/// of the four allocations, the largest of them, and that total plus the XML
/// budget and 32 KiB of headroom for the work that follows the open.
pub const ALLOCATION_BYTES: usize = region::<OUTER_BYTES, 8>() + region::<BANK_BYTES, 16>()
    + region::<9_616, 16>() + region::<SELECTION_BYTES, 16>();
pub const LARGEST_ALLOCATION_BYTES: usize = region::<BANK_BYTES, 16>();
pub const REQUIRED_HEAP_BYTES: usize = ALLOCATION_BYTES + 32 * 1024 + 32_768;

/// Why an open got no reader memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal { LargestBlock, FreeHeap, Allocation }
/// Frozen readerOwnerHeapBytes credits a bound owner as its size plus one
/// max_align_t allowance; the pinned allocator aligns every block to 16.
pub const OWNER_ALIGN_ALLOWANCE_BYTES: usize = 16;

/// The same owner word holds the same four exact frozen allocations.
pub struct ReaderMemory {
    outer: AlignedRegion<OUTER_BYTES, 8>,
}
impl ReaderMemory {
    /// Frozen ReaderRecovery::attempt, checked before any allocation: the
    /// largest free block must hold the largest allocation (checked first),
    /// then free heap plus a bound session's heap charge (`credit`) must cover
    /// REQUIRED_HEAP_BYTES.
    pub fn try_admitted(free: usize, largest: usize, credit: usize) -> Result<Self, Refusal> {
        if largest < LARGEST_ALLOCATION_BYTES { return Err(Refusal::LargestBlock); }
        if free.saturating_add(credit) < REQUIRED_HEAP_BYTES { return Err(Refusal::FreeHeap); }
        Self::try_new().ok_or(Refusal::Allocation)
    }

    pub fn try_new() -> Option<Self> {
        let mut outer = AlignedRegion::try_new()?;
        let mut shared = AlignedRegion::<BANK_BYTES, 16>::try_new();
        if let Some(shared) = shared.as_mut() { zero(shared); }
        // Frozen attempts plan after a failed shared allocation, and only
        // attempts selections with shared backing, even if the plan failed.
        let mut plan = AlignedRegion::<9_616, 16>::try_new();
        if let Some(plan) = plan.as_mut() { zero(plan); }
        let selections = shared.as_ref().and_then(|_| AlignedRegion::<SELECTION_BYTES, 16>::try_new());
        let (Some(selections), Some(plan), Some(shared)) = (selections, plan, shared) else { return None; };
        let raw = outer.uninit().as_mut_ptr().cast::<u8>();
        // SAFETY: the extent fits Regions and its disjoint value suffix. All
        // fields/selections are initialized once before publishing the owner.
        unsafe {
            let regions = raw.cast::<Regions>();
            addr_of_mut!((*regions).selections).write(selections);
            addr_of_mut!((*regions).plan).write(plan);
            addr_of_mut!((*regions).shared).write(shared);
            let slots = (*regions).selections.uninit().as_mut_ptr().cast::<MaybeUninit<Selection>>();
            super::selection_init::initialize(&mut *slots);
            super::selection_init::initialize(&mut *slots.add(1));
            raw.add(VALUE_AT).cast::<Retained>().write(Retained {
                reader: ReaderState::new(PageLayout::PORTRAIT), owner: ExternalOwner::default(),
                ui: ReaderUiState::default(), plan: PlanState::new(), storage: StorageState::new(),
            });
        }
        Some(Self { outer })
    }

    pub fn view(&self) -> View<'_> {
        let raw = self.outer.uninit_ref().as_ptr().cast::<u8>();
        // SAFETY: outside lend the suffix contains Retained and selections
        // are initialized. Shared self excludes a live mutable loan.
        unsafe {
            let regions = &*raw.cast::<Regions>();
            let retained = &*raw.add(VALUE_AT).cast::<Retained>();
            let selections = &*regions.selections.uninit_ref().as_ptr().cast::<[Selection; 2]>();
            View { reader: retained.reader.view(selections), owner: &retained.owner, ui: retained.ui }
        }
    }

    pub fn with<R>(mut self, operation: impl for<'bank, 'loan, 'm, 'p> FnOnce(Memory<'bank, 'loan, 'm, 'p>) -> R) -> R {
        self.lend(|loan| {
            let Loan { storage, runtime } = loan;
            storage.with_unpublished_backing(|bank| operation(Memory { bank, runtime: &mut **runtime }))
        })
    }

    pub fn lend<R>(&mut self, operation: impl for<'m, 'p> FnOnce(&mut Loan<'m, 'p>) -> R) -> R {
        let raw = self.outer.uninit().as_mut_ptr().cast::<u8>();
        // SAFETY: try_new initialized Regions and the sole retained value.
        // Exclusive self prevents a second loan, projection or deallocation.
        let (regions, retained) = unsafe {
            (&mut *raw.cast::<Regions>(), raw.add(VALUE_AT).cast::<Retained>())
        };
        let runs = regions.plan.uninit().as_mut_ptr().cast::<u8>();
        // SAFETY: plan bytes were zeroed once; disjoint integer runs/text and
        // both selections remain initialized in their original allocations.
        let (runs, text, selections, bank) = unsafe {
            (&mut *runs.cast::<[Run; RUNS]>(),
             &mut *runs.add(size_of::<[Run; RUNS]>()).cast::<[u8; TEXT]>(),
             &mut *regions.selections.uninit().as_mut_ptr().cast::<[Selection; 2]>(),
             &mut *regions.shared.uninit().as_mut_ptr().cast::<[u8; BANK_BYTES]>())
        };
        // SAFETY: move these disjoint fields before initialize replaces the
        // suffix. Reader/owner/UI move inside that short initialization frame,
        // so their temporary values do not occupy the callback's ancestor.
        let (plan, storage) = unsafe {
            (addr_of_mut!((*retained).plan).read(), addr_of_mut!((*retained).storage).read())
        };
        let mut plan = Plan::from_state(runs, text, plan);
        let plan_ptr = addr_of_mut!(plan);
        let bank_address = bank.as_ptr();
        // SAFETY: the same unchanged bank is paired with its own style state.
        let storage = unsafe { Storage::from_state(bank, storage) };
        // SAFETY: the suffix fits Runtime; initialize consumes the remaining
        // Retained fields before replacing them with the live runtime.
        let runtime = unsafe { initialize(retained, selections, &mut *plan_ptr) };
        let mut returned = Returned { loan: ManuallyDrop::new(Loan { storage, runtime }),
            retained, plan: plan_ptr, bank_address };
        operation(&mut returned.loan)
    }
}

fn zero<const N: usize, const A: usize>(region: &mut AlignedRegion<N, A>) {
    // SAFETY: exclusive initialization covers exactly the allocated payload.
    unsafe { write_bytes(region.uninit().as_mut_ptr(), 0, N); }
}

// A separate frame keeps state moves out of parser/layout stack depth.
// SAFETY: caller supplies the aligned suffix and the same initialized backing.
#[inline(never)]
unsafe fn initialize<'m, 'p>(raw: *mut Retained,
    selections: &'m mut [Selection; 2], plan: &'m mut Plan<'p>,
) -> &'m mut Runtime<'m, 'p> {
    let slot = raw.cast::<Runtime<'m, 'p>>();
    // SAFETY: read all remaining initialized fields before overwriting their
    // allocation; plan/storage were moved by the caller. Write each resulting
    // field once before borrowing the complete runtime.
    unsafe {
        let state = addr_of_mut!((*raw).reader).read();
        let owner = addr_of_mut!((*raw).owner).read();
        let ui = addr_of_mut!((*raw).ui).read();
        addr_of_mut!((*slot).reader).write(state.lend(selections, Some(plan)));
        addr_of_mut!((*slot).owner).write(owner);
        addr_of_mut!((*slot).ui).write(ui);
        &mut *slot
    }
}

struct Returned<'m, 'p> {
    loan: ManuallyDrop<Loan<'m, 'p>>,
    retained: *mut Retained,
    plan: *const Plan<'p>,
    bank_address: *const u8,
}
impl Drop for Returned<'_, '_> {
    // Return-state temporaries exist only after the operation loans have ended.
    #[inline(never)]
    fn drop(&mut self) {
        // SAFETY: runs once after every callback loan ends, including unwind.
        // Plan was constructed before this guard and remains live. Reading
        // Runtime moves its state out before replacing the same suffix with
        // no-reference Retained; this transition invokes no user code.
        unsafe {
            let Loan { storage, runtime } = ManuallyDrop::take(&mut self.loan);
            let Runtime { reader, owner, ui } = core::ptr::read(runtime);
            let (bank, state) = storage.into_parts();
            // Safe clients may replace their Storage value. A replacement
            // cannot transfer another bank's Home proof into this allocation.
            let storage = if bank.as_ptr() == self.bank_address { state } else { StorageState::new() };
            self.retained.write(Retained { reader: reader.into_state(), owner, ui,
                plan: (*self.plan).state(), storage });
        }
    }
}
impl Drop for ReaderMemory {
    fn drop(&mut self) {
        // SAFETY: every operation returned its no-reference state. Destroy it,
        // then selections/plan/shared, before freeing the enclosing allocation.
        unsafe {
            let raw = self.outer.uninit().as_mut_ptr().cast::<u8>();
            drop_in_place(raw.add(VALUE_AT).cast::<Retained>());
            drop_in_place(raw.cast::<Regions>());
        }
    }
}

const _: () = {
    assert!(align_of::<Regions>() <= 8 && VALUE_ALIGN <= 8);
    assert!(align_of::<Selection>() <= 16);
    assert!(size_of::<[Run; RUNS]>() + TEXT <= 9_616);
    assert!(!core::mem::needs_drop::<Selection>());
};
const _: () = assert!(LARGEST_ALLOCATION_BYTES >= region::<OUTER_BYTES, 8>()
    && LARGEST_ALLOCATION_BYTES >= region::<9_616, 16>()
    && LARGEST_ALLOCATION_BYTES >= region::<SELECTION_BYTES, 16>());
#[cfg(target_pointer_width = "32")]
const _: () = { assert!(OUTER_BYTES == 400); assert!(SELECTION_BYTES == 3_552);
    assert!(ALLOCATION_BYTES == 62_788); assert!(LARGEST_ALLOCATION_BYTES == 49_171); };

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, thread_local};

    thread_local! { static ATTEMPTS: Cell<usize> = const { Cell::new(0) }; }
    struct Counting;
    #[global_allocator]
    static COUNTING: Counting = Counting;
    // SAFETY: forwards every valid layout and live pointer to System unchanged;
    // the const TLS counter neither allocates nor recurses.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = ATTEMPTS.try_with(|count| count.set(count.get() + 1));
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) }
        }
    }
    fn attempts(free: usize, largest: usize, credit: usize) -> (Result<(), Refusal>, usize) {
        let before = ATTEMPTS.with(Cell::get);
        let result = ReaderMemory::try_admitted(free, largest, credit).map(drop);
        (result, ATTEMPTS.with(Cell::get) - before)
    }

    #[test]
    fn free_heap_one_byte_short_refuses_before_allocating() {
        assert_eq!(attempts(REQUIRED_HEAP_BYTES - 1, usize::MAX, 0), (Err(Refusal::FreeHeap), 0));
        // A bound session's bytes count toward the reserve, as frozen credits them.
        assert_eq!(attempts(REQUIRED_HEAP_BYTES - 101, usize::MAX, 100), (Err(Refusal::FreeHeap), 0));
    }

    #[test]
    fn largest_block_one_byte_short_refuses_first() {
        assert_eq!(attempts(usize::MAX, LARGEST_ALLOCATION_BYTES - 1, 0), (Err(Refusal::LargestBlock), 0));
        assert_eq!(attempts(0, LARGEST_ALLOCATION_BYTES - 1, 0), (Err(Refusal::LargestBlock), 0));
    }

    #[test]
    fn exact_requirements_admit_all_four_allocations() {
        assert_eq!(attempts(REQUIRED_HEAP_BYTES, LARGEST_ALLOCATION_BYTES, 0), (Ok(()), 4));
        assert_eq!(attempts(REQUIRED_HEAP_BYTES - 100, LARGEST_ALLOCATION_BYTES, 100), (Ok(()), 4));
    }
}
