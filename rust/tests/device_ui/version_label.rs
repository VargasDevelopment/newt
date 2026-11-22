use super::version_label;

#[test]
fn version_label_preserves_short_and_full_width_text() {
    let mut line = [0; 34];
    assert_eq!(
        version_label(&mut line, b"Version: ", b"0.2.0"),
        b"Version: 0.2.0"
    );
    assert_eq!(
        version_label(&mut line, b"Updated to ", b"0.2.1-dev.20261122.1234"),
        b"Updated to 0.2.1-dev.20261122.1234"
    );
}
