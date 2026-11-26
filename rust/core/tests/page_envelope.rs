use newt_core::page_envelope::*;

#[test]
fn null_and_partial_route_preserve_the_original_output_boundary() {
    let mut out = [0xaa; 32];
    assert!(!route(None, &mut out, true));
    assert_eq!(out, [0xaa; 32]);
    let mut name = [b'0'; 69];
    name[64..].copy_from_slice(b".fibp");
    name[4] = b'G';
    assert!(!route(Some(&name), &mut out, true));
    assert_eq!(&out[..3], &[0, 0, 0xaa]);
}

#[test]
fn invalid_inspection_does_not_publish_a_payload() {
    let mut out = 765;
    let mut header = [0; 64];
    manifest(&mut header, 1, &[0; 32], 0, 0);
    assert!(!inspect(&header, 69, &[1; 32], &mut out));
    assert_eq!(out, 765);
    let mut old = header;
    old[4..8].copy_from_slice(&1u32.to_le_bytes());
    let checksum = crc(u32::MAX, &old[..60]) ^ u32::MAX;
    old[60..].copy_from_slice(&checksum.to_le_bytes());
    assert!(!inspect(&old, 69, &[0; 32], &mut out));
    assert_eq!(out, 765);
    assert!(inspect(&header, 69, &[0; 32], &mut out));
    assert_eq!(out, 1);
}

#[test]
fn older_envelope_versions_are_rejected() {
    use newt_core::book_index::{FileKind, FileName};
    let mut header = [0; 64];
    manifest(&mut header, 1, &[0; 32], 0, 0);
    let mut page_name = [b'0'; 69];
    page_name[64..].copy_from_slice(b".fibp");
    let short_index = FileName::new(&[0; 32], FileKind::Index);
    let mut legacy_index = [b'0'; 69];
    legacy_index[64..].copy_from_slice(b".fibx");
    let mut bad_hex_index = legacy_index;
    bad_hex_index[0] = b'g';
    let mut trailing_index = [b'0'; 73];
    trailing_index[64..69].copy_from_slice(b".fibx");
    trailing_index[69..].copy_from_slice(b".bak");
    let stamped = |version: u32| {
        let mut old = header;
        old[4..8].copy_from_slice(&version.to_le_bytes());
        let checksum = crc(u32::MAX, &old[..60]) ^ u32::MAX;
        old[60..].copy_from_slice(&checksum.to_le_bytes());
        old
    };
    // Pages (.fibp) and unnamed manifests reject v1..=v4 and future versions.
    for other in [1u32, 2, 3, 4, 6] {
        let old = stamped(other);
        let mut out = 765;
        assert!(!inspect(&old, 69, &[0; 32], &mut out), "accepted v{other}");
        assert_eq!(out, 765);
        assert!(
            !inspect_for(Some(&page_name), &old, 69, &[0; 32], &mut out),
            "accepted .fibp v{other}"
        );
        assert_eq!(out, 765);
    }
    // Book indexes (.IDX and legacy .fibx) reject v1, v2 and future versions,
    // while admitting v3, v4 and v5.
    for (label, index_name) in [
        (".IDX", short_index.as_bytes().as_slice()),
        (".fibx", legacy_index.as_slice()),
    ] {
        for rejected in [1u32, 2, 6] {
            let old = stamped(rejected);
            let mut out = 765;
            assert!(
                !inspect_for(Some(index_name), &old, 69, &[0; 32], &mut out),
                "accepted {label} v{rejected}"
            );
            assert_eq!(out, 765);
        }
        for admitted in [3u32, 4, 5] {
            let mut out = 765;
            assert!(
                inspect_for(Some(index_name), &stamped(admitted), 69, &[0; 32], &mut out),
                "rejected {label} v{admitted}"
            );
            assert_eq!(out, 1);
        }
    }
    // Non-canonical index names (.TMP/.WRK/.BAK or malformed legacy names) do not
    // receive older-version index admission.
    for non_index in [
        short_index.with_kind(FileKind::Temporary).as_bytes().as_slice(),
        short_index.with_kind(FileKind::Work).as_bytes().as_slice(),
        short_index.with_kind(FileKind::Backup).as_bytes().as_slice(),
        bad_hex_index.as_slice(),
        trailing_index.as_slice(),
    ] {
        for older in [3u32, 4] {
            let mut out = 765;
            assert!(
                !inspect_for(Some(non_index), &stamped(older), 69, &[0; 32], &mut out),
                "accepted non-canonical index v{older}"
            );
            assert_eq!(out, 765);
        }
    }
    let mut out = 765;
    assert!(inspect(&header, 69, &[0; 32], &mut out));
    assert_eq!(out, 1);
}

#[test]
fn standard_crc_and_chunked_update_match() {
    assert_eq!(crc(u32::MAX, b"123456789") ^ u32::MAX, 0xcbf43926);
    assert_eq!(
        crc(crc(u32::MAX, b"1234"), b"56789"),
        crc(u32::MAX, b"123456789")
    );
}
