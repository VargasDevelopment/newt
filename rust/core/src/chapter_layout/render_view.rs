//! Borrowed offset-to-slice interpretation. No page metadata or text is copied.
use crate::{
    page_renderer::{PageImage, PageTextRun, PageView, SafetyError},
    chapter_layout::types::{Bytes, Page},
};
fn bytes(pool: &[u8], b: Bytes) -> Result<&[u8], SafetyError> {
    let start = b.start as usize;
    let end = start
        .checked_add(usize::from(b.len))
        .ok_or(SafetyError::TextLength)?;
    pool.get(start..end).ok_or(SafetyError::TextLength)
}
impl PageView for Page<'_> {
    fn run_count(&self) -> usize {
        self.runs.len()
    }
    fn image_count(&self) -> usize {
        self.images.len()
    }
    fn link_count(&self) -> usize {
        self.links.len()
    }
    fn run(&self, index: usize) -> Result<PageTextRun<'_>, SafetyError> {
        let r = self.runs.get(index).ok_or(SafetyError::PageCount)?;
        Ok(PageTextRun {
            text: bytes(self.bytes, r.text)?,
            x: r.x,
            baseline_y: r.baseline,
            size_px: r.size,
            style_flags: r.flags,
        })
    }
    fn image(&self, index: usize) -> Result<PageImage<'_>, SafetyError> {
        let r = self.images.get(index).ok_or(SafetyError::PageCount)?;
        Ok(PageImage {
            href: bytes(self.bytes, r.href)?,
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        })
    }
}
