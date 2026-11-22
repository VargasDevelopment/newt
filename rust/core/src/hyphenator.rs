//! Borrowed FIBH matcher. See tests/hyphenator/CONTRACT.md for the parity domain.

pub struct Hyphenator<'a> {
    data: &'a [u8],
    count: usize,
    blob: usize,
    max_len: u8,
    records_valid: bool,
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

impl<'a> Hyphenator<'a> {
    pub const fn new() -> Self {
        Self {
            data: &[],
            count: 0,
            blob: 0,
            max_len: 0,
            records_valid: true,
        }
    }

    /// Adopts only the header, as native does. Failed init clears readiness.
    /// Overflowing native uint32 table-size arithmetic is outside parity.
    pub fn init(&mut self, data: Option<&'a [u8]>) -> bool {
        self.count = 0;
        self.records_valid = true;
        let Some(data) = data else { return false };
        if data.len() < 12 || &data[..4] != b"FIBH" || data[4..6] != [1, 0] {
            return false;
        }
        self.max_len = data[6];
        let Some(count) = u32_at(data, 8).and_then(|n| usize::try_from(n).ok()) else {
            return false;
        };
        let Some(blob) = count.checked_mul(4).and_then(|n| n.checked_add(12)) else {
            return false;
        };
        if blob > data.len() || blob > u32::MAX as usize {
            return false;
        }
        self.data = data;
        self.blob = blob;
        self.count = count;
        self.records_valid = self.validate_records();
        true
    }

    pub fn ready(&self) -> bool {
        self.count > 0
    }
    pub fn pattern_count(&self) -> usize {
        self.count
    }
    pub fn max_pattern_len(&self) -> u8 {
        self.max_len
    }

    fn record(&self, index: usize) -> Option<(&[u8], &[u8])> {
        if index >= self.count {
            return None;
        }
        let offset = usize::try_from(u32_at(
            self.data,
            12usize.checked_add(index.checked_mul(4)?)?,
        )?)
        .ok()?;
        let at = self.blob.checked_add(offset)?;
        let len = usize::from(*self.data.get(at)?);
        let begin = at.checked_add(1)?;
        let end = begin.checked_add(len)?;
        let values_end = end.checked_add(len.checked_add(1)?)?;
        Some((self.data.get(begin..end)?, self.data.get(end..values_end)?))
    }

    /// Immutable borrowed data needs validation only when adopted, not per word.
    pub fn records_valid(&self) -> bool {
        self.records_valid
    }

    fn validate_records(&self) -> bool {
        let mut previous: Option<&[u8]> = None;
        for index in 0..self.count {
            let Some((key, _)) = self.record(index) else {
                return false;
            };
            if key.is_empty() || previous.is_some_and(|p| p >= key) {
                return false;
            }
            previous = Some(key);
        }
        true
    }

    /// Writes increasing UTF-8 byte offsets; at most min(output.len(), 255).
    /// Unsupported words/records return zero without touching output.
    pub fn break_positions(&self, word: &[u8], output: &mut [u8]) -> usize {
        if !self.ready() || word.len() < 5 || word.len() > 62 || !self.records_valid() {
            return 0;
        }
        let Ok(text) = core::str::from_utf8(word) else {
            return 0;
        };
        let mut marked = [0u8; 66];
        let mut char_index = [0u8; 66];
        marked[0] = b'.';
        let mut char_count = 0u8;
        for (at, ch) in text.char_indices() {
            // Native's encoder has no four-byte arm: exclude that undefined domain.
            if ch as u32 > 0xffff {
                return 0;
            }
            let Some(folded) = char::from_u32(fold_case(ch as u32)) else {
                return 0;
            };
            let mut encoded = [0u8; 4];
            let bytes = folded.encode_utf8(&mut encoded).as_bytes();
            if bytes.len() != ch.len_utf8() {
                return 0;
            }
            marked[1 + at..1 + at + bytes.len()].copy_from_slice(bytes);
            char_index[at..at + bytes.len()].fill(char_count);
            char_count += 1;
        }
        if char_count < 5 {
            return 0;
        }
        marked[word.len() + 1] = b'.';
        let marked_len = word.len() + 2;
        let mut values = [0u8; 67];
        for start in 0..marked_len {
            let mut lo = 0;
            let mut hi = self.count;
            for depth in 0..(marked_len - start).min(usize::from(self.max_len)) {
                if lo >= hi {
                    break;
                }
                let c = marked[start + depth];
                let mut a = lo;
                let mut b = hi;
                while a < b {
                    let mid = a + (b - a) / 2;
                    let Some((key, _)) = self.record(mid) else {
                        return 0;
                    };
                    if key.len() <= depth || key[depth] < c {
                        a = mid + 1;
                    } else {
                        b = mid;
                    }
                }
                lo = a;
                b = hi;
                while a < b {
                    let mid = a + (b - a) / 2;
                    let Some((key, _)) = self.record(mid) else {
                        return 0;
                    };
                    if key.len() <= depth || key[depth] <= c {
                        a = mid + 1;
                    } else {
                        b = mid;
                    }
                }
                hi = a;
                if lo >= hi {
                    break;
                }
                let Some((key, weights)) = self.record(lo) else {
                    return 0;
                };
                if key.len() == depth + 1 {
                    for v in 0..=key.len() {
                        if weights[v] > values[start + v] {
                            values[start + v] = weights[v];
                        }
                    }
                }
            }
        }
        let mut found = 0;
        for v in 1..marked_len - 1 {
            if found >= output.len().min(255) {
                break;
            }
            let pos = v - 1;
            if values[v] & 1 == 0 || word[pos] & 0xc0 == 0x80 {
                continue;
            }
            let left = char_index[pos];
            if left < 2 || char_count - left < 3 {
                continue;
            }
            output[found] = pos as u8;
            found += 1;
        }
        found
    }
}

fn fold_case(cp: u32) -> u32 {
    if (0x41..=0x5a).contains(&cp)
        || ((0xc0..=0xde).contains(&cp) && cp != 0xd7)
        || ((0x391..=0x3a9).contains(&cp) && cp != 0x3a2)
        || (0x410..=0x42f).contains(&cp)
    {
        return cp + 0x20;
    }
    if ((0x100..=0x137).contains(&cp) && cp & 1 == 0)
        || ((0x139..=0x148).contains(&cp) && cp & 1 == 1)
        || ((0x14a..=0x177).contains(&cp) && cp & 1 == 0)
        || ((0x179..=0x17e).contains(&cp) && cp & 1 == 1)
    {
        return cp + 1;
    }
    if cp == 0x178 {
        return 0xff;
    }
    if (0x400..=0x40f).contains(&cp) {
        return cp + 0x50;
    }
    cp
}

impl Default for Hyphenator<'_> {
    fn default() -> Self {
        Self::new()
    }
}
