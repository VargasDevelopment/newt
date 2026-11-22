// Newt MIT; scoped callback at frozen FilePort::producerDestinationSafe.
impl<'op, 'a, S: Native> Operation<'op, 'a, S> {
    fn canonical(&self) -> Path {
        let mut path = self.port.state.path;
        path.truncate(self.port.state.final_bytes);
        path
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
