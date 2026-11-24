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
    // The widest test build: tools/nightly.py's form at the 23-character
    // limit, in 0, the widest digit. Its own line must hold it uncut.
    let widest = b"0.0.0-dev.20000000.0000";
    assert_eq!(updated_title(&mut line, widest), (b"Updated to".as_slice(), Some(widest.as_slice())));
    assert!(width(widest, 32, true) <= SYSTEM_CONTENT_WIDTH);
}
