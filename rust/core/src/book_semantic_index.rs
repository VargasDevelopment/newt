//! Policy-1 counts from authenticated complete SMALL-profile parser events.
//! `finish` does not certify EOF, exact chapter consumption, or source validity.
pub const MAX_SPINE_ITEMS: usize = 1024;
/// 2: each non-decorative picture counts one position (#179).
pub const POLICY_VERSION: u32 = 2;
/// The last policy before pictures contributed a reading position.
pub const PRE_PICTURE_POLICY_VERSION: u32 = 1;
pub type Digest = [u8; 32];
use crate::chapter_layout::types::PictureNote;

#[derive(Default)]
pub struct Counter {
    hidden: [bool; 16],
    characters: u32,
    overflow_depth: u32,
    suppressed: u32,
    paragraph_bytes: u16,
    stack_top: usize,
    in_body: bool,
    pending_space: bool,
    failed: bool,
    finished: bool,
}

fn local(raw: &[u8]) -> &[u8] {
    // Match C strings, including last-colon namespace handling.
    let raw = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
    raw.rsplit(|&b| b == b':').next().unwrap_or_default()
}
fn suppressed(name: &[u8]) -> bool {
    matches!(name, b"head" | b"style" | b"script" | b"title")
}
fn block(name: &[u8]) -> bool {
    matches!(
        name,
        b"p" | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"blockquote"
            | b"li"
            | b"div"
            | b"section"
            | b"article"
            | b"figure"
            | b"aside"
            | b"figcaption"
            | b"ul"
            | b"ol"
            | b"table"
            | b"tr"
            | b"td"
            | b"th"
            | b"dt"
            | b"dd"
    )
}
impl Counter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    fn boundary(&mut self) {
        self.paragraph_bytes = 0;
        self.pending_space = false;
    }
    fn append(&mut self, byte: u8) {
        if self.paragraph_bytes == 3071 {
            self.boundary();
        }
        self.paragraph_bytes += 1;
        if byte & 0xc0 != 0x80 {
            self.count_position();
        }
    }
    fn count_position(&mut self) {
        if let Some(n) = self.characters.checked_add(1) {
            self.characters = n;
        } else {
            self.failed = true;
        }
    }
    /// `alt` is an `<img>`'s alt attribute; layout reads the same one.
    pub fn start_element(&mut self, raw: &[u8], display_none: bool, image_has_source: bool,
        alt: Option<&[u8]>) {
        if self.failed || self.finished {
            return;
        }
        let name = local(raw);
        if suppressed(name) {
            self.suppressed = self.suppressed.wrapping_add(1);
            return;
        }
        if name == b"body" {
            self.in_body = true;
            return;
        }
        if !self.in_body || self.suppressed != 0 {
            return;
        }
        if self.stack_top + 1 < self.hidden.len() {
            let hidden = self.hidden[self.stack_top] || display_none;
            self.stack_top += 1;
            self.hidden[self.stack_top] = hidden;
        } else {
            self.overflow_depth = self.overflow_depth.wrapping_add(1);
        }
        if block(name) || name == b"hr" {
            self.boundary();
        } else if name == b"br" {
            self.append(b'\n');
        } else if matches!(name, b"img" | b"image")
            && image_has_source
            && !self.hidden[self.stack_top]
        {
            self.boundary();
            if PictureNote::of(alt) != PictureNote::Decorative {
                self.count_position();
            }
        }
    }
    pub fn end_element(&mut self, raw: &[u8]) {
        if self.failed || self.finished {
            return;
        }
        let name = local(raw);
        if suppressed(name) {
            if self.suppressed != 0 {
                self.suppressed -= 1;
            }
            return;
        }
        if name == b"body" {
            self.boundary();
            self.in_body = false;
            return;
        }
        if !self.in_body || self.suppressed != 0 {
            return;
        }
        if self.overflow_depth != 0 {
            self.overflow_depth -= 1;
        } else if self.stack_top != 0 {
            self.stack_top -= 1;
        }
        if block(name) {
            self.boundary();
        }
    }
    pub fn text(&mut self, bytes: &[u8]) {
        if self.failed
            || self.finished
            || !self.in_body
            || self.suppressed != 0
            || self.hidden[self.stack_top]
        {
            return;
        }
        for &byte in bytes {
            if self.failed {
                break;
            }
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.pending_space = self.paragraph_bytes > 0;
            } else {
                if self.pending_space {
                    self.append(b' ');
                    self.pending_space = false;
                }
                self.append(byte);
            }
        }
    }
    pub fn finish(&mut self) -> Option<u32> {
        if self.failed {
            return None;
        }
        self.finished = true;
        self.boundary();
        Some(self.characters)
    }
}

pub struct Index {
    digest: Digest,
    counts: [u32; MAX_SPINE_ITEMS],
    known: [u8; (MAX_SPINE_ITEMS + 7) / 8],
    spines: u32,
    recorded: u32,
    policy: u32,
    valid: bool,
}
impl Default for Index {
    fn default() -> Self {
        Self {
            digest: [0; 32],
            counts: [0; MAX_SPINE_ITEMS],
            known: [0; (MAX_SPINE_ITEMS + 7) / 8],
            spines: 0,
            recorded: 0,
            policy: 0,
            valid: false,
        }
    }
}
impl Index {
    pub fn invalidate(&mut self) {
        // Clear the caller's table in place; avoid a full index temporary.
        self.valid = false;
        self.spines = 0;
        self.recorded = 0;
        self.policy = 0;
        self.digest.fill(0);
        self.known.fill(0);
        self.counts.fill(0);
    }
    pub fn bind(&mut self, digest: &Digest, policy: u32, spines: u32) -> bool {
        self.invalidate();
        if policy != POLICY_VERSION || spines == 0 || spines > MAX_SPINE_ITEMS as u32 {
            return false;
        }
        self.digest = *digest;
        self.policy = policy;
        self.spines = spines;
        self.valid = true;
        true
    }
    fn matches(&self, digest: &Digest, policy: u32) -> bool {
        self.valid && self.digest == *digest && self.policy == policy
    }
    fn known(&self, spine: u32) -> bool {
        self.known[spine as usize / 8] & (1 << (spine % 8)) != 0
    }
    pub fn record(&mut self, digest: &Digest, policy: u32, spine: u32, characters: u32) -> bool {
        if !self.matches(digest, policy)
            || spine >= self.spines
            || (self.known(spine) && self.counts[spine as usize] != characters)
        {
            self.invalidate();
            return false;
        }
        if !self.known(spine) {
            self.known[spine as usize / 8] |= 1 << (spine % 8);
            self.recorded += 1;
        }
        self.counts[spine as usize] = characters;
        true
    }
    pub fn complete(&self, digest: &Digest, policy: u32) -> bool {
        self.matches(digest, policy) && self.recorded == self.spines
    }
    pub fn spine_count(&self, digest: &Digest, policy: u32) -> u32 {
        if self.matches(digest, policy) {
            self.spines
        } else {
            0
        }
    }
    pub fn chapter_count(&self, digest: &Digest, policy: u32, spine: u32) -> Option<u32> {
        if !self.matches(digest, policy) || spine >= self.spines || !self.known(spine) {
            return None;
        }
        Some(self.counts[spine as usize])
    }
    pub fn project(
        &self,
        digest: &Digest,
        policy: u32,
        spine: u32,
        character: u32,
    ) -> Option<(u64, u64)> {
        if !self.complete(digest, policy)
            || spine >= self.spines
            || character > self.counts[spine as usize]
        {
            return None;
        }
        let mut sum = 0u64;
        let mut prefix = 0u64;
        for i in 0..self.spines as usize {
            if i < spine as usize {
                prefix += u64::from(self.counts[i]);
            }
            sum += u64::from(self.counts[i]);
        }
        if sum == 0 {
            return None;
        }
        Some((prefix + u64::from(character), sum))
    }
}

#[cfg(test)]
#[path = "semantic_index_tests.rs"]
mod tests;
