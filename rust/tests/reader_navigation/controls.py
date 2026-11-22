"""Direct, type-preserving edits; witnesses are names in the existing N1 corpus.

These are proposed controls, not observed kills. Admission is in prepare.py.
"""
# name, source, unique old text, replacement, bounded existing witnesses
CONTROLS = [
    ('weights-member-admission', 'lib.rs', 'valid = false;\n                continue;',
     'valid = true;\n                continue;', ('missing-text', 'bad-method', 'too-large')),
    ('duplicate-href', 'lib.rs', 'if found != NO_SPINE\n            || entry.is_none_or',
     'if entry.is_none_or', ('duplicate-spine',)),
    ('utf8-title-boundary', 'lib.rs', '&& at < TITLE_BYTES\n    {',
     '&& at < TITLE_BYTES - 2\n    {', ('long-invalid-title',)),
    ('full-declaration-scan', 'lib.rs', '(0..book.toc_count()).any(|i| {',
     '(0..selected).any(|i| {', ('declared-conflict-unobserved',)),
    ('full-id-collision', 'lib.rs', '&& hash_path(fragment) == hash_path(peer))',
     '&& hash_path(fragment) != hash_path(peer))', ('full-id-collision',)),
    ('record-overflow', 'lib.rs', 'if usize::from(self.count) == MAX_TARGETS {\n            self.bind_issues |= RECORD_OVERFLOW;',
     'if usize::from(self.count) == MAX_TARGETS {\n            self.bind_issues |= STRING_OVERFLOW;', ('record-pressure-65535',)),
    ('string-overflow', 'lib.rs', 'self.bind_issues |= STRING_OVERFLOW;\n            return NO_STRING;',
     'self.bind_issues |= RECORD_OVERFLOW;\n            return NO_STRING;', ('string-pressure',)),
    ('required-toc-reservation', 'lib.rs', 'usize::from(self.count) + 1 >= MAX_TARGETS',
     'usize::from(self.count) + 1 > MAX_TARGETS', ('record-pressure-12',)),
    ('inherited-unordered', 'lib.rs', '&& self.bind_issues & UNORDERED == 0',
     '&& self.bind_issues & UNORDERED != 0', ('unordered', 'selected-2')),
    ('stop-unresolved', 'lib.rs', 'if !target || !anchor {\n            return false;',
     'if !target || !anchor {\n            return true;', ('observations-15',)),
    ('depth-tie', 'lib.rs', '(start == self.start && depth > self.depth)',
     '(start == self.start && depth < self.depth)', ('page-0',)),
    ('anchor-idempotence', 'lib.rs', 'if target.flags & RESOLVED != 0 && target.char_start != start {',
     'if target.flags & RESOLVED != 0 && target.char_start == start {', ('observations-17',)),
    ('menu-order', 'menu.rs', 'mapping.iter().enumerate() {',
     'mapping.iter().enumerate().rev() {', ('selected-2', 'unordered')),
    ('menu-focus', 'menu.rs', 'focus.saturating_sub(3).min(count - 5)',
     'focus.saturating_sub(2).min(count - 5)', ('menu-window-4',)),
    ('menu-reuse', 'menu.rs', 'if info.first != window_first(focus, info.count) {',
     'if info.first == window_first(focus, info.count) {', ('independent-resolved',)),
    ('staged-state', 'lib.rs', 'let required_toc = self.staged_required_toc();',
     'let required_toc = self.staged_required_toc().saturating_sub(1);', ('record-pressure-12',)),
]
