//! The cover CRC's table must give the values of the bitwise CRC it replaced
//! (#74), or every saved cover record would read as stale.
use newt_core::cover::crc;

fn bitwise(mut value: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 { value = (value >> 1) ^ if value & 1 != 0 { 0xedb8_8320 } else { 0 }; }
    }
    value
}

fn noise(length: usize, mut seed: u32) -> Vec<u8> {
    (0..length).map(|_| { seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (seed >> 24) as u8 }).collect()
}

#[test]
fn matches_the_bitwise_crc() {
    assert_eq!(crc(u32::MAX, b"123456789") ^ u32::MAX, 0xcbf4_3926);
    assert_eq!(crc(u32::MAX, &[]), u32::MAX);
    for byte in 0..=255 { assert_eq!(crc(u32::MAX, &[byte]), bitwise(u32::MAX, &[byte])); }
    for (length, seed) in [(3, 1), (511, 2), (1023, 3), (4097, 4), (8192, 5), (8193, 6), (65_537, 7)] {
        let bytes = noise(length, seed);
        assert_eq!(crc(u32::MAX, &bytes), bitwise(u32::MAX, &bytes), "length {length}");
        assert_eq!(crc(seed, &bytes), bitwise(seed, &bytes), "length {length} from {seed}");
    }
}

#[test]
fn chunks_give_the_whole_crc() {
    let bytes = noise(3 * 8192 + 5, 9);
    let whole = bitwise(u32::MAX, &bytes);
    for step in [1, 1023, 1024, 4096, 8191, 8192] {
        let chunked = bytes.chunks(step).fold(u32::MAX, crc);
        assert_eq!(chunked, whole, "step {step}");
    }
    // An unaligned start, as a read that resumes mid-block.
    let (head, tail) = bytes.split_at(7);
    assert_eq!(crc(crc(u32::MAX, head), tail), whole);
}
