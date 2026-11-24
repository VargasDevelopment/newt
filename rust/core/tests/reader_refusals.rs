use newt_core::{save_notice::{ReaderIssue, ReadingSaveNotice}, zip_container::Status};

#[test]
fn required_book_refusals_keep_runtime_memory_and_existing_codes_distinct() {
    for (code, status) in [Status::Ok, Status::IoError, Status::NotZip, Status::NotEpub,
        Status::Encrypted, Status::Truncated, Status::Unsupported, Status::OutOfMemory,
        Status::ParseError, Status::NotFound, Status::Stale].into_iter().enumerate() {
        assert_eq!(status as usize, code, "existing status codes must not shift");
    }
    for (status, issue, message) in [
        (Status::MetadataTooLarge, ReaderIssue::MetadataTooLarge, "Book metadata is too large. Position kept."),
        (Status::PathTooLong, ReaderIssue::PathTooLong, "An EPUB path is too long. Position kept."),
        (Status::OutOfMemory, ReaderIssue::Memory, "Reader memory is unavailable. Position kept."),
    ] {
        assert_eq!(ReaderIssue::for_open_failure(status), issue);
        assert_eq!(issue.text().unwrap().to_str().unwrap(), message);
        let mut notice = ReadingSaveNotice::default();
        notice.set_reader(issue);
        assert_eq!(ReadingSaveNotice::from_bytes(notice.to_bytes()).reader(), issue as u8);
    }
    assert_eq!(Status::MetadataTooLarge as u8, 11);
    assert_eq!(Status::PathTooLong as u8, 12);
    for (code, issue) in [ReaderIssue::None, ReaderIssue::Session, ReaderIssue::Memory,
        ReaderIssue::InvalidCheckpoint, ReaderIssue::InvalidText, ReaderIssue::Io,
        ReaderIssue::NotEpub, ReaderIssue::Encrypted, ReaderIssue::Truncated, ReaderIssue::Unsupported,
        ReaderIssue::Parse, ReaderIssue::Unavailable, ReaderIssue::SourceUnavailable].into_iter().enumerate() {
        assert_eq!(issue as usize, code, "existing notice codes must not shift");
    }
    assert_eq!(ReaderIssue::MetadataTooLarge as u8, 13);
    assert_eq!(ReaderIssue::PathTooLong as u8, 14);
}
