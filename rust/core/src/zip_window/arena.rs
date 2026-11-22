//! Safe byte-only translation of the pinned FreeInk FontArena (MIT).
//! Offsets align relative to the start, not to the backing slice's address.
//! Handles belong to one arena initialization and one nested allocation scope.
//! Release/reset/init invalidate affected handles. Views are borrow-scoped;
//! callers must keep source/output and arena storage in disjoint domains.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    start: usize,
    len: usize,
}
impl ByteRange {
    pub fn offset(self) -> usize {
        self.start
    }
    pub fn len(self) -> usize {
        self.len
    }
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// Pointer-sized C-string offset. Like ByteRange, it belongs to one arena
/// initialization/scope. It is not a durable record or an address to dereference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CStringRef(core::num::NonZeroUsize);
impl CStringRef {
    pub fn offset(self) -> usize {
        self.0.get() - 1
    }
}

/// Scalar byte-allocator continuation, without references or stored pointers.
/// This neither owns the backing bytes nor proves typed-object validity.
#[derive(Debug, Eq, PartialEq)]
pub struct ArenaState {
    capacity: usize,
    used: usize,
    high_water: usize,
    failed_alloc_size: usize,
}

#[derive(Default)]
pub struct Arena<'a> {
    storage: Option<&'a mut [u8]>,
    used: usize,
    high_water: usize,
    failed_alloc_size: usize,
}
impl<'a> Arena<'a> {
    pub fn new(storage: &'a mut [u8]) -> Self {
        Self {
            storage: Some(storage),
            ..Self::default()
        }
    }
    pub fn init(&mut self, storage: &'a mut [u8]) {
        *self = Self::new(storage);
    }
    /// Record allocation offsets and diagnostics between short backing loans.
    pub fn state(&self) -> ArenaState {
        ArenaState {
            capacity: self.capacity(),
            used: self.used,
            high_water: self.high_water,
            failed_alloc_size: self.failed_alloc_size,
        }
    }
    /// Resume the byte allocator on the exact recorded storage extent.
    /// Existing handles remain in the caller's logical ownership scope only
    /// when the same bank and allocated bytes remain unchanged. This does not
    /// establish bank provenance, typed-object validity or other capabilities.
    pub fn from_state(storage: &'a mut [u8], state: ArenaState) -> Result<Self, ArenaState> {
        if storage.len() != state.capacity
            || state.used > state.capacity
            || state.used > state.high_water
        {
            return Err(state);
        }
        // A withdrawn tail can leave historical high-water above capacity.
        Ok(Self {
            storage: Some(storage),
            used: state.used,
            high_water: state.high_water,
            failed_alloc_size: state.failed_alloc_size,
        })
    }
    /// Freeze existing handles while lending the disjoint unallocated suffix.
    /// Neither metadata allocation nor a suffix reference can escape the scope.
    pub fn with_unused_suffix<R>(
        &mut self,
        operation: impl FnOnce(&Arena<'_>, &mut [u8]) -> R,
    ) -> R {
        let used = self.used;
        let (prefix, suffix) = self
            .storage
            .as_deref_mut()
            .unwrap_or(&mut [])
            .split_at_mut(used);
        let frozen = Arena {
            storage: Some(prefix),
            used,
            high_water: self.high_water,
            failed_alloc_size: self.failed_alloc_size,
        };
        operation(&frozen, suffix)
    }
    pub fn capacity(&self) -> usize {
        self.storage.as_ref().map_or(0, |s| s.len())
    }
    /// Withdraw disjoint, aligned backing from the unused tail. Existing string
    /// handles stay at their offsets; future byte allocations cannot alias it.
    /// The caller initializes typed objects only within the returned bytes.
    pub fn take_tail(&mut self, size: usize, alignment: usize) -> Option<&'a mut [u8]> {
        if !alignment.is_power_of_two() {
            return None;
        }
        let storage = self.storage.as_ref()?;
        let base = storage.as_ptr() as usize;
        let available = storage.len().checked_sub(size)?;
        let address = base.checked_add(available)? & !(alignment - 1);
        let at = address.checked_sub(base)?;
        if at < self.used {
            return None;
        }
        let storage = self.storage.take()?;
        let (prefix, tail) = storage.split_at_mut(at);
        self.storage = Some(prefix);
        Some(&mut tail[..size])
    }
    /// Reborrow a temporary aligned tail. Callback references cannot escape;
    /// used bytes and diagnostics survive, while full capacity returns on exit.
    pub fn with_tail<R>(
        &mut self,
        size: usize,
        alignment: usize,
        operation: impl FnOnce(&mut Arena<'_>, &mut [u8]) -> R,
    ) -> Option<R> {
        if !alignment.is_power_of_two() {
            return None;
        }
        let storage = self.storage.as_deref_mut()?;
        let base = storage.as_ptr() as usize;
        let available = storage.len().checked_sub(size)?;
        let address = base.checked_add(available)? & !(alignment - 1);
        let at = address.checked_sub(base)?;
        if at < self.used {
            return None;
        }
        let (prefix, tail) = storage.split_at_mut(at);
        let mut scoped = Arena {
            storage: Some(prefix),
            used: self.used,
            high_water: self.high_water,
            failed_alloc_size: self.failed_alloc_size,
        };
        let result = operation(&mut scoped, &mut tail[..size]);
        self.used = scoped.used;
        self.high_water = scoped.high_water;
        self.failed_alloc_size = scoped.failed_alloc_size;
        Some(result)
    }
    pub fn used(&self) -> usize {
        self.used
    }
    pub fn mark(&self) -> usize {
        self.used
    }
    pub fn high_water(&self) -> usize {
        self.high_water
    }
    pub fn failed_alloc_size(&self) -> usize {
        self.failed_alloc_size
    }
    pub fn reset(&mut self) {
        self.used = 0;
    }
    pub fn release(&mut self, mark: usize) {
        if mark <= self.used {
            self.used = mark;
        }
    }
    // Alignment is explicit: byte ranges make no typed alignment promise.
    pub fn alloc(&mut self, size: usize, align: usize) -> Option<ByteRange> {
        if self.storage.is_none() || !align.is_power_of_two() {
            return None;
        }
        let aligned = self.used.checked_add(align - 1).map(|v| v & !(align - 1));
        let start = match aligned {
            Some(v) if size <= self.capacity() && v <= self.capacity() - size => v,
            _ => {
                self.failed_alloc_size = self.failed_alloc_size.max(size);
                return None;
            }
        };
        self.used = start + size;
        self.high_water = self.high_water.max(self.used);
        Some(ByteRange { start, len: size })
    }
    // Equivalent byte count only; this does not construct T or return typed objects.
    // Multiplication overflow, like allocArray<T>, does not alter OOM diagnostics.
    pub fn alloc_array_bytes(
        &mut self,
        count: usize,
        element_bytes: usize,
        align: usize,
    ) -> Option<ByteRange> {
        self.alloc(count.checked_mul(element_bytes)?, align)
    }
    pub fn view(&self, range: ByteRange) -> Option<&[u8]> {
        let end = range.start.checked_add(range.len)?;
        if end > self.used {
            return None;
        }
        self.storage.as_ref()?.get(range.start..end)
    }
    pub fn view_mut(&mut self, range: ByteRange) -> Option<&mut [u8]> {
        let end = range.start.checked_add(range.len)?;
        if end > self.used {
            return None;
        }
        self.storage.as_mut()?.get_mut(range.start..end)
    }
    // Explicit length variant copies embedded NULs, then appends one NUL.
    // None models nullptr. Unrepresentable len+1 is rejected safely.
    pub fn copy_nul(&mut self, input: Option<&[u8]>) -> Option<ByteRange> {
        let input = input?;
        let range = self.alloc(input.len().checked_add(1)?, 1)?;
        let out = self.view_mut(range)?;
        out[..input.len()].copy_from_slice(input);
        out[input.len()] = 0;
        Some(range)
    }
    /// The terminating byte must belong to the supplied allocation range.
    pub fn string_ref(&self, range: ByteRange) -> Option<CStringRef> {
        if self.view(range)?.last() != Some(&0) {
            return None;
        }
        Some(CStringRef(core::num::NonZeroUsize::new(
            range.start.checked_add(1)?,
        )?))
    }
    pub fn copy_nul_ref(&mut self, input: Option<&[u8]>) -> Option<CStringRef> {
        let range = self.copy_nul(input)?;
        self.string_ref(range)
    }
    /// Bounded by current used storage; never returns the terminator itself.
    pub fn c_string(&self, value: CStringRef) -> Option<&[u8]> {
        let bytes = self.storage.as_ref()?.get(value.offset()..self.used)?;
        let len = bytes.iter().position(|byte| *byte == 0)?;
        Some(&bytes[..len])
    }
    /// Compare with already bounded content (excluding its NUL). Equality
    /// requires a stored terminator; mismatches need not measure the field.
    pub fn c_string_eq(&self, value: CStringRef, expected: &[u8]) -> bool {
        let Some(bytes) = self.storage.as_ref().and_then(|s| s.get(value.offset()..self.used))
        else {
            return false;
        };
        bytes.get(expected.len()) == Some(&0) && bytes.starts_with(expected)
    }
    // Models strlen: requires an accessible terminator, never scans beyond input.
    pub fn copy_c_string(&mut self, input: Option<&[u8]>) -> Option<ByteRange> {
        let input = input?;
        let len = input.iter().position(|b| *b == 0)?;
        self.copy_nul(Some(&input[..len]))
    }
}

#[cfg(test)]
#[path = "arena_scope_tests.rs"]
mod scope_tests;

#[cfg(test)]
#[path = "arena_state_tests.rs"]
mod state_tests;
