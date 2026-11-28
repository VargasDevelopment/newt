// CacheStorage contract from pinned BookStorage.h:36-65.
/// Exactly the SDK cache operations used here. A read returns <= dst.len().
/// The adapter owns one active write and atomically publishes it at end_write.
pub trait CacheStorage {
    fn exists(&mut self, name: &[u8]) -> bool { self.file_size(name) >= 0 }
    fn remove(&mut self, name: &[u8]) -> bool;
    fn file_size(&mut self, name: &[u8]) -> i64;
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32;
    fn begin_write(&mut self, name: &[u8]) -> bool;
    /// Exact payload size, when known before the first write. Stores may reserve it.
    fn begin_write_sized(&mut self, name: &[u8], _payload: u32) -> bool {
        self.begin_write(name)
    }
    fn write(&mut self, bytes: &[u8]) -> bool;
    fn end_write(&mut self) -> bool;
    /// Native stores lend this check to the final destination-safety boundary.
    /// Memory stores have no rename boundary and check immediately before end.
    fn end_write_checked(&mut self, receipt_live: &mut dyn FnMut() -> bool) -> bool {
        receipt_live() && self.end_write()
    }
    fn read_back_at(&mut self, _offset: u32, _dst: &mut [u8]) -> i32 {
        -1
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_micros(&self) -> u32 {
        0
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_begin(&mut self, _idx: u32) {}
    #[cfg(feature = "page-turn-trace")]
    fn trace_add_tee_us(&mut self, _us: u32) {}
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_layout_done(&mut self) {}
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_end(&mut self, _pages: u32) {}
    /// Positive witness that the most recent `end_write` durably published its
    /// canonical file, even if a later post-promotion cancellation check failed.
    fn last_write_published(&self) -> bool {
        false
    }
    /// Whether the active producer grant can begin another file in this attempt.
    fn producer_can_begin(&self) -> bool {
        true
    }
}
/// One checked cache read, as pinned ChapterPageCache readEntry/locate:
/// beginRead(query), attach, the SDK read, query.finish, finishRead.
/// `opening` is `!hasPage`; `scratch` is the parse loan beyond the
/// index, where a native store places its verified query. `read` gets the
/// store to read from; Ok's bool is the checked exit. A scope that could not
/// begin or attach is Err(Stale). No default success implementation: the
/// caller supplies real scope authority. Check source_live after admission
/// before attach, then after query cleanup before the checked exit, including
/// failures; the read callback has no raw source capability.
pub trait ReadScope: CacheStorage {
    /// A cancelled operation ends its scope here. A refused same-name
    /// descriptor stays latched until an explicit close, so without this the
    /// next operation's first read is refused too and reads as a damaged
    /// index (#199).
    fn close_after_cancel(&mut self) {}
    /// A failed consumed index block must not leave a header-only cache hit.
    fn invalidate_index(&mut self, digest: &[u8; 32]) -> bool {
        let name = crate::book_index::FileName::new(digest, crate::book_index::FileKind::Index);
        self.remove(name.as_bytes())
    }
    fn query_index<R>(&mut self,name:&[u8],_digest:&[u8;32],opening:bool,scratch:&mut[u8],
        source_live:&mut dyn FnMut()->bool,read:impl FnOnce(&mut dyn CacheStorage)->R)
        ->Result<(R,bool),Status> {
        self.query(name,opening,scratch,source_live,read)
    }
    fn query<R>(
        &mut self,
        name: &[u8],
        opening: bool,
        scratch: &mut [u8],
        source_live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status>;
}
