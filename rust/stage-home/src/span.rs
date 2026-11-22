use crate::cstr;
use newt_ui_canvas::atlas::{self, Face};
#[derive(Clone, Copy, Default)]
pub(crate) struct Span<'a> {
    pub text: &'a [u8],
    pub joined: bool,
    pub suffix: &'a [u8],
}
impl<'a> Span<'a> {
    pub fn plain(text: &'a [u8]) -> Self {
        Self {
            text: cstr(text),
            joined: false,
            suffix: b"",
        }
    }
    pub fn joined(text: &'a [u8], suffix: &'a [u8]) -> Self {
        Self {
            text,
            joined: true,
            suffix,
        }
    }
    pub fn spell(self, mut use_cp: impl FnMut(u32)) {
        let mut run = |t: &[u8]| {
            let mut at = 0;
            while at < t.len() {
                use_cp(atlas::next(t, &mut at));
            }
        };
        if !self.joined {
            run(self.text);
            return;
        }
        let mut at = 0;
        let mut first = true;
        // Keep the mutable closure borrow short between words.
        while let Some((start, end)) = word(self.text, &mut at) {
            if !first {
                run(b" ");
            }
            first = false;
            run(&self.text[start..end]);
        }
        run(self.suffix);
    }
    pub fn bytes(self) -> usize {
        if !self.joined {
            return self.text.len();
        }
        let mut at = 0;
        let mut n = self.suffix.len();
        let mut words = 0;
        while let Some((a, b)) = word(self.text, &mut at) {
            n += b - a + usize::from(words != 0);
            words += 1;
        }
        n
    }
    pub fn width(self, face: Face) -> i32 {
        let mut prev = 0;
        let mut width = 0;
        self.spell(|cp| {
            width += i32::from(atlas::glyph(face, cp, 0).advance) + atlas::kern(face, prev, cp);
            prev = cp;
        });
        width
    }
    pub fn fits(self, face: Face, width: i32) -> bool {
        self.bytes() < 128 && self.width(face) <= width * 64
    }
    pub fn copy(self) -> [u8; 128] {
        let mut out = [0; 128];
        let mut used = 0;
        fn put(out: &mut [u8; 128], used: &mut usize, t: &[u8], room: usize) -> bool {
            let mut n = t.len().min(room - room.min(*used));
            if n < t.len() {
                let mut cut = 0;
                let mut at = 0;
                while atlas::next(t, &mut at) != 0 && at <= n {
                    cut = at;
                }
                n = cut;
            }
            out[*used..*used + n].copy_from_slice(&t[..n]);
            *used += n;
            n == t.len()
        }
        if !self.joined {
            put(&mut out, &mut used, self.text, 127);
        } else {
            let tail = self.suffix.len().min(127);
            let mut at = 0;
            while let Some((a, b)) = word(self.text, &mut at) {
                if (used != 0 && !put(&mut out, &mut used, b" ", 127 - tail))
                    || !put(&mut out, &mut used, &self.text[a..b], 127 - tail)
                {
                    break;
                }
            }
            put(&mut out, &mut used, &self.suffix[..tail], 127);
        }
        out
    }
}
pub(crate) fn word(text: &[u8], at: &mut usize) -> Option<(usize, usize)> {
    let space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b);
    while *at < text.len() && space(text[*at]) {
        *at += 1;
    }
    let start = *at;
    while *at < text.len() && !space(text[*at]) {
        *at += 1;
    }
    (start != *at).then_some((start, *at))
}
pub(crate) fn fit(text: &mut [u8; 128], face: Face, max: i32) {
    if Span::plain(text).width(face) <= max * 64 {
        return;
    }
    let mut used = cstr(text).len();
    while used != 0 {
        used -= 1;
        while used != 0 && text[used] & 0xc0 == 0x80 {
            used -= 1;
        }
        text[used] = 0;
        if Span::plain(text).width(face) + Span::plain("…".as_bytes()).width(face) <= max * 64
            && used + 4 <= 128
        {
            text[used..used + 4].copy_from_slice("…\0".as_bytes());
            return;
        }
    }
    text[..4].copy_from_slice("…\0".as_bytes());
}
pub(crate) fn wrap<'a, const N: usize>(
    text: &'a [u8],
    face: Face,
    width: i32,
) -> ([Span<'a>; N], usize) {
    assert!(N > 0);
    let text = cstr(text);
    let mut lines = [Span::default(); N];
    let mut at = 0;
    let mut start = 0;
    let mut end = 0;
    let mut used = 0;
    while let Some((a, b)) = word(text, &mut at) {
        if start == end {
            start = a;
            end = b;
        } else if Span::joined(&text[start..b], b"").fits(face, width) {
            end = b;
        } else if used == N - 1 {
            let mut cursor = start;
            let mut last = start;
            while let Some((_, e)) = word(text, &mut cursor) {
                if !Span::joined(&text[start..e], "…".as_bytes()).fits(face, width) {
                    break;
                }
                last = e;
            }
            if last == start {
                let mut c = start;
                if let Some((_, e)) = word(text, &mut c) {
                    last = e;
                }
            }
            lines[used] = Span::joined(&text[start..last], "…".as_bytes());
            return (lines, N);
        } else {
            lines[used] = Span::joined(&text[start..end], b"");
            used += 1;
            start = a;
            end = b;
        }
    }
    lines[used] = Span::joined(&text[start..end], b"");
    (lines, used + 1)
}
