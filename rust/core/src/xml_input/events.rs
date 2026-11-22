// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Bounded XML event operations; the owners and public API stay in xml_input.
use super::{Attribute, EncryptionScan, MAX_TEXT_CAPTURE, TextCapture, c_bytes, local_is, space};

impl Default for TextCapture {
    fn default() -> Self {
        Self {
            buf: [0; 256],
            len: 0,
            active: false,
        }
    }
}
impl TextCapture {
    pub fn begin(&mut self) {
        self.active = true;
        self.len = 0;
        self.buf[0] = 0;
    }
    pub fn end(&mut self) {
        self.active = false;
    }
    /// Explicit callback length. Positive lengths must fit the supplied slice.
    pub fn add_event(&mut self, text: &[u8], text_len: i32) {
        if !self.active || text_len <= 0 {
            return;
        }
        self.add(&text[..text_len as usize]);
    }
    /// Slice-length convenience for a nonnegative callback event.
    pub fn add(&mut self, text: &[u8]) {
        if !self.active {
            return;
        }
        let n = text.len().min(MAX_TEXT_CAPTURE - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&text[..n]);
        self.len += n;
        self.buf[self.len] = 0;
    }
    /// Does not change len or move the prefix, including when called repeatedly.
    pub fn trimmed(&mut self) -> &[u8] {
        let mut start = 0;
        while start < self.len && space(self.buf[start]) {
            start += 1;
        }
        let mut stop = self.len;
        while stop > start && space(self.buf[stop - 1]) {
            stop -= 1;
        }
        self.buf[stop] = 0;
        c_bytes(&self.buf[start..=stop])
    }
}
impl EncryptionScan {
    pub fn on_start_element(&mut self, name: &[u8], atts: &[Attribute<'_>]) {
        if !local_is(name, b"EncryptionMethod") {
            return;
        }
        for (name, alg) in atts {
            if !local_is(name, b"Algorithm") {
                continue;
            }
            let alg = c_bytes(alg);
            let font = alg == b"http://www.idpf.org/2008/embedding"
                || alg == b"http://ns.adobe.com/pdf/enc#RC";
            if !font {
                self.content_encrypted = true;
            }
        }
    }
}
