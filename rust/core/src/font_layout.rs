//! Minimal borrowed GPOS/GSUB readers derived from FreeInk (MIT).
//! See rust/THIRD_PARTY.md and rust/tests/font_layout/CONTRACT.md. Not a full shaper.
#![forbid(unsafe_code)]

#[derive(Clone, Copy)]
struct View<'a>(&'a [u8]);
impl<'a> View<'a> {
    fn has(self, at: usize, count: usize) -> bool {
        at <= self.0.len() && count <= self.0.len() - at
    }
    fn u16(self, at: usize) -> u32 {
        if self.has(at, 2) {
            u32::from(self.0[at]) * 256 + u32::from(self.0[at + 1])
        } else {
            0
        }
    }
    fn u32(self, at: usize) -> u32 {
        if self.has(at, 4) {
            (self.u16(at) << 16) | self.u16(at + 2)
        } else {
            0
        }
    }
    fn sub(self, at: u32) -> Self {
        let at = at as usize;
        if at != 0 && self.has(at, 1) {
            Self(&self.0[at..])
        } else {
            Self(&[])
        }
    }
}

fn coverage(view: View<'_>, glyph: u32) -> Option<usize> {
    let count = view.u16(2).min(8192) as usize;
    match view.u16(0) {
        1 if view.has(4, count * 2) => {
            let (mut low, mut high) = (0, count);
            while low < high {
                let middle = (low + high) / 2;
                if view.u16(4 + middle * 2) < glyph {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            (low < count && view.u16(4 + low * 2) == glyph).then_some(low)
        }
        2 if view.has(4, count * 6) => {
            for i in 0..count {
                let at = 4 + i * 6;
                let start = view.u16(at);
                if glyph >= start && glyph <= view.u16(at + 2) {
                    return Some((view.u16(at + 4) + glyph - start) as usize);
                }
            }
            None
        }
        _ => None,
    }
}
fn glyph_class(view: View<'_>, glyph: u32) -> usize {
    match view.u16(0) {
        1 => {
            let start = view.u16(2);
            let count = view.u16(4).min(8192);
            if !view.has(6, count as usize * 2) {
                return 0;
            }
            if glyph >= start && glyph - start < count {
                view.u16(6 + (glyph - start) as usize * 2) as usize
            } else {
                0
            }
        }
        2 => {
            let count = view.u16(2).min(8192) as usize;
            if !view.has(4, count * 6) {
                return 0;
            }
            for i in 0..count {
                let at = 4 + i * 6;
                if glyph >= view.u16(at) && glyph <= view.u16(at + 2) {
                    return view.u16(at + 4) as usize;
                }
            }
            0
        }
        _ => 0,
    }
}
fn pair(sub: View<'_>, left: u32, right: u32) -> Option<i32> {
    let vf1 = sub.u16(4);
    let vf2 = sub.u16(6);
    if vf1 & 4 == 0 {
        return None;
    }
    let len1 = vf1.count_ones() as usize * 2;
    let len2 = vf2.count_ones() as usize * 2;
    let advance = (vf1 & 3).count_ones() as usize * 2;
    let index = coverage(sub.sub(sub.u16(2)), left)?;
    let at = match sub.u16(0) {
        1 => {
            let count = sub.u16(8) as usize;
            if index >= count || !sub.has(10, count * 2) {
                return None;
            }
            let set = sub.sub(sub.u16(10 + index * 2));
            let count = set.u16(0) as usize;
            let width = 2 + len1 + len2;
            if !set.has(2, count * width) {
                return None;
            }
            let (mut low, mut high) = (0, count);
            while low < high {
                let mid = (low + high) / 2;
                if set.u16(2 + mid * width) < right {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            if low >= count || set.u16(2 + low * width) != right {
                return None;
            }
            return Some(i32::from(set.u16(4 + low * width + advance) as u16 as i16));
        }
        2 => {
            let n1 = sub.u16(12) as usize;
            let n2 = sub.u16(14) as usize;
            let c1 = glyph_class(sub.sub(sub.u16(8)), left);
            let c2 = glyph_class(sub.sub(sub.u16(10)), right);
            if c1 >= n1 || c2 >= n2 {
                return None;
            }
            let width = len1 + len2;
            // Reject RV32 size_t overflow rather than accepting a wrapped matrix.
            let bytes = (n1 as u32)
                .checked_mul(n2 as u32)?
                .checked_mul(width as u32)? as usize;
            if !sub.has(16, bytes) {
                return None;
            }
            16usize
                .checked_add((c1 * n2 + c2).checked_mul(width)?)?
                .checked_add(advance)?
        }
        _ => return None,
    };
    Some(i32::from(sub.u16(at) as u16 as i16))
}
fn step(work: &mut u32) -> bool {
    *work += 1;
    *work <= 4096
}
fn usable(lang: View<'_>) -> bool {
    lang.has(0, 6) && lang.has(6, lang.u16(4) as usize * 2)
}
fn default_lang<'a>(scripts: View<'a>, tag: u32, work: &mut u32) -> View<'a> {
    let (mut low, mut high) = (0, scripts.u16(0) as usize);
    while low < high {
        if !step(work) {
            return View(&[]);
        }
        let mid = low + (high - low) / 2;
        if scripts.u32(2 + mid * 6) < tag {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    if low >= scripts.u16(0) as usize || scripts.u32(2 + low * 6) != tag {
        return View(&[]);
    }
    let script = scripts.sub(scripts.u16(6 + low * 6));
    if script.has(0, 4) {
        script.sub(script.u16(0))
    } else {
        View(&[])
    }
}

// The shared traversal preserves both readers' original ordered feature walk.
struct Layout<'a> {
    features: View<'a>,
    lookups: View<'a>,
    lang: View<'a>,
    work: u32,
}
impl<'a> Layout<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        let table = View(bytes);
        if !table.has(0, 10) {
            return None;
        }
        let scripts = table.sub(table.u16(4));
        let features = table.sub(table.u16(6));
        let lookups = table.sub(table.u16(8));
        if !scripts.has(0, 2)
            || !scripts.has(2, scripts.u16(0) as usize * 6)
            || !features.has(2, features.u16(0) as usize * 6)
            || !lookups.has(2, lookups.u16(0) as usize * 2)
        {
            return None;
        }
        let mut work = 0;
        let latin = default_lang(scripts, u32::from_be_bytes(*b"latn"), &mut work);
        let fallback = default_lang(scripts, u32::from_be_bytes(*b"DFLT"), &mut work);
        let lang = if usable(latin) { latin } else { fallback };
        usable(lang).then_some(Self {
            features,
            lookups,
            lang,
            work,
        })
    }
    fn feature(&mut self, index: u32, seq: &[u32], kern: bool) -> Option<i32> {
        if index >= self.features.u16(0) || !step(&mut self.work) {
            return None;
        }
        let record = 2 + index as usize * 6;
        let tag = self.features.u32(record);
        if if kern {
            tag != u32::from_be_bytes(*b"kern")
        } else {
            tag != u32::from_be_bytes(*b"liga") && tag != u32::from_be_bytes(*b"rlig")
        } {
            return None;
        }
        let feature = self.features.sub(self.features.u16(record + 4));
        let count = feature.u16(2) as usize;
        if !feature.has(4, count * 2) {
            return None;
        }
        for i in 0..count {
            if !step(&mut self.work) {
                return None;
            }
            let index = feature.u16(4 + i * 2) as usize;
            if index >= self.lookups.u16(0) as usize {
                continue;
            }
            let lookup = self.lookups.sub(self.lookups.u16(2 + index * 2));
            let kind = lookup.u16(0);
            let direct = if kern { 2 } else { 4 };
            let extension = if kern { 9 } else { 7 };
            let count = lookup.u16(4) as usize;
            if (kind != direct && kind != extension) || !lookup.has(6, count * 2) {
                continue;
            }
            for j in 0..count {
                if !step(&mut self.work) {
                    return None;
                }
                let mut sub = lookup.sub(lookup.u16(6 + j * 2));
                if kind == extension {
                    if !sub.has(0, 8) || sub.u16(0) != 1 || sub.u16(2) != direct {
                        continue;
                    }
                    sub = sub.sub(sub.u32(4));
                }
                if kern {
                    if let Some(value) = pair(sub, seq[0], seq[1]) {
                        return Some(value);
                    }
                    continue;
                }
                if sub.u16(0) != 1 {
                    continue;
                }
                let Some(index) = coverage(sub.sub(sub.u16(2)), seq[0]) else {
                    continue;
                };
                let count = sub.u16(4) as usize;
                if index >= count || !sub.has(6, count * 2) {
                    continue;
                }
                let set = sub.sub(sub.u16(6 + index * 2));
                let count = set.u16(0) as usize;
                if !set.has(2, count * 2) {
                    continue;
                }
                for k in 0..count {
                    if !step(&mut self.work) {
                        return None;
                    }
                    let ligature = set.sub(set.u16(2 + k * 2));
                    if ligature.u16(2) as usize != seq.len()
                        || !ligature.has(4, (seq.len() - 1) * 2)
                    {
                        continue;
                    }
                    let mut matched = true;
                    for (component, &glyph) in seq.iter().enumerate().skip(1) {
                        matched = matched && ligature.u16(4 + (component - 1) * 2) == glyph;
                    }
                    if matched {
                        return Some(ligature.u16(0) as i32);
                    }
                }
            }
        }
        None
    }
    fn query(mut self, seq: &[u32], kern: bool) -> i32 {
        let required = self.lang.u16(2);
        if required != 0xffff {
            if let Some(value) = self.feature(required, seq, kern) {
                if kern || value != 0 {
                    return value;
                }
            }
        }
        for i in 0..self.lang.u16(4) as usize {
            if let Some(value) = self.feature(self.lang.u16(6 + i * 2), seq, kern) {
                if kern || value != 0 {
                    return value;
                }
            }
        }
        0
    }
}
/// First matching left XAdvance in FUnits; zero is also a terminal match.
pub fn pair_kern_adjustment(table: &[u8], left: u32, right: u32) -> i32 {
    if left == 0 || right == 0 {
        return 0;
    }
    Layout::new(table).map_or(0, |layout| layout.query(&[left, right], true))
}
/// Glyph ID for exactly two or three glyphs, or zero. No Unicode conversion.
pub fn ligature_glyph_id(table: &[u8], sequence: &[u32]) -> u32 {
    if !(2..=3).contains(&sequence.len()) {
        return 0;
    }
    Layout::new(table).map_or(0, |layout| layout.query(sequence, false) as u32)
}
