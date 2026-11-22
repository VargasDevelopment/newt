//! Bounded ZIP central-directory window reader. MIT.
//! The source binding/content must remain immutable for the whole scope;
//! direct fallback intentionally has no repeated validation or yield calls.
use crate::zip_window::{
    arena::{Arena, ByteRange},
    source::BookSource,
};
pub const WINDOW_BYTES: usize = 4096;

pub struct ZipDirectoryWindow<'scope, 'storage, S: BookSource + ?Sized> {
    source: &'scope mut S,
    arena: &'scope mut Arena<'storage>,
    mark: usize,
    begin: u64,
    end: u64,
    source_bytes: u64,
    on_yield: Option<fn(&mut S)>,
    buffer: Option<ByteRange>,
    buffer_at: u64,
    valid: usize,
    failed: bool,
}
impl<'scope, 'storage, S: BookSource + ?Sized> ZipDirectoryWindow<'scope, 'storage, S> {
    pub fn new(
        source: &'scope mut S,
        arena: &'scope mut Arena<'storage>,
        begin: u64,
        end: u64,
        on_yield: Option<fn(&mut S)>,
    ) -> Self {
        let mark = arena.mark();
        let source_bytes = source.size();
        let mut window = Self {
            source,
            arena,
            mark,
            begin,
            end,
            source_bytes,
            on_yield,
            buffer: None,
            buffer_at: 0,
            valid: 0,
            failed: false,
        };
        if begin > end || end > source_bytes {
            window.failed = true;
            return window;
        }
        if window.arena.capacity() - window.arena.used() >= WINDOW_BYTES && window.probe(begin) == 0
        {
            window.buffer = window.arena.alloc(WINDOW_BYTES, 1);
        }
        window
    }
    /// Read only the pre-directory region without exposing the cached source binding.
    /// The BookSource implementor must keep content/size stable for this scope.
    pub fn read_before_directory(&mut self, at: u64, out: &mut [u8]) -> bool {
        if at > self.begin || out.len() as u64 > self.begin - at { return false; }
        // Use the inspect read loop, not window's 4096-byte fallback cap.
        let mut done = 0;
        while done < out.len() {
            let got = self.source.read_at(at + done as u64, &mut out[done..]);
            if got <= 0 || got as usize > out.len() - done { return false; }
            done += got as usize;
        }
        true
    }
    pub fn arena_used(&self) -> usize {
        self.arena.used()
    }
    pub fn arena_high_water(&self) -> usize {
        self.arena.high_water()
    }
    pub fn arena_failed_alloc_size(&self) -> usize {
        self.arena.failed_alloc_size()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn cached(&self) -> bool {
        self.buffer.is_some()
    }
    fn fail(&mut self) -> bool {
        self.failed = true;
        self.valid = 0;
        false
    }
    fn probe(&mut self, at: u64) -> i32 {
        let mut unused = [0u8];
        self.source.read_at(at, &mut unused[..0])
    }
    fn valid_source(&mut self, at: u64) -> bool {
        if self.source.size() != self.source_bytes || self.probe(at) != 0 {
            return self.fail();
        }
        true
    }
    /// None plus nonzero bytes models the original invalid null destination.
    /// A too-small output slice is a nonsticky safe API rejection.
    pub fn read(&mut self, mut at: u64, output: Option<&mut [u8]>, bytes: u32) -> bool {
        if self.failed
            || at < self.begin
            || at > self.end
            || u64::from(bytes) > self.end - at
            || (bytes != 0 && output.as_ref().is_none_or(|out| out.len() < bytes as usize))
        {
            return false;
        }
        let mut empty = [];
        let out = output.unwrap_or(&mut empty);
        let out = &mut out[..bytes as usize];
        let Some(buffer) = self.buffer else {
            if !read_fully(self.source, at, out) {
                return self.fail();
            }
            return true;
        };
        let mut copied = 0;
        while copied < out.len() {
            if !self.valid_source(at) {
                return false;
            }
            if let Some(yield_fn) = self.on_yield {
                yield_fn(self.source);
            }
            if !self.valid_source(at) {
                return false;
            }
            if at < self.buffer_at || at - self.buffer_at >= self.valid as u64 {
                self.valid = 0;
                let take = (self.end - at).min(WINDOW_BYTES as u64) as usize;
                let scratch = self.arena.view_mut(buffer).expect("exclusive window range");
                if !read_fully(self.source, at, &mut scratch[..take]) {
                    return self.fail();
                }
                self.buffer_at = at;
                self.valid = take;
                if !self.valid_source(at) {
                    return false;
                }
            }
            let offset = (at - self.buffer_at) as usize;
            let take = (out.len() - copied).min(self.valid - offset);
            let scratch = self.arena.view(buffer).expect("exclusive window range");
            out[copied..copied + take].copy_from_slice(&scratch[offset..offset + take]);
            at += take as u64;
            copied += take;
        }
        true
    }
}
impl<S: BookSource + ?Sized> Drop for ZipDirectoryWindow<'_, '_, S> {
    fn drop(&mut self) {
        self.arena.release(self.mark);
    }
}
fn read_fully<S: BookSource + ?Sized>(source: &mut S, mut at: u64, mut out: &mut [u8]) -> bool {
    while !out.is_empty() {
        let take = out.len().min(WINDOW_BYTES);
        let got = source.read_at(at, &mut out[..take]);
        if got <= 0 || got as usize > take {
            return false;
        }
        at += got as u64;
        out = &mut out[got as usize..];
    }
    true
}
