use super::Digest;
pub const SLOTS: u32 = 3;
pub const DIGEST_KEY_BYTES: usize = 52;
pub type Key = [u8; 61];
const ALPHABET: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
pub fn make(digest: &Digest, slot: u32, key: &mut Key) {
    key[..3].copy_from_slice(&[b'c', slot.wrapping_add(u32::from(b'0')) as u8, b'-']);
    let (mut value, mut bits, mut at) = (0u32, 0u32, 3usize);
    for &byte in digest {
        value = value.wrapping_shl(8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            key[at] = ALPHABET[((value >> bits) & 31) as usize];
            at += 1;
        }
    }
    if bits != 0 {
        key[at] = ALPHABET[((value << (5 - bits)) & 31) as usize];
        at += 1;
    }
    key[at..].copy_from_slice(b".ntci\0");
}
/// Length-addressed, including embedded NUL rejection in the digest alphabet.
pub fn matches(name: &[u8]) -> bool {
    name.len() == 60
        && name[0] == b'c'
        && (b'0'..b'3').contains(&name[1])
        && name[2] == b'-'
        && &name[55..] == b".ntci"
        && name[3..55].iter().all(|b| ALPHABET.contains(b))
        && matches!(name[54], b'0' | b'g')
}
