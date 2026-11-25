// SPDX-License-Identifier: MIT
//! Read-only, explicit snapshot query; normal DIAG and its consumers stay intact.
use super::{Bridge, Text};
impl Bridge {
    pub(super) fn wake_reply(&mut self, id: u32, snapshot: Option<[u32; newt_core::wake_timing::WORDS]>) {
        let Some(words) = snapshot else {
            self.reply(id, b"WAKE version=1 available=0");
            return;
        };
        const PREFIX: &[u8] = b"WAKE version=1 available=1 data=";
        // Text reserves a NUL even though the wire protocol sends no NUL.
        let mut bytes = [0; PREFIX.len() + newt_core::wake_timing::WORDS * 8 + 1];
        let mut body = Text::new(&mut bytes);
        body.bytes(PREFIX);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for word in words {
            for byte in word.to_le_bytes() {
                body.bytes(&[HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]]);
            }
        }
        self.reply(id, body.written());
    }
}
