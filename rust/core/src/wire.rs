//! Explicit wire operations; native Rust object layout is never persisted.

pub fn get32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated record extent"),
    )
}

pub fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn crc32(bytes: &[u8]) -> u32 {
    crc(bytes, 0xedb8_8320)
}

pub fn crc32c(bytes: &[u8]) -> u32 {
    crc(bytes, 0x82f6_3b78)
}

fn crc(bytes: &[u8], polynomial: u32) -> u32 {
    let mut value = u32::MAX;
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 {
            value = (value >> 1) ^ if value & 1 != 0 { polynomial } else { 0 };
        }
    }
    !value
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2_166_136_261u32, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}
