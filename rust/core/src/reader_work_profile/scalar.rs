use core::cell::Cell;

/// Read-only public observation of one UI-owned primitive. Mutation is confined
/// to this profile; Cell keeps the same layout and rejects cross-task sharing.
#[derive(Default)]
#[repr(transparent)]
pub struct Scalar<T: Copy>(Cell<T>);

impl<T: Copy> Scalar<T> {
    pub fn get(&self) -> T { self.0.get() }
    pub(super) const fn new(value: T) -> Self { Self(Cell::new(value)) }
    pub(super) fn set(&self, value: T) { self.0.set(value); }
}

impl<T: Copy + Default> Scalar<T> {
    pub(super) fn reset(&self) { self.set(T::default()); }
}

impl Scalar<u32> {
    pub(super) fn add(&self, value: u32) { self.set(self.get().wrapping_add(value)); }
}
