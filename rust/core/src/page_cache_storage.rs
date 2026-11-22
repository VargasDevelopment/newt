// CacheStorage contract from pinned BookStorage.h:36-65.
/// Exactly the SDK cache operations used here. A read returns <= dst.len().
/// The adapter owns one active write and atomically publishes it at end_write.
pub trait CacheStorage {
    fn exists(&mut self, name: &[u8]) -> bool { self.file_size(name) >= 0 }
    fn remove(&mut self, name: &[u8]) -> bool;
    fn file_size(&mut self, name: &[u8]) -> i64;
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32;
    fn begin_write(&mut self, name: &[u8]) -> bool;
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
}