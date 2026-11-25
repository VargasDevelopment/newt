// Newt MIT; scoped callback at frozen FilePort::producerDestinationSafe.
impl<'op, 'a, S: Native> Operation<'op, 'a, S> {
    fn canonical(&self) -> Path {
        let mut path = self.port.state.path;
        path.truncate(self.port.state.final_bytes);
        if self.port.state.owns && newt_core::book_index::FileName::parse(self.suffix().as_bytes()).is_some() {
            path.truncate(path.len()-3);
            path.write_str("IDX").expect("same-width index extension");
        }
        path
    }
    fn backup_name(&self)->Option<Path> {
        let mut path=self.canonical();
        if newt_core::book_index::FileName::parse(self.suffix().as_bytes()).is_some() {
            path.truncate(path.len()-3);path.write_str("BAK").ok()?;
        } else {path.write_str(".bak").ok()?;}
        Some(path)
    }
    /// Reborrow the same port and policy; this view never detaches either.
    fn with_receipt_check<'short, R>(
        &'short mut self,
        receipt_live: &'short mut dyn FnMut() -> bool,
        operation: impl FnOnce(Operation<'short, 'a, S>) -> R,
    ) -> R {
        operation(Operation {
            port: &mut *self.port,
            producer: self.producer,
            receipt_live: Some(receipt_live),
        })
    }
    fn producer_invalid(&mut self) -> bool {
        if self.receipt_live.as_deref_mut().is_some_and(|check| !check()) {
            return true;
        }
        self.producer.is_some_and(|policy| policy.authority.invalid())
    }
}
