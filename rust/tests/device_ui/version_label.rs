use super::{SYSTEM_CONTENT_WIDTH, updated_title, version_label, width};

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

#[test]
fn updated_title_shows_the_whole_version() {
    let mut line = [0; 34];
    assert_eq!(
        updated_title(&mut line, b"0.4.0"),
        (b"Updated to 0.4.0".as_slice(), None)
    );
    // The widest 23-character nightly (tools/nightly.py) with a date through
    // 2035 measures exactly the line's 428 px: '4' is the widest digit and
    // "0." kerns tighter. From 2040, a 4-digit counter may be cut.
    let widest = b"0.0.4-dev.20300404.4444";
    assert_eq!(updated_title(&mut line, widest), (b"Updated to".as_slice(), Some(widest.as_slice())));
    assert_eq!(width(widest, 32, true), SYSTEM_CONTENT_WIDTH);
}
