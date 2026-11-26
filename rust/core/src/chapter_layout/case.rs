// SPDX-License-Identifier: MIT
//! Compact `no_std` 1:1 case mapping and O(N) forward word-start tagging for
//! EPUB text transforms and synthesized small capitals (no `core::unicode`).
#![forbid(unsafe_code)]

use super::text::{decode, is_combining};

/// Synthesized small-cap font size (~80% of `base_px`, rounded to nearest px).
/// At Newt's default 30 px reading size this hits the native 24 px atlas size.
pub const fn small_cap_px(base_px: u16) -> u16 {
    let scaled = (base_px as u32 * 80 + 50) / 100;
    if scaled < 8 { 8 } else { scaled as u16 }
}

// Symmetric 1:1 bicameral pairs covering 100% of ReaderAtlas's repertoire,
// stored in statics so LLVM loops over .rodata instead of unrolling ranges.
// Each (lo, hi) in ALT starts on an uppercase codepoint and alternates U/L.
static ALT: [(u16, u16); 21] = [
    (0x0100, 0x012f),
    (0x0132, 0x0137),
    (0x0139, 0x0148),
    (0x014a, 0x0177),
    (0x0179, 0x017e),
    (0x0182, 0x0185),
    (0x01a0, 0x01a5),
    (0x01b3, 0x01b6),
    (0x01cd, 0x01dc),
    (0x01de, 0x01ef),
    (0x01f8, 0x021f),
    (0x0222, 0x0233),
    (0x0246, 0x024f),
    (0x0370, 0x0373),
    (0x03d8, 0x03e1),
    (0x0460, 0x0481),
    (0x048a, 0x04bf),
    (0x04c1, 0x04ce),
    (0x04d0, 0x04ff),
    (0x1e00, 0x1e95),
    (0x1ea0, 0x1eff),
];

// Contiguous ranges whose lowercase is uppercase + 32.
static SHIFT32: [(u16, u16); 6] = [
    (0x0041, 0x005a),
    (0x00c0, 0x00d6),
    (0x00d8, 0x00de),
    (0x0391, 0x03a1),
    (0x03a3, 0x03ab),
    (0x0410, 0x042f),
];

// Remaining symmetric (upper, lower) singletons in ReaderAtlas.
static SINGLES: [(u16, u16); 41] = [
    (0x0178, 0x00ff),
    (0x0187, 0x0188),
    (0x018b, 0x018c),
    (0x018e, 0x01dd),
    (0x0191, 0x0192),
    (0x0198, 0x0199),
    (0x01a7, 0x01a8),
    (0x01ac, 0x01ad),
    (0x01af, 0x01b0),
    (0x01b8, 0x01b9),
    (0x01bc, 0x01bd),
    (0x01c4, 0x01c6),
    (0x01c7, 0x01c9),
    (0x01ca, 0x01cc),
    (0x01f1, 0x01f3),
    (0x01f4, 0x01f5),
    (0x01f6, 0x0195),
    (0x01f7, 0x01bf),
    (0x0220, 0x019e),
    (0x023b, 0x023c),
    (0x023d, 0x019a),
    (0x0241, 0x0242),
    (0x0243, 0x0180),
    (0x0376, 0x0377),
    (0x037f, 0x03f3),
    (0x0386, 0x03ac),
    (0x0388, 0x03ad),
    (0x0389, 0x03ae),
    (0x038a, 0x03af),
    (0x038c, 0x03cc),
    (0x038e, 0x03cd),
    (0x038f, 0x03ce),
    (0x03cf, 0x03d7),
    (0x03f7, 0x03f8),
    (0x03f9, 0x03f2),
    (0x03fa, 0x03fb),
    (0x03fd, 0x037b),
    (0x03fe, 0x037c),
    (0x03ff, 0x037d),
    (0x04c0, 0x04cf),
    (0x2132, 0x214e),
];

// Asymmetric 1:1 mappings (titlecase digraphs, Greek variants, Kelvin/Angstrom, etc.).
static ASYM_UPPER: [(u16, u16); 16] = [
    (0x00b5, 0x039c),
    (0x0131, 0x0049),
    (0x017f, 0x0053),
    (0x01c5, 0x01c4),
    (0x01c8, 0x01c7),
    (0x01cb, 0x01ca),
    (0x01f2, 0x01f1),
    (0x03c2, 0x03a3),
    (0x03d0, 0x0392),
    (0x03d1, 0x0398),
    (0x03d5, 0x03a6),
    (0x03d6, 0x03a0),
    (0x03f0, 0x039a),
    (0x03f1, 0x03a1),
    (0x03f5, 0x0395),
    (0x1e9b, 0x1e60),
];

static ASYM_LOWER: [(u16, u16); 10] = [
    (0x0130, 0x0069),
    (0x01c5, 0x01c6),
    (0x01c8, 0x01c9),
    (0x01cb, 0x01cc),
    (0x01f2, 0x01f3),
    (0x03f4, 0x03b8),
    (0x1e9e, 0x00df),
    (0x2126, 0x03c9),
    (0x212a, 0x006b),
    (0x212b, 0x00e5),
];

#[inline(never)]
fn in_ranges(table: &[(u16, u16)], c: u16) -> u16 {
    for &(lo, hi) in table {
        if c >= lo && c <= hi {
            return lo;
        }
    }
    0
}

#[inline(never)]
fn lookup_pair(table: &[(u16, u16)], c: u16, flip: bool) -> u16 {
    for &(a, b) in table {
        let (k, v) = if flip { (b, a) } else { (a, b) };
        if c == k {
            return v;
        }
    }
    0
}

#[inline(never)]
fn map_case(cp: u32, upper: bool) -> u32 {
    if cp > 0x214e {
        return cp;
    }
    let c = cp as u16;
    if in_ranges(&SHIFT32, if upper { c.wrapping_sub(32) } else { c }) != 0 {
        return if upper { cp - 32 } else { cp + 32 };
    }
    let cyr = if upper { c.wrapping_sub(80) } else { c };
    if (0x0400..=0x040f).contains(&cyr) {
        return if upper { cp - 80 } else { cp + 80 };
    }
    let lo = in_ranges(&ALT, c);
    if lo != 0 {
        let is_upper = (c & 1) == (lo & 1);
        return if upper != is_upper {
            u32::from(if upper { c - 1 } else { c + 1 })
        } else {
            cp
        };
    }
    let single = lookup_pair(&SINGLES, c, upper);
    if single != 0 {
        return u32::from(single);
    }
    let asym: &[(u16, u16)] = if upper { &ASYM_UPPER } else { &ASYM_LOWER };
    let mapped = lookup_pair(asym, c, false);
    if mapped != 0 { u32::from(mapped) } else { cp }
}

/// 1:1 Unicode simple uppercase mapping covering 100% of ReaderAtlas pairs.
/// Never increases UTF-8 byte length.
pub fn to_upper(cp: u32) -> u32 {
    map_case(cp, true)
}

/// 1:1 Unicode simple lowercase mapping covering 100% of ReaderAtlas pairs.
/// Never increases UTF-8 byte length.
pub fn to_lower(cp: u32) -> u32 {
    map_case(cp, false)
}

pub const WS_SEP: u8 = 0;
const WS_WORD: u8 = 1;
const WS_APOS: u8 = 2;
/// Stored in bit 7 of `storage.levels[pos]`, disjoint from bidi (0..2),
/// Arabic form (3..5), and focus bold (6).
pub const WORD_START_BIT: u8 = 0x80;

static WORD_RANGES: [(u16, u16); 17] = [
    (0x0030, 0x0039),
    (0x0041, 0x005a),
    (0x0061, 0x007a),
    (0x00b5, 0x00b5),
    (0x00c0, 0x00d6),
    (0x00d8, 0x00f6),
    (0x00f8, 0x024f),
    (0x0370, 0x0373),
    (0x0376, 0x037d),
    (0x037f, 0x0383),
    (0x0386, 0x0386),
    (0x0388, 0x04ff),
    (0x1e00, 0x1eff),
    (0x2126, 0x2126),
    (0x212a, 0x212b),
    (0x2132, 0x2132),
    (0x214e, 0x214e),
];

pub fn is_word_char(cp: u32) -> bool {
    cp <= 0x214e && in_ranges(&WORD_RANGES, cp as u16) != 0
}

/// Single O(N) forward pass tagging `WORD_START_BIT` in `levels` for
/// `text-transform: capitalize`. Returns the ending word state so a mid-word
/// paragraph capacity flush continues without capitalizing interior letters,
/// and arbitrary runs of soft hyphens or combining diacritics keep state intact.
#[inline(never)]
pub fn mark_word_starts(text: &[u8], levels: &mut [u8], mut state: u8) -> u8 {
    let mut i = 0;
    while i < text.len() {
        let pos = i;
        let cp = decode(text, &mut i);
        if cp == 0x00ad || is_combining(cp) {
            continue;
        }
        if is_word_char(cp) {
            if state == WS_SEP {
                levels[pos] |= WORD_START_BIT;
            }
            state = WS_WORD;
        } else if matches!(cp, 0x0027 | 0x2019) {
            state = if state == WS_WORD { WS_APOS } else { WS_SEP };
        } else {
            state = WS_SEP;
        }
    }
    state
}
