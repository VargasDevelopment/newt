// MIT. Included inside canonical.rs; complete-table preparation only.
struct SemanticHandler(crate::book_semantic_index::Counter);
impl Handler for SemanticHandler {
    fn start_element(&mut self, name: &[u8], attributes: &mut dyn Attributes) {
        let hidden = crate::css::parse_inline_style(attributes.local(b"style")).display_none == 1;
        let image = attributes.local(b"src").is_some() || attributes.local(b"href").is_some();
        self.0.start_element(name, hidden, image);
    }
    fn end_element(&mut self, name: &[u8]) {
        self.0.end_element(name);
    }
    fn text(&mut self, text: &[u8]) {
        self.0.text(text);
    }
}

impl<S: BookSource + ?Sized, B: ParserBackend + ?Sized> Book<'_, '_, S, B> {
    /// Consumes the Book: no metadata/XML/source loan escapes this operation.
    /// Caller invalidates the table on EVERY error, including final validation.
    pub fn semantic_counts(
        mut self,
        expected_spines: u32,
        source_bytes: u64,
        digest: &crate::book_semantic_index::Digest,
        index: &mut crate::book_semantic_index::Index,
        on_yield: crate::admission::Yield<'_>,
    ) -> Status {
        use crate::book_semantic_index::POLICY_VERSION;
        if self.result.spine_count != expected_spines as usize {
            return Status::Stale;
        }
        if !index.bind(digest, POLICY_VERSION, expected_spines) {
            return Status::Unsupported;
        }
        for spine in 0..expected_spines {
            let Some(href) = self
                .spine_item(spine as usize)
                .and_then(|m| m.href)
                .and_then(|h| self.arena.c_string(h))
            else {
                return Status::NotFound;
            };
            let Some((_, entry)) = self.catalog().find(href) else {
                return Status::NotFound;
            };
            // Same bound/UTF-8/path policy as the actual reader selection.
            if !crate::phased_metadata::admissible_href(href, 512) {
                return Status::Unsupported;
            }
            let mut handler = SemanticHandler(Default::default());
            let mut chapter = match crate::admission::ChapterSource::open(
                self.workflow.source,
                &entry,
                self.workflow.decoder.as_mut().map(|s| InflateScratch {
                    state: &mut *s.state,
                    dictionary: &mut *s.dictionary,
                    input: &mut *s.input,
                }),
                on_yield,
                self.workflow.policy,
            ) {
                Ok(chapter) => chapter,
                Err(e) => return e,
            };
            let raw = Entry::raw(entry.uncompressed_size);
            let mut eof = false;
            let mut status = {
                let mut stream =
                    crate::xml_session::XmlStream::with_policy(&mut chapter, None, self.workflow.backend, crate::xml_session::OptionalPolicy(self.workflow.policy));
                let mut status = stream.open(
                    &raw,
                    crate::xml_session::Resources {
                        buffer: self.workflow.buffer.as_deref_mut(),
                        filter: self.workflow.filter.as_deref_mut(),
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
            if status == Status::Ok {
                status = semantic_validate(self.workflow.source, source_bytes);
            }
            if status != Status::Ok {
                return status;
            }
            if !index.record(
                digest,
                POLICY_VERSION,
                spine,
                chars.expect("successful counter finish"),
            ) {
                return Status::Stale;
            }
        }
        Status::Ok
    }
}

pub fn semantic_validate<S: BookSource + ?Sized>(source: &mut S, bytes: u64) -> Status {
    if source.size() != bytes {
        Status::Stale
    } else if source.read_at(0, &mut []) != 0 {
        Status::IoError
    } else {
        Status::Ok
    }
}
