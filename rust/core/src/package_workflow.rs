/// Each synchronous pass reborrows all resources and reopens the same entry.
/// A failed pass never retains a handler or a parser session.
pub struct Workflow<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized> {
    pub source: &'a mut S,
    pub backend: &'a mut B,
    pub decoder: Option<InflateScratch<'a>>,
    pub buffer: Option<&'a mut [u8; PARSE_CHUNK]>,
    pub filter: Option<&'a mut crate::xml_input::EntityFilter>,
    pub policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
}
impl<S: BookSource + ?Sized, B: ParserBackend + ?Sized> Workflow<'_, S, B> {
    /// A shorter-lived workflow over the same resources, for a callee that consumes one.
    pub fn reborrow(&mut self) -> Workflow<'_, S, B> {
        Workflow {
            source: &mut *self.source,
            backend: &mut *self.backend,
            decoder: self.decoder.as_mut().map(|s| InflateScratch {
                state: &mut *s.state,
                dictionary: &mut *s.dictionary,
                input: &mut *s.input,
            }),
            buffer: self.buffer.as_deref_mut(),
            filter: self.filter.as_deref_mut(),
            policy: self.policy,
        }
    }
    pub fn pass(&mut self, entry: &Checked, handler: &mut dyn Handler) -> Status {
        let decoder = self.decoder.as_mut().map(|s| InflateScratch {
            state: &mut *s.state,
            dictionary: &mut *s.dictionary,
            input: &mut *s.input,
        });
        crate::xml_session::parse_entry_with_policy(
            self.source,
            entry,
            decoder,
            self.backend,
            handler,
            Resources {
                buffer: self.buffer.as_deref_mut(),
                filter: self.filter.as_deref_mut(),
            },
            false,
            crate::xml_session::OptionalPolicy(self.policy),
        )
    }
}
