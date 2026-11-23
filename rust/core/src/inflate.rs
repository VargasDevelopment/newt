#![forbid(unsafe_code)]
//! Safe translation of the pinned FreeInkBook tinfl coroutine; see rust/tests/inflate/CONTRACT.md.

pub const PARSE_ZLIB_HEADER: u32 = 1;
pub const HAS_MORE_INPUT: u32 = 2;
pub const NON_WRAPPING: u32 = 4;
pub const COMPUTE_ADLER32: u32 = 8;
pub const DICTIONARY_SIZE: usize = 32768;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    CannotMakeProgress = -4,
    BadParam = -3,
    AdlerMismatch = -2,
    Failed = -1,
    Done = 0,
    NeedsMoreInput = 1,
    HasMoreOutput = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub status: Status,
    pub consumed: usize,
    pub produced: usize,
    pub adler32: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BitBuffer {
    Bits32,
    Bits64,
}
#[derive(Clone, Copy)]
#[repr(u8)]
enum Phase {
    Start,
    Header0,
    Header1,
    Block,
    StoredHeader,
    StoredBuffered,
    StoredPending,
    StoredCopy,
    StoredAwaitInput,
    Sizes,
    CodeLengths,
    Build,
    LengthCodes,
    Repeat,
    Symbols,
    Literal,
    LengthExtra,
    Distance,
    DistanceExtra,
    Copy,
    EndBlock,
    Trailer,
    Done,
    Failed,
}
struct Table<const N: usize, const C: usize> {
    lookup: [i16; 1024],
    tree: [i16; N],
    codes: [u8; C],
}
impl<const N: usize, const C: usize> Table<N, C> {
    const fn new() -> Self {
        Self {
            lookup: [0; 1024],
            tree: [0; N],
            codes: [0; C],
        }
    }
    fn build(&mut self, count: usize) -> bool {
        self.lookup.fill(0);
        self.tree.fill(0);
        let mut totals = [0u32; 16];
        let mut next = [0u32; 17];
        for &c in &self.codes[..count] {
            totals[c as usize] += 1;
        }
        let (mut used, mut total) = (0, 0);
        for i in 1..=15 {
            used += totals[i];
            total = (total + totals[i]) << 1;
            next[i + 1] = total;
        }
        if total != 65536 && used > 1 {
            return false;
        }
        let mut tree_next = -1i32;
        for sym in 0..count {
            let size = self.codes[sym] as usize;
            if size == 0 {
                continue;
            }
            let mut cur = next[size];
            next[size] += 1;
            let mut rev = 0usize;
            for _ in 0..size {
                rev = (rev << 1) | (cur as usize & 1);
                cur >>= 1;
            }
            if size <= 10 {
                let entry = ((size << 9) | sym) as i16;
                while rev < 1024 {
                    self.lookup[rev] = entry;
                    rev += 1 << size;
                }
                continue;
            }
            let mut node = self.lookup[rev & 1023] as i32;
            if node == 0 {
                self.lookup[rev & 1023] = tree_next as i16;
                node = tree_next;
                tree_next -= 2;
            }
            rev >>= 9;
            for _ in (11..size).rev() {
                rev >>= 1;
                node -= (rev & 1) as i32;
                let ix = (-node - 1) as usize;
                if ix >= N {
                    return false;
                }
                if self.tree[ix] == 0 {
                    self.tree[ix] = tree_next as i16;
                    node = tree_next;
                    tree_next -= 2;
                } else {
                    node = self.tree[ix] as i32;
                }
            }
            rev >>= 1;
            node -= (rev & 1) as i32;
            let ix = (-node - 1) as usize;
            if ix >= N {
                return false;
            }
            self.tree[ix] = sym as i16;
        }
        true
    }
    fn peek(&self, bits: u64, available: u32, partial: bool) -> Option<(u32, u32)> {
        let mut temp = self.lookup[bits as usize & 1023] as i32;
        let mut len;
        if temp >= 0 {
            len = temp as u32 >> 9;
            temp &= 511;
            if partial && (len == 0 || available < len) {
                return None;
            }
        } else {
            if partial && available <= 10 {
                return None;
            }
            len = 10;
            loop {
                let ix = ((!temp) as usize) + ((bits >> len) as usize & 1);
                temp = *self.tree.get(ix)? as i32;
                len += 1;
                if temp >= 0 {
                    break;
                }
                if partial && available < len + 1 {
                    return None;
                }
                if len >= 16 {
                    return None;
                }
            }
        }
        Some((temp as u32, len))
    }
}
#[path = "inflate/tables.rs"]
mod tables;
use tables::{DEZIGZAG, DIST_BASE, DIST_EXTRA, LENGTH_BASE, LENGTH_EXTRA};

/// Initialized, caller-owned state. `reset` changes only the coroutine entry,
/// as does tinfl_init; no dictionary or large temporary is created by reset.
pub struct State {
    phase: Phase,
    mode: BitBuffer,
    bits: u64,
    num_bits: u32,
    huff_filling: bool,
    header0: u32,
    final_bits: u32,
    block_type: u32,
    build_type: usize,
    adler: u32,
    trailer: u32,
    counter: u32,
    dist: u32,
    history_cursor: usize,
    extra: u32,
    index: usize,
    sizes: [usize; 3],
    raw: [u8; 4],
    lens: [u8; 457],
    literal: Table<576, 288>,
    distance: Table<64, 32>,
    code: Table<38, 19>,
}
#[path = "inflate/state.rs"]
mod state;
impl State {
    pub const fn new(mode: BitBuffer) -> Self {
        Self {
            phase: Phase::Start,
            mode,
            bits: 0,
            num_bits: 0,
            huff_filling: false,
            header0: 0,
            final_bits: 0,
            block_type: 0,
            build_type: 0,
            adler: 0,
            trailer: 1,
            counter: 0,
            dist: 0,
            history_cursor: 0,
            extra: 0,
            index: 0,
            sizes: [0; 3],
            raw: [0; 4],
            lens: [0; 457],
            literal: Table::new(),
            distance: Table::new(),
            code: Table::new(),
        }
    }
    pub fn reset(&mut self) {
        self.phase = Phase::Start;
    }
    pub fn adler32(&self) -> u32 {
        self.adler
    }
    /// Whether the stream reached its end, including the zlib checksum.
    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Done)
    }
    fn append(&mut self, byte: u8) {
        self.bits |= (byte as u64) << self.num_bits;
        if self.mode == BitBuffer::Bits32 {
            self.bits &= u32::MAX as u64;
        }
        self.num_bits += 8;
    }
    fn take(&mut self, n: u32) -> u32 {
        let v = (self.bits & ((1u64 << n) - 1)) as u32;
        self.bits >>= n;
        self.num_bits -= n;
        v
    }
    fn get(&mut self, input: &[u8], pos: &mut usize, n: u32) -> Option<u32> {
        while self.num_bits < n {
            let b = *input.get(*pos)?;
            *pos += 1;
            self.append(b);
        }
        Some(self.take(n))
    }
    fn peek(&self, table: usize, partial: bool) -> Option<(u32, u32)> {
        match table {
            0 => self.literal.peek(self.bits, self.num_bits, partial),
            1 => self.distance.peek(self.bits, self.num_bits, partial),
            _ => self.code.peek(self.bits, self.num_bits, partial),
        }
    }
    fn huff(&mut self, table: usize, input: &[u8], pos: &mut usize) -> Option<u32> {
        // A suspended HUFF_BITBUF_FILL resumes inside the macro, bypassing
        // HUFF_DECODE's two-byte prefetch even with a large new input slice.
        if self.huff_filling || self.num_bits < 15 {
            if self.huff_filling || input.len() - *pos < 2 {
                self.huff_filling = true;
                loop {
                    if self.peek(table, true).is_some() {
                        break;
                    }
                    let b = *input.get(*pos)?;
                    *pos += 1;
                    self.append(b);
                    if self.num_bits >= 15 {
                        break;
                    }
                }
                self.huff_filling = false;
            } else {
                for _ in 0..2 {
                    self.append(input[*pos]);
                    *pos += 1;
                }
            }
        }
        let (sym, n) = self.peek(table, false)?;
        if n > self.num_bits {
            return None;
        }
        self.take(n);
        Some(sym)
    }
    fn fast_symbol(&mut self) -> Option<u32> {
        let (sym, n) = self.peek(0, false)?;
        if n == 0 || n > self.num_bits {
            return None;
        }
        self.take(n);
        Some((n << 16) | sym)
    }
    /// `output[..offset]` is preserved history. The writable range is
    /// `offset..offset+capacity`; in ring mode that end is the ring size.
    /// The exclusive output borrow makes input/output aliasing impossible.
    pub fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        offset: usize,
        capacity: usize,
        flags: u32,
    ) -> Progress {
        let end = match offset.checked_add(capacity) {
            Some(n) if n <= output.len() => n,
            _ => {
                return Progress {
                    status: Status::BadParam,
                    consumed: 0,
                    produced: 0,
                    adler32: self.adler,
                }
            }
        };
        let wrapping = flags & NON_WRAPPING == 0;
        if wrapping && end != 0 && !end.is_power_of_two() {
            return Progress {
                status: Status::BadParam,
                consumed: 0,
                produced: 0,
                adler32: self.adler,
            };
        }
        let mask = if wrapping {
            end.wrapping_sub(1)
        } else {
            usize::MAX
        };
        let (mut ip, mut op) = (0, offset);
        let need = if flags & HAS_MORE_INPUT != 0 {
            Status::NeedsMoreInput
        } else {
            Status::CannotMakeProgress
        };
        let mut status = 'decode: loop {
            macro_rules! bits {
                ($n:expr) => {
                    match self.get(input, &mut ip, $n) {
                        Some(v) => v,
                        None => break 'decode need,
                    }
                };
            }
            macro_rules! byte {
                () => {
                    match input.get(ip) {
                        Some(&v) => {
                            ip += 1;
                            v
                        }
                        None => break 'decode need,
                    }
                };
            }
            macro_rules! huff {
                ($t:expr) => {
                    match self.huff($t, input, &mut ip) {
                        Some(v) => v,
                        None => break 'decode need,
                    }
                };
            }
            macro_rules! fail {
                () => {{
                    self.phase = Phase::Failed;
                    break 'decode Status::Failed;
                }};
            }
            match self.phase {
                Phase::Start => {
                    self.bits = 0;
                    self.num_bits = 0;
                    self.counter = 0;
                    self.dist = 0;
                    self.extra = 0;
                    self.huff_filling = false;
                    self.header0 = 0;
                    self.adler = 1;
                    self.trailer = 1;
                    self.phase = if flags & PARSE_ZLIB_HEADER != 0 {
                        Phase::Header0
                    } else {
                        Phase::Block
                    };
                }
                Phase::Header0 => {
                    self.header0 = byte!() as u32;
                    self.phase = Phase::Header1;
                }
                Phase::Header1 => {
                    let h = byte!() as u32;
                    if (self.header0 * 256 + h) % 31 != 0 || h & 32 != 0 || self.header0 & 15 != 8 {
                        fail!();
                    }
                    if wrapping {
                        let window = 1usize << (8 + (self.header0 >> 4));
                        if window > 32768 || end < window {
                            fail!();
                        }
                    }
                    self.phase = Phase::Block;
                }
                Phase::Block => {
                    self.final_bits = bits!(3);
                    self.block_type = self.final_bits >> 1;
                    match self.block_type {
                        0 => {
                            self.take(self.num_bits & 7);
                            self.index = 0;
                            self.phase = Phase::StoredHeader;
                        }
                        1 => {
                            self.sizes[0] = 288;
                            self.sizes[1] = 32;
                            self.distance.codes[..32].fill(5);
                            self.literal.codes[..144].fill(8);
                            self.literal.codes[144..256].fill(9);
                            self.literal.codes[256..280].fill(7);
                            self.literal.codes[280..288].fill(8);
                            self.build_type = 1;
                            self.phase = Phase::Build;
                        }
                        2 => {
                            self.index = 0;
                            self.phase = Phase::Sizes;
                        }
                        _ => {
                            fail!();
                        }
                    }
                }
                Phase::StoredHeader => {
                    while self.index < 4 {
                        self.raw[self.index] = if self.num_bits != 0 {
                            bits!(8) as u8
                        } else {
                            byte!()
                        };
                        self.index += 1;
                    }
                    self.counter = u16::from_le_bytes([self.raw[0], self.raw[1]]) as u32;
                    if self.counter != (!u16::from_le_bytes([self.raw[2], self.raw[3]])) as u32 {
                        fail!();
                    }
                    self.phase = Phase::StoredBuffered;
                }
                Phase::StoredBuffered => {
                    if self.counter == 0 {
                        self.phase = Phase::EndBlock;
                        continue;
                    }
                    if self.num_bits == 0 {
                        self.phase = Phase::StoredCopy;
                        continue;
                    }
                    self.dist = bits!(8);
                    self.phase = Phase::StoredPending;
                }
                Phase::StoredPending => {
                    if op == end {
                        break 'decode Status::HasMoreOutput;
                    }
                    output[op] = self.dist as u8;
                    op += 1;
                    self.counter -= 1;
                    self.phase = Phase::StoredBuffered;
                }
                Phase::StoredCopy => {
                    if self.counter == 0 {
                        self.phase = Phase::EndBlock;
                        continue;
                    }
                    if op == end {
                        break 'decode Status::HasMoreOutput;
                    }
                    self.phase = Phase::StoredAwaitInput;
                }
                Phase::StoredAwaitInput => {
                    if ip == input.len() {
                        break 'decode need;
                    }
                    let n = core::cmp::min(
                        core::cmp::min(end - op, input.len() - ip),
                        self.counter as usize,
                    );
                    output[op..op + n].copy_from_slice(&input[ip..ip + n]);
                    op += n;
                    ip += n;
                    self.counter -= n as u32;
                    self.phase = Phase::StoredCopy;
                }
                Phase::Sizes => {
                    while self.index < 3 {
                        self.sizes[self.index] =
                            bits!([5, 5, 4][self.index]) as usize + [257, 1, 4][self.index];
                        self.index += 1;
                    }
                    self.code.codes.fill(0);
                    self.index = 0;
                    self.phase = Phase::CodeLengths;
                }
                Phase::CodeLengths => {
                    while self.index < self.sizes[2] {
                        self.code.codes[DEZIGZAG[self.index]] = bits!(3) as u8;
                        self.index += 1;
                    }
                    self.sizes[2] = 19;
                    self.build_type = 2;
                    self.phase = Phase::Build;
                }
                Phase::Build => {
                    let ok = match self.build_type {
                        2 => self.code.build(self.sizes[2]),
                        1 => self.distance.build(self.sizes[1]),
                        _ => self.literal.build(self.sizes[0]),
                    };
                    if !ok {
                        fail!();
                    }
                    if self.build_type == 2 {
                        self.index = 0;
                        self.phase = Phase::LengthCodes;
                    } else if self.build_type == 1 {
                        self.build_type = 0;
                    } else {
                        self.phase = Phase::Symbols;
                    }
                }
                Phase::LengthCodes => {
                    if self.index >= self.sizes[0] + self.sizes[1] {
                        if self.index != self.sizes[0] + self.sizes[1] {
                            fail!();
                        }
                        self.literal.codes[..self.sizes[0]]
                            .copy_from_slice(&self.lens[..self.sizes[0]]);
                        self.distance.codes[..self.sizes[1]]
                            .copy_from_slice(&self.lens[self.sizes[0]..self.index]);
                        self.build_type = 1;
                        self.phase = Phase::Build;
                        continue;
                    }
                    self.dist = huff!(2);
                    if self.dist < 16 {
                        self.lens[self.index] = self.dist as u8;
                        self.index += 1;
                    } else {
                        if self.dist > 18 || (self.dist == 16 && self.index == 0) {
                            fail!();
                        }
                        self.extra = [2, 3, 7][(self.dist - 16) as usize];
                        self.phase = Phase::Repeat;
                    }
                }
                Phase::Repeat => {
                    let n = bits!(self.extra) as usize + [3, 3, 11][(self.dist - 16) as usize];
                    let v = if self.dist == 16 {
                        self.lens[self.index - 1]
                    } else {
                        0
                    };
                    if self.index + n > self.lens.len() {
                        fail!();
                    }
                    self.lens[self.index..self.index + n].fill(v);
                    self.index += n;
                    self.phase = Phase::LengthCodes;
                }
                Phase::Symbols => {
                    if self.huff_filling || input.len() - ip < 4 || end - op < 2 {
                        self.counter = huff!(0);
                        if self.counter < 256 {
                            self.phase = Phase::Literal;
                            continue;
                        }
                    } else {
                        let threshold = if self.mode == BitBuffer::Bits64 {
                            30
                        } else {
                            15
                        };
                        if self.num_bits < threshold {
                            let n = if self.mode == BitBuffer::Bits64 { 4 } else { 2 };
                            for _ in 0..n {
                                self.append(input[ip]);
                                ip += 1;
                            }
                        }
                        let first = match self.fast_symbol() {
                            Some(v) => v,
                            None => {
                                fail!();
                            }
                        };
                        self.counter = first & 65535;
                        if self.counter & 256 == 0 {
                            if self.mode == BitBuffer::Bits32 && self.num_bits < 15 {
                                for _ in 0..2 {
                                    self.append(input[ip]);
                                    ip += 1;
                                }
                            }
                            let second = match self.fast_symbol() {
                                Some(v) => v & 65535,
                                None => {
                                    fail!();
                                }
                            };
                            output[op] = self.counter as u8;
                            op += 1;
                            if second & 256 == 0 {
                                output[op] = second as u8;
                                op += 1;
                                continue;
                            }
                            self.counter = second;
                        }
                    }
                    self.counter &= 511;
                    if self.counter == 256 {
                        self.phase = Phase::EndBlock;
                        continue;
                    }
                    if !(257..=287).contains(&self.counter) {
                        fail!();
                    }
                    self.extra = LENGTH_EXTRA[(self.counter - 257) as usize];
                    self.counter = LENGTH_BASE[(self.counter - 257) as usize];
                    self.phase = Phase::LengthExtra;
                }
                Phase::Literal => {
                    if op == end {
                        break 'decode Status::HasMoreOutput;
                    }
                    output[op] = self.counter as u8;
                    op += 1;
                    self.phase = Phase::Symbols;
                }
                Phase::LengthExtra => {
                    self.counter += bits!(self.extra);
                    self.phase = Phase::Distance;
                }
                Phase::Distance => {
                    let d = huff!(1) as usize;
                    if d >= 32 {
                        fail!();
                    }
                    self.extra = DIST_EXTRA[d];
                    self.dist = DIST_BASE[d];
                    self.phase = Phase::DistanceExtra;
                }
                Phase::DistanceExtra => {
                    self.dist += bits!(self.extra);
                    if !wrapping && (self.dist == 0 || self.dist as usize > op || op == 0) {
                        fail!();
                    }
                    self.history_cursor = op;
                    self.phase = Phase::Copy;
                }
                Phase::Copy => {
                    while self.counter > 0 {
                        if op == end {
                            break 'decode Status::HasMoreOutput;
                        }
                        let src = self.history_cursor.wrapping_sub(self.dist as usize) & mask;
                        // In the defined C domain this index is inside initialized history.
                        if src >= output.len() {
                            fail!();
                        }
                        output[op] = output[src];
                        self.history_cursor = self.history_cursor.wrapping_add(1);
                        op += 1;
                        self.counter -= 1;
                    }
                    self.phase = Phase::Symbols;
                }
                Phase::EndBlock => {
                    if self.final_bits & 1 == 0 {
                        self.phase = Phase::Block;
                        continue;
                    }
                    self.take(self.num_bits & 7);
                    while ip > 0 && self.num_bits >= 8 {
                        ip -= 1;
                        self.num_bits -= 8;
                    }
                    self.bits &= (1u64 << self.num_bits) - 1;
                    self.index = 0;
                    self.phase = if flags & PARSE_ZLIB_HEADER != 0 {
                        Phase::Trailer
                    } else {
                        Phase::Done
                    };
                }
                Phase::Trailer => {
                    while self.index < 4 {
                        let b = if self.num_bits != 0 {
                            bits!(8)
                        } else {
                            byte!() as u32
                        };
                        self.trailer = (self.trailer << 8) | b;
                        self.index += 1;
                    }
                    self.phase = Phase::Done;
                }
                Phase::Done => break 'decode Status::Done,
                Phase::Failed => break 'decode Status::Failed,
            }
        };
        if status != Status::NeedsMoreInput && status != Status::CannotMakeProgress {
            while ip > 0 && self.num_bits >= 8 {
                ip -= 1;
                self.num_bits -= 8;
            }
        }
        self.bits &= (1u64 << self.num_bits) - 1;
        if flags & (PARSE_ZLIB_HEADER | COMPUTE_ADLER32) != 0 && status as i32 >= 0 {
            self.adler = adler32(self.adler, &output[offset..op]);
            if status == Status::Done
                && flags & PARSE_ZLIB_HEADER != 0
                && self.adler != self.trailer
            {
                status = Status::AdlerMismatch;
            }
        }
        Progress {
            status,
            consumed: ip,
            produced: op - offset,
            adler32: self.adler,
        }
    }
}
pub fn adler32(adler: u32, bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (adler & 65535, adler >> 16);
    for chunk in bytes.chunks(5552) {
        for &v in chunk {
            a += v as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}
