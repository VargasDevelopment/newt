"""Unique, type-preserving SOURCE proposals. Every kill needs a clean EA1 witness."""
def proposals():
    return [
        ('local-rule','core/src/member_check.rs',') == ZipLocalHeaderRule::Mismatch',') == ZipLocalHeaderRule::Mismatch && false',['member_header_is_checked_when_it_opens']),
        ('names','core/src/member_check.rs','if got != expected {','if false {',['member_name_is_checked_when_it_opens']),
        ('flags-precedence','core/src/admission.rs','zip_central_record_rule(0, method, name, uncompressed)','zip_central_record_rule(flags, method, name, uncompressed)',['declared-budget-before-flags']),
        ('expansion-probe','core/src/admission.rs','- produced + 1).min(2048)','- produced).min(2048)',['required-excess-deflate']),
        ('duplicates','core/src/canonical.rs','if duplicate {\n            return Err(Status::Unsupported);','if duplicate && false {\n            return Err(Status::Unsupported);',['duplicate-before-required']),
        ('nav-selection','core/src/canonical.rs','Ok(_) | Err(Status::NotFound) => {}','Ok(member) => if self.workflow.pass(&member, &mut EncryptionScan { encrypted: false }) != Status::Ok { return Status::ParseError; },\n                Err(Status::NotFound) => {}',['valid-nav-not-parsed']),
        ('required-expansion','core/src/canonical.rs','find(b\"META-INF/container.xml\"), required,','find(b\"META-INF/container.xml\"), false,',['required-short-deflate']),
        ('reset','core/src/admission.rs','    catalog.reset();\n    parse.reset();\n    status','    // mutation: retain allocations\n    status',['valid-nav-not-parsed']),
        ('zero-yield','core/src/admission.rs','            if got == 0 {\n                return if produced','            if got == 0 {\n                if let Some(f) = on_yield { f.yield_now(); }\n                return if produced',['valid-nav-not-parsed']),
        ('finish-probe','core/src/admission.rs','self.consumed == self.length && self.reader.read(&mut extra) == 0','self.consumed == self.length && { let _: &mut [u8] = &mut extra; true }',['chapter-deflate-extra']),
    ]

def apply(root, proposal):
    _,relative,old,new,_ = proposal
    path = root / relative
    text = path.read_text()
    if text.count(old) != 1: raise RuntimeError('mutation anchor not unique: '+proposal[0])
    text=text.replace(old,new)
    path.write_text(text)
