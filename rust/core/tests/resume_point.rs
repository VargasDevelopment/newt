use newt_core::resume_point::*;
use newt_core::xml_input::EntityFilter;

fn header(mode: u8, lengths: [u32; 7]) -> [u8; HEADER_BYTES] {
    let mut h = [0; HEADER_BYTES];
    h[..4].copy_from_slice(&MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = mode;
    for (i, length) in lengths.into_iter().enumerate() {
        h[8 + 4 * i..12 + 4 * i].copy_from_slice(&length.to_le_bytes());
    }
    h
}

fn total(lengths: [u32; 7]) -> u32 {
    HEADER_BYTES as u32 + lengths.iter().sum::<u32>() + TRAILER_BYTES
}

#[test]
fn sections_are_contiguous_in_consumption_order() {
    let lengths = [40, 12, 30, 9000, 0, 7, 300];
    let layout = Layout::parse(&header(1, lengths), total(lengths)).unwrap();
    assert_eq!(layout.mode, Mode::Reinflate);
    let mut at = HEADER_BYTES as u32;
    for (section, length) in SECTIONS.into_iter().zip(lengths) {
        assert_eq!(layout.range(section), (at, length), "{section:?}");
        at += length;
    }
    assert_eq!(layout.trailer(), at);
    assert_eq!(at + TRAILER_BYTES, total(lengths));
    // Restore reads the parser input before the engine, then the feed bytes.
    assert_eq!(&SECTIONS[1..4], &[Section::Source, Section::Stream, Section::Engine]);
    assert_eq!(&SECTIONS[4..], &[Section::Prolog, Section::Names, Section::Replay]);
}

#[test]
fn every_header_field_and_bound_is_checked() {
    let lengths = [1, 1, 1, 1, 0, 1, 0];
    let point = total(lengths);
    assert_eq!(Layout::parse(&header(0, lengths), point).unwrap().mode, Mode::Seek);
    assert_eq!(Layout::parse(&header(0, lengths), point + 1), None);
    assert_eq!(Layout::parse(&header(2, lengths), point), None);
    for at in [0, 4, 7] {
        let mut bad = header(0, lengths);
        bad[at] ^= 1;
        assert_eq!(Layout::parse(&bad, point), None, "byte {at}");
    }
    for (i, section) in SECTIONS.into_iter().enumerate() {
        let mut over = lengths;
        over[i] = section.max_bytes() + 1;
        assert_eq!(Layout::parse(&header(0, over), total(over)), None, "{section:?} over");
        let mut full = lengths;
        full[i] = section.max_bytes();
        assert!(Layout::parse(&header(0, full), total(full)).is_some(), "{section:?} at cap");
        if !matches!(section, Section::Prolog | Section::Replay) {
            let mut empty = lengths;
            empty[i] = 0;
            assert_eq!(Layout::parse(&header(0, empty), total(empty)), None, "{section:?} empty");
        }
    }
}

#[test]
fn the_stream_section_holds_the_whole_entity_filter() {
    // Host layout, usize = 8: an upper bound on any packed encoding of the
    // filter, plus the two u64 stream counters.
    let state = size_of::<EntityFilter>() + 2 * size_of::<u64>();
    assert!(state >= 512 + 32 + 13, "the filter keeps its raw, pending and entity buffers");
    assert!(state <= Section::Stream.max_bytes() as usize, "{state} B");
}
