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
