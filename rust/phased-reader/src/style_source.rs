struct FaultTrack<'a, S: BookSource + ?Sized> {
    source: &'a mut S,
    bytes: u64,
    fault: bool,
}
impl<S: BookSource + ?Sized> BookSource for FaultTrack<'_, S> {
    fn size(&self) -> u64 {
        self.source.size()
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let n = self.source.read_at(at, out);
        if n < 0 || (!out.is_empty() && n == 0) {
            self.fault = true;
        }
        n
    }
}
impl<S: BookSource + ?Sized> FaultTrack<'_, S> {
    fn damaged(&mut self) -> bool {
        !self.fault && self.source.size() == self.bytes && self.source.read_at(0, &mut []) == 0
    }
}
