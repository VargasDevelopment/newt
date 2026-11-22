//! Only the frozen work profile's pure, closed operation and route domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReaderIoOperation { Size, Position, Seek, Read }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReaderHashOperation { Begin, End, Update, Wait }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ReaderOpenRoute {
    WarmAttempt = 1 << 0,
    WarmHit = 1 << 1,
    WarmMiss = 1 << 2,
    ImportBegin = 1 << 3,
    ImportFailed = 1 << 4,
    ImportVerified = 1 << 5,
    CompactAdmission = 1 << 6,
    StrictAdmission = 1 << 7,
    SealFailed = 1 << 8,
    SealSucceeded = 1 << 9,
    StrictFallback = 1 << 10,
    ReceiptLoaded = 1 << 11,
    CanonicalSelected = 1 << 12,
    ChapterVerified = 1 << 13,
    CacheProbe = 1 << 14,
    CacheHit = 1 << 15,
    CacheMiss = 1 << 16,
    CacheBuild = 1 << 17,
    OrdinaryPage = 1 << 18,
    PageReady = 1 << 19,
    ProjectionBegin = 1 << 20,
    ProjectionEnd = 1 << 21,
    PrefetchRunning = 1 << 22,
    PrefetchComplete = 1 << 23,
    PrefetchStopped = 1 << 24,
    TimingInvalid = 1 << 31,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReaderOpenTerminal { None, SourceUnavailable, ProgressUnavailable, SessionUnavailable, Reader }
