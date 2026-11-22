"""Proposed executable behavioral controls; compilation and witnesses pending."""
EXT='core/src/external_progress_store.rs'
READBACK='''        let result = self.owner.store.read_blob(key, output);
        if result == BlobRead::Failed {
            self.owner.status = Status::StorageFailure;
        }
        if result != BlobRead::Present(expected.len())'''
CONTROLS=(
 ('external-full-digest',EXT,'observed_digest != self.owner.digest','observed_digest[..7] != self.owner.digest[..7]'),
 ('external-missing-digest-slots',EXT,'if any_present {','if false && any_present {'),
 ('external-inactive-book',EXT,'if decoded.book != self.owner.book {','if false && decoded.book != self.owner.book {'),
 ('external-admission',EXT,'if !readonly && self.admission.as_mut().is_some_and(|gate| !gate.enter()) {','if false && !readonly && self.admission.as_mut().is_some_and(|gate| !gate.enter()) {'),
 ('external-ambiguity',EXT,'if select(slot(0), slot(1)) == Selection::Ambiguous {','if false && select(slot(0), slot(1)) == Selection::Ambiguous {'),
 ('external-verify-precedence',EXT,'self.owner.status = Status::VerifyFailure;','self.owner.status = Status::StorageFailure;'),
 ('external-physical-readback',EXT,READBACK,READBACK.replace('let result = self.owner.store.read_blob(key, output);','let _physical = self.owner.store.read_blob(key, output);\n        let result = BlobRead::Missing;')),
 ('external-bounded-retry','core/src/durable.rs','for _ in 0..2 {','for _ in 0..1 {'),
 ('external-durable-reconcile','core/src/progress_store.rs','&& self.backend.readback_durable()','&& !self.backend.readback_durable()'),
 ('external-strict-schema','core/src/progress_store.rs','strict || protected.is_some()','false && (strict || protected.is_some())'),
 ('external-protected-peer','core/src/progress_store.rs','plan.target_slot = 1 - kept;','plan.target_slot = kept;'),
 ('external-early-close',EXT,'        self.end();\n        self.owner.status = status;','        self.owner.status = status;'),
 ('external-ambiguous-result',EXT,'if self.status == Status::Ambiguous {\n                    Load::Ambiguous','if self.status == Status::Ambiguous {\n                    Load::Failed'),
 ('external-checkpoint-collision',EXT,'Load::Ambiguous => Load::Ambiguous,','Load::Ambiguous => Load::Failed,'),
 ('external-wrong-book-verify-order',EXT,'if result != BlobRead::Present(expected.len()) || output != expected {',
  'if result == BlobRead::Present(BYTES) && let Some(value) = Progress::decode(output) && value.book != self.owner.book { self.owner.status = Status::IdentityMismatch; return BlobRead::Failed; }\n        if result != BlobRead::Present(expected.len()) || output != expected {'),

)
