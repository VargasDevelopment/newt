#![forbid(unsafe_code)]
// A library compile probe: runtime borrowed input/output/storage, no static
// promotion or owned cache. Compile as a library, not as a test harness.
use newt_core::zip_window::{arena::Arena, source::BookSource, window::ZipDirectoryWindow};
struct Borrowed<'a>(&'a [u8]);
impl BookSource for Borrowed<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let Some(tail) = self.0.get(at..) else {
            return -1;
        };
        let n = out.len().min(tail.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
pub fn read_borrowed(input: &[u8], storage: &mut [u8], output: &mut [u8], at: u64) -> bool {
    let Ok(bytes) = u32::try_from(output.len()) else {
        return false;
    };
    let mut source = Borrowed(input);
    let mut arena = Arena::new(storage);
    let mut window = ZipDirectoryWindow::new(&mut source, &mut arena, 0, input.len() as u64, None);
    window.read(at, Some(output), bytes)
}
