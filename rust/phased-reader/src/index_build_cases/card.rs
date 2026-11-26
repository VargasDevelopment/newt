#[derive(Default)]
struct Card {
    file: Vec<u8>, pending: Vec<u8>, work: Vec<u8>, census: u32, creates: u32,
    source_released: std::rc::Rc<Cell<bool>>,
    writes: u32, spilled_once: std::rc::Rc<Cell<bool>>, removed: bool, fail: bool, open: u32, most: u32, verified: bool,
}
impl Card {
    fn descriptor(&mut self) { self.open += 1; self.most = self.most.max(self.open); assert!(self.open <= 3); }
}
impl Store for Card {
    fn census(&mut self, cancelled: &dyn Fn() -> bool) { self.census += 1; cancelled(); }
    fn spilled(&self) -> bool { !self.work.is_empty() }
    fn release_pages(&mut self) -> bool { self.pending.clear(); self.open = 2 - u32::from(self.source_released.get()) + u32::from(!self.work.is_empty()); true }
}
impl WorkFile for Card {
    fn append(&mut self, bytes: &[u8]) -> bool {
        if self.fail { return false; }
        if self.work.is_empty() { self.descriptor(); }
        self.spilled_once.set(true);
        self.work.extend_from_slice(bytes); true
    }
    fn read_at(&mut self, at: u32, out: &mut [u8]) -> bool {
        let Some(bytes) = self.work.get(at as usize..at as usize+out.len()) else { return false; };
        out.copy_from_slice(bytes); true
    }
    fn remove_work(&mut self) -> bool {
        if !self.work.is_empty() { self.open -= 1; }
        self.work.clear(); self.removed = true; true
    }
    fn sync_work(&mut self) -> bool { true }
}
impl CacheStorage for Card {
    fn remove(&mut self, _: &[u8]) -> bool { self.file.clear(); true }
    fn file_size(&mut self, _: &[u8]) -> i64 { if self.file.is_empty() { -1 } else { self.file.len() as i64 } }
    fn read_at(&mut self, name: &[u8], at: u32, out: &mut [u8]) -> i32 {
        if newt_core::cache_names::namespace::envelope_kind(name)==Some(newt_core::cache_names::namespace::Kind::Index) && !self.verified { return -1; }
        let Some(bytes) = self.file.get(at as usize..at as usize+out.len()) else { return -1; };
        out.copy_from_slice(bytes); out.len() as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool { self.creates += 1; self.descriptor(); !self.fail }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.writes += 1;
        if self.fail { return false; }
        self.pending.extend_from_slice(bytes); true
    }
    fn end_write(&mut self) -> bool { self.file = core::mem::take(&mut self.pending); self.open -= 1; true }
}
impl ReadScope for Card {
    fn query<R>(&mut self, _: &[u8], _: bool, scratch: &mut [u8],
        live: &mut dyn FnMut() -> bool, read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        assert!(scratch.len() >= 4160 + 4096);
        if !live() { return Err(Status::Stale); }
        self.verified = true;
        let result = read(self);
        self.verified = false;
        Ok((result, live()))
    }
}
impl CachePresence for Card {
    fn with_index<R>(&mut self, _: &[u8; 32], op: impl FnOnce(&mut dyn Store) -> R) -> Option<R> {
        Some(op(self))
    }
    fn present(&mut self, _: &[u8], _: bool) -> bool { !self.file.is_empty() }
}
