use super::*;

/// One existing image/window lifetime. The current descriptor closes on drop,
/// including a descriptor retained by a failed open/stat, just like frozen CardImage.
pub struct CardImage<'a> {
    files: &'a mut dyn Files,
    window: &'a mut [u8; WINDOW_BYTES],
    yielding: &'a mut dyn Yield,
    fd: i32,
    size: u32,
    window_at: u32,
    window_bytes: u32,
    reads: u32,
}
impl<'a> CardImage<'a> {
    pub fn new(files: &'a mut dyn Files, window: &'a mut [u8; WINDOW_BYTES], yielding: &'a mut dyn Yield) -> Self {
        Self { files, window, yielding, fd: -1, size: 0, window_at: 0, window_bytes: 0, reads: 0 }
    }
    pub fn open(&mut self, offer: &UpdateOffer) -> bool {
        if offer.manifest.kind == UpdateKind::Firmware {
            let path = firmware_path(&offer.file);
            let Ok(path) = CStr::from_bytes_until_nul(&path) else { return false };
            self.fd = self.files.open(path);
        } else { self.fd = self.files.open(NEWT_IMAGE); }
        if self.fd < 0 { return false; }
        let Some(info) = self.files.stat(self.fd) else { return false };
        if !info.regular || info.size < 0 || info.size > i64::from(APP_SLOT_BYTES) { return false; }
        self.size = info.size as u32;
        true
    }
    pub fn size(&self) -> u32 { self.size }
    pub fn read(&mut self, mut at: u32, mut out: &mut [u8]) -> bool {
        let Ok(count) = u32::try_from(out.len()) else { return false };
        if self.fd < 0 || count > self.size || at > self.size - count { return false; }
        if out.len() >= WINDOW_BYTES {
            return read_at(self.files, self.fd, &mut self.reads, self.yielding, at, out);
        }
        while !out.is_empty() {
            if at < self.window_at || at >= self.window_at + self.window_bytes {
                let start = at / WINDOW_BYTES as u32 * WINDOW_BYTES as u32;
                let bytes = (self.size - start).min(WINDOW_BYTES as u32);
                self.window_bytes = 0;
                if !read_at(self.files, self.fd, &mut self.reads, self.yielding, start, &mut self.window[..bytes as usize]) { return false; }
                self.window_at = start;
                self.window_bytes = bytes;
            }
            let offset = at - self.window_at;
            let part = (self.window_bytes - offset).min(out.len() as u32) as usize;
            out[..part].copy_from_slice(&self.window[offset as usize..offset as usize + part]);
            out = &mut out[part..];
            at += part as u32;
        }
        true
    }
}
fn read_at(files: &mut dyn Files, fd: i32, reads: &mut u32, yielding: &mut dyn Yield, at: u32, mut remaining: &mut [u8]) -> bool {
    if files.seek(fd, at) != i64::from(at) { return false; }
    while !remaining.is_empty() {
        let got = files.read(fd, remaining);
        if got <= 0 || got as usize > remaining.len() { return false; }
        remaining = &mut remaining[got as usize..];
    }
    *reads = reads.wrapping_add(1);
    if *reads % 16 == 0 { yielding.yield_now(); }
    true
}
impl Drop for CardImage<'_> { fn drop(&mut self) { if self.fd >= 0 { self.files.close(self.fd); } } }
