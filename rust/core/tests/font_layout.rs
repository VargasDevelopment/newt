use newt_core::font_layout::{ligature_glyph_id, pair_kern_adjustment};
#[test]
fn absent_and_truncated_tables() {
    for n in 0..32 {
        let table = vec![0xff; n];
        assert_eq!(pair_kern_adjustment(&table, 1, 2), 0);
        assert_eq!(ligature_glyph_id(&table, &[1, 2]), 0);
    }
}
#[test]
fn glyph_inputs_are_u32_and_lengths_are_borrowed() {
    assert_eq!(pair_kern_adjustment(&[], u32::MAX, 1), 0);
    for sequence in [&[][..], &[1][..], &[1, 2, 3, 4][..]] {
        assert_eq!(ligature_glyph_id(&[], sequence), 0);
    }
}
fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}
fn wrap(kind: u16, sub: &[u8]) -> Vec<u8> {
    let mut scripts = words(&[1]);
    scripts.extend_from_slice(b"latn");
    scripts.extend(words(&[8, 4, 0, 0, 0xffff, 1, 0]));
    let mut features = words(&[1]);
    features.extend_from_slice(if kind == 2 { b"kern" } else { b"liga" });
    features.extend(words(&[8, 0, 1, 0]));
    let mut lookups = words(&[1, 4, kind, 0, 1, 8]);
    lookups.extend_from_slice(sub);
    let mut table = words(&[
        1,
        0,
        10,
        10 + scripts.len() as u16,
        10 + scripts.len() as u16 + features.len() as u16,
    ]);
    table.extend(scripts);
    table.extend(features);
    table.extend(lookups);
    table
}
#[test]
fn signed_pair_and_full_width_glyph_inputs() {
    // PairPos1, coverage(10), pair set {20: -37 FUnits}.
    let table = wrap(
        2,
        &words(&[1, 12, 4, 0, 1, 18, 1, 1, 10, 1, 20, 65535 - 36]),
    );
    assert_eq!(pair_kern_adjustment(&table, 10, 20), -37);
    assert_eq!(pair_kern_adjustment(&table, 65546, 20), 0);
    assert_eq!(pair_kern_adjustment(&table, 10, 65556), 0);
    assert_eq!(pair_kern_adjustment(&table, 0, 20), 0);
}
#[test]
fn ligature_ids_and_component_length() {
    let table = wrap(4, &words(&[1, 8, 1, 14, 1, 1, 10, 1, 4, 77, 2, 20]));
    assert_eq!(ligature_glyph_id(&table, &[10, 20]), 77);
    assert_eq!(ligature_glyph_id(&table, &[10, 21]), 0);
    assert_eq!(ligature_glyph_id(&table, &[10, 20, 30]), 0);
}
