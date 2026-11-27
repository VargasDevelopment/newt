// MIT. Included inside canonical.rs; complete-table preparation only.
struct SemanticHandler(crate::book_semantic_index::Counter);
impl Handler for SemanticHandler {
    fn start_element(&mut self, name: &[u8], attributes: &mut dyn Attributes) {
        let hidden = crate::css::parse_inline_style(attributes.local(b"style")).display_none == 1;
        let image = attributes.local(b"src").is_some() || attributes.local(b"href").is_some();
        let alt = if crate::chapter_layout::text::local(name) == b"img" { attributes.local(b"alt") } else { None };
        self.0.start_element(name, hidden, image, alt);
    }
    fn end_element(&mut self, name: &[u8]) {
        self.0.end_element(name);
    }
    fn text(&mut self, text: &[u8]) {
        self.0.text(text);
    }
}

/// Count one already checked chapter using the shared complete-stream policy.
pub fn semantic_count_member<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>, entry: &Checked, on_yield: crate::admission::Yield<'_>,
) -> Result<u32, Status> {
    let mut handler = SemanticHandler(Default::default());
    let mut chapter = match crate::admission::ChapterSource::open(
        workflow.source,
        entry,
        workflow.decoder.as_mut().map(|s| InflateScratch {
            state: &mut *s.state,
            dictionary: &mut *s.dictionary,
            input: &mut *s.input,
        }),
        on_yield,
        workflow.policy,
    ) {
        Ok(chapter) => chapter,
        Err(e) => return Err(e),
    };
    let raw = Checked::raw(entry.uncompressed_size);
    let mut eof = false;
    let mut status = {
        let mut stream =
            crate::xml_session::XmlStream::with_policy(&mut chapter, None, workflow.backend, crate::xml_session::OptionalPolicy(workflow.policy));
        let mut status = stream.open(
            &raw,
            crate::xml_session::Resources {
                buffer: workflow.buffer.as_deref_mut(),
                filter: workflow.filter.as_deref_mut(),
            },
            true,
        );
        while status == Status::Ok && !eof {
            status = stream.feed_chunk(&mut handler, Some(&mut eof));
            if status == Status::Ok && !eof {
                if let Some(f) = on_yield {
                    f.yield_now();
                }
            }
        }
        status
    }; // XML closes before chapter.finish and before decoder release.
    if status == Status::Ok && (!eof || !chapter.finish()) {
        status = Status::Truncated;
    }
    let chars = if status == Status::Ok {
        handler.0.finish()
    } else {
        None
    };
    if status == Status::Ok && chars.is_none() {
        status = Status::Unsupported;
    }
    drop(chapter);
    if status == Status::Ok { Ok(chars.expect("successful counter finish")) } else { Err(status) }
}

impl<S: BookSource + ?Sized, B: ParserBackend + ?Sized> Book<'_, '_, S, B> {
    /// Parser oracle; indexed readers share the same member counter above.
    pub fn semantic_counts(mut self, expected_spines: u32, source_bytes: u64,
        digest: &crate::book_semantic_index::Digest, index: &mut crate::book_semantic_index::Index,
        on_yield: crate::admission::Yield<'_>,
    ) -> Status {
        use crate::book_semantic_index::POLICY_VERSION;
        if self.result.spine_count != expected_spines as usize { return Status::Stale; }
        if !index.bind(digest, POLICY_VERSION, expected_spines) { return Status::Unsupported; }
        for spine in 0..expected_spines {
            let Some(href) = self.spine_item(spine as usize).and_then(|item| item.href)
                .and_then(|href| self.arena.c_string(href)) else { return Status::NotFound; };
            let Some((_, entry)) = self.catalog().find(href) else { return Status::NotFound; };
            if !crate::phased_metadata::admissible_href(href, 512) { return Status::Unsupported; }
            let entry = match self.check(&entry) { Ok(entry) => entry, Err(error) => return error };
            let chars = match semantic_count_member(&mut self.workflow, &entry, on_yield) {
                Ok(chars) => chars, Err(error) => return error,
            };
            let status = semantic_validate(self.workflow.source, source_bytes);
            if status != Status::Ok { return status; }
            if !index.record(digest, POLICY_VERSION, spine, chars) { return Status::Stale; }
        }
        Status::Ok
    }
}

pub fn semantic_validate<S: BookSource + ?Sized>(source: &mut S, bytes: u64) -> Status {
    if source.size() != bytes { Status::Stale }
    else if source.read_at(0, &mut []) != 0 { Status::IoError }
    else { Status::Ok }
}
