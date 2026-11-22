/// Sources' retained record lives at parse's top; its text lives below Home in
/// shared. Its descriptor is recreated per scope, never self-referential.
#[derive(Default)]
struct SourceRecord {
    links: [styles::Member; 4],
    start: [u16; 8],
    length: [u16; 8],
    issues: u16,
    used: usize,
    capacity: usize,
    offset: usize,
    events: usize,
    external_mask: u8,
}
impl SourceRecord {
    fn with_text<R>(
        &mut self,
        text: &mut [u8],
        operation: impl FnOnce(&mut styles::Sources<'_>) -> R,
    ) -> R {
        let mut sources = styles::Sources {
            links: self.links,
            start: self.start,
            length: self.length,
            issues: self.issues,
            used: self.used,
            events: self.events,
            external_mask: self.external_mask,
            text,
        };
        let r = operation(&mut sources);
        self.links = sources.links;
        self.start = sources.start;
        self.length = sources.length;
        self.issues = sources.issues;
        self.used = sources.used;
        self.events = sources.events;
        self.external_mask = sources.external_mask;
        r
    }
}
