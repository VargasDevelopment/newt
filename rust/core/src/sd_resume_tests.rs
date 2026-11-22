// SPDX-License-Identifier: MIT
use super::{RETAINED_WORDS, SdResumeRecord};

#[test]
fn primitive_fields_preserve_every_cold_word_without_layout_casts() {
    let words = core::array::from_fn(|index| (index as u32 + 1) * 0x0403_0201);
    let record = SdResumeRecord::from_words(&words);
    assert_eq!((record.magic, record.version, record.checksum), (words[0], words[1], words[34]));
    assert_eq!(record.digest[..4], words[2].to_le_bytes());
    assert_eq!(record.digest[28..], words[9].to_le_bytes());
    assert_eq!(record.filename[..4], words[10].to_le_bytes());
    assert_eq!(record.filename[92..], words[33].to_le_bytes());
    let mut output = [u32::MAX; RETAINED_WORDS];
    record.write_words(&mut output);
    assert_eq!(output, words);
    assert!(!record.valid());
}

#[test]
fn valid_and_corrupt_records_keep_the_original_checksum_and_magic() {
    let record = SdResumeRecord::new(c"fixture.epub", [13; 32]).unwrap();
    let mut words = [0; RETAINED_WORDS];
    record.write_words(&mut words);
    assert_eq!(SdResumeRecord::from_words(&words), record);
    assert!(SdResumeRecord::from_words(&words).valid());
    for index in [0, 1, 2, 9, 10, 33, 34] {
        let mut damaged = words;
        damaged[index] ^= 0x8080_8080;
        let decoded = SdResumeRecord::from_words(&damaged);
        assert!(!decoded.valid(), "field {index}");
        let mut output = [0; RETAINED_WORDS];
        decoded.write_words(&mut output);
        assert_eq!(output, damaged);
    }
}
