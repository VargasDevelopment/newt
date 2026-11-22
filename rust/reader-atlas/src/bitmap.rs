//! Lossless flash storage for the original two-bit coverage bytes.
use core::ops::Index;

pub(super) struct Bitmap {
    blocks: &'static [u16],
    len: usize,
}
const _: () = assert!(core::mem::size_of::<Bitmap>() == 3 * core::mem::size_of::<usize>());
impl Bitmap {
    pub(super) const fn new(blocks: &'static [u16], len: usize) -> Self {
        assert!(blocks.len() == len.div_ceil(4));
        Self { blocks, len }
    }
    pub(super) fn view(&'static self) -> View {
        View { bitmap: self, start: 0 }
    }
}

/// A transient slice view; glyph offsets still name original coverage bytes.
pub(super) struct View {
    bitmap: &'static Bitmap,
    start: usize,
}
impl View {
    pub(super) fn tail(&self, at: usize) -> Self {
        assert!(at <= self.bitmap.len - self.start);
        Self { bitmap: self.bitmap, start: self.start + at }
    }
}
impl Index<usize> for View {
    type Output = u8;
    fn index(&self, at: usize) -> &Self::Output {
        assert!(at < self.bitmap.len - self.start);
        let offset = self.start + at;
        let block = usize::from(self.bitmap.blocks[offset / 4]);
        &super::data::BITMAP_BLOCKS[block * 4 + offset % 4]
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    #[test]
    fn slices_preserve_original_bounds_and_partial_last_block() {
        static BITMAP: Bitmap = Bitmap::new(&[0, 0], 5);
        let view = BITMAP.view();
        assert_eq!(view[4], super::super::data::BITMAP_BLOCKS[0]);
        assert!(std::panic::catch_unwind(|| view[5]).is_err());
        assert!(std::panic::catch_unwind(|| view.tail(6)).is_err());
        assert!(std::panic::catch_unwind(|| view.tail(5)[0]).is_err());
    }
}
