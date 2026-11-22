"""Exact unique-site mutations, proposed only; compilation/kills NOT yet verified.

Each tuple follows rust/check.py's (name, relative, before, after) interface.
Use only after merging modules. Compiler failure never counts as a kill.
"""
U = 'core/src/utf8.rs'
Z = 'core/src/zip_record_rules.rs'
CONTROLS = (
    ('UTF8 failure advances one byte', U, '*offset += 1;', '*offset += 0;'),
    ('UTF8 failure preserves prior output', U, '*offset += 1;', '*offset += 1;\n    *codepoint = 0;'),
    ('UTF8 ASCII excludes continuations', U, 'first < 0x80', 'first <= 0x80'),
    ('UTF8 two-byte lower bound accepted', U, '0xc2..=0xdf => 1,', '0xc3..=0xdf => 1,'),
    ('UTF8 three-byte lower bound accepted', U, '0xe0..=0xef => 2,', '0xe1..=0xef => 2,'),
    ('UTF8 four-byte upper bound accepted', U, '0xf0..=0xf4 => 3,', '0xf0..=0xf3 => 3,'),
    ('UTF8 exact final width accepted', U, 'text.len() - start - 1 < extra', 'text.len() - start - 1 <= extra'),
    ('UTF8 continuation marker required', U, 'byte & 0xc0 != 0x80', 'byte & 0xc0 == 0x80'),
    ('UTF8 all continuation payload bits retained', U, 'u32::from(byte & 0x3f)', 'u32::from(byte & 0x1f)'),
    ('UTF8 three-byte overlong rejected', U, 'extra == 2 && value < 0x800', 'extra == 2 && value < 0x400'),
    ('UTF8 four-byte overlong rejected', U, 'extra == 3 && value < 0x10000', 'extra == 3 && value < 0x8000'),
    ('UTF8 above scalar maximum rejected', U, 'value > 0x10ffff', 'value > 0x11ffff'),
    ('UTF8 surrogate rejected', U, '(0xd800..=0xdfff).contains(&value)', 'false && (0xd800..=0xdfff).contains(&value)'),
    ('UTF8 success consumes full scalar', U, '*offset = start + extra + 1;', '*offset = start + extra;'),
    ('ZIP table bit 13 encryption', Z, 'Ok,\n        Encrypted,\n        Ok,\n        Ok,\n    ]',
     'Ok,\n        Ok,\n        Ok,\n        Ok,\n    ]'),
    ('ZIP table bit 5 unsupported', Z, '        Ok,\n        Unsupported,\n        Encrypted,', '        Unsupported,\n        Ok,\n        Encrypted,'),
    ('ZIP mask bit ordering', Z, 'mask |= 1u16 << bit;', 'mask |= 1u16 << (15 - bit);'),
    ('ZIP encryption wins over patch flag', Z, 'if flags & encrypted != 0 {', 'if flags & encrypted != 0 && flags & unsupported == 0 {'),
    ('ZIP central encryption wins over other rejection', Z, 'return flag_rule;', 'return ZipCentralRecordRule::Unsupported;'),
    ('ZIP stored method supported', Z, 'method != 0 && method != 8', 'method != 8 && method != 8'),
    ('ZIP raw deflate method supported', Z, 'method != 0 && method != 8', 'method != 0 && method != 0'),
    ('ZIP empty names rejected', Z, '|| name_bytes == 0', '|| name_bytes == u16::MAX'),
    ('ZIP maximum name is inclusive', Z, 'usize::from(name_bytes) > EPUB_MAX_MEMBER_NAME_BYTES', 'usize::from(name_bytes) >= EPUB_MAX_MEMBER_NAME_BYTES'),
    ('ZIP maximum output is inclusive', Z, 'uncompressed_bytes > EPUB_MAX_MEMBER_BYTES', 'uncompressed_bytes >= EPUB_MAX_MEMBER_BYTES'),
    ('ZIP total expanded limit exact', Z, '64 * 1024 * 1024;', '63 * 1024 * 1024;'),
    ('ZIP local signature exact', Z, 'signature == 0x04034b50', 'signature == 0x04034b51'),
    ('ZIP local flags equal including descriptor', Z, '&& flags == central_flags', '&& (flags & !8) == (central_flags & !8)'),
    ('ZIP local method equality', Z, '&& method == central_method', '&& (method == central_method || true)'),
    ('ZIP local name length equality', Z, '&& name_bytes == central_name_bytes', '&& (name_bytes == central_name_bytes || true)'),
)

CONTROL_FAMILIES = {'utf8.rs': ('utf8_next',), 'zip_record_rules.rs': ('zip_',)}
