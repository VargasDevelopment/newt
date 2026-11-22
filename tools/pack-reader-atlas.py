#!/usr/bin/env python3
"""Generate flat or losslessly packed reader coverage and shared frozen classes.

No glyph, coverage level, metric or offset changes. --flat restores direct
frozen tables. --check verifies the representation selected by provenance;
--share-classes aliases the equal italic kerning-class arrays.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import zlib

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / 'rust/reader-atlas'
SOURCE = CRATE / 'source/reader_font_data.inc.zlib'
ORIGINAL_SHA = 'd06c02a0d9fddb7c3ea78ef4e1fa15d03538c3e9c54185f87491567a9bd022bc'


def read_source_bytes():
    raw = zlib.decompress(SOURCE.read_bytes())
    if hashlib.sha256(raw).hexdigest() != ORIGINAL_SHA:
        raise ValueError('pinned original reader coverage changed')
    return raw


def rows(values, count=16):
    return '\n'.join('    ' + ', '.join(map(str, values[at:at + count])) + ','
                     for at in range(0, len(values), count))


def share_classes(text):
    # Equal frozen classes share a symbol, without changing a lookup or descriptor.
    canonical, alias = 'kAddedItalicClasses', 'kAddedBoldItalicClasses'
    source = read_source_bytes().decode()
    values = []
    for name in (canonical, alias):
        match = re.search(r'static constexpr AddedKernClasses ' + name + r'\[\] = \{(.*?)\};', source, re.S)
        if match is None:
            raise ValueError('frozen kerning classes missing: ' + name)
        values.append(re.findall(r'\{\s*(\d+),\s*(\d+)\s*\}', match[1]))
    if len(values[0]) != 2041 or values[0] != values[1]:
        raise ValueError('frozen kerning classes differ')
    pattern = r'pub\(super\) static (NAME): \[AddedKernClasses; 2041\] = \[(.*?)\];'
    first = re.search(pattern.replace('(NAME)', canonical), text, re.S)
    if first is None or re.findall(r'left: (\d+), right: (\d+)', first[1]) != values[0]:
        raise ValueError('Rust canonical classes differ from frozen values')
    declaration = 'pub(super) use self::' + canonical + ' as ' + alias + ';'
    duplicate = re.search(pattern.replace('(NAME)', alias), text, re.S)
    if duplicate is not None:
        if re.findall(r'left: (\d+), right: (\d+)', duplicate[1]) != values[0]:
            raise ValueError('Rust duplicate classes differ from frozen values')
        text = text[:duplicate.start()] + declaration + text[duplicate.end():]
    elif text.count(declaration) != 1:
        raise ValueError('one frozen class alias required')
    return text, dict(canonical=canonical, alias=alias, elements=2041, saved_bytes=8164)


def generate(text, flat=False):
    raw = read_source_bytes()
    bitmaps = []
    logical = {table['cpp']: table['count'] for table in json.loads((CRATE / 'provenance.json').read_text())['arrays']}
    for match in re.finditer(r'static constexpr std::uint8_t (\w*Bitmap)\[\] = \{(.*?)\};', raw.decode(), re.S):
        tokens = match[2].replace(',', ' ').split()
        if any(not re.fullmatch(r'0x[0-9a-fA-F]+|\d+', token) for token in tokens):
            raise ValueError('nonliteral coverage byte: ' + match[1])
        values = bytes(int(token, 0) for token in tokens)
        if len(values) != logical[match[1]]:
            raise ValueError('original logical coverage extent differs: ' + match[1])
        bitmaps.append((match[1], values))
    if len(bitmaps) != 9 or sum(len(value) for _, value in bitmaps) != 637493:
        raise ValueError('expected all nine original coverage tables')
    blocks, ids, tables = [], {}, []
    for name, original in bitmaps:
        text, removed = re.subn(rf'pub\(super\) static {name}Blocks: \[u16; \d+\] = \[.*?\];\n',
                                '', text, flags=re.S)
        if removed > 1:
            raise ValueError('duplicate bitmap index table: ' + name)
        indices = []
        for at in range(0, len(original), 4):
            block = original[at:at + 4].ljust(4, b'\0')
            if block not in ids:
                ids[block] = len(blocks)
                blocks.append(block)
            indices.append(ids[block])
        if len(blocks) > 65536:
            raise ValueError('dictionary exceeds u16 indices')
        decoded = b''.join(blocks[index] for index in indices)[:len(original)]
        if decoded != original:
            raise ValueError('coverage byte reconstruction failed: ' + name)
        declaration = (f'pub(super) static {name}Blocks: [u16; {len(indices)}] = [\n'
                       + rows(indices) + f'\n];\npub(super) static {name}: Bitmap = '
                       + f'Bitmap::new(&{name}Blocks, {len(original)});')
        if flat:
            declaration = f'pub(super) static {name}: [u8; {len(original)}] = [\n' + rows(original) + '\n];'
        pattern = (rf'pub\(super\) static {name}: (?:\[u8; \d+\] = \[.*?\];'
                   r'|Bitmap = Bitmap::new\(&\[.*?\], \d+\);'
                   + rf'|Bitmap = Bitmap::new\(&{name}Blocks, \d+\);)')
        text, replaced = re.subn(pattern, lambda _: declaration, text, flags=re.S)
        if replaced != 1:
            raise ValueError('one original or encoded declaration required: ' + name)
        tables.append(dict(name=name, original_bytes=len(original), indices=len(indices),
                           original_sha256=hashlib.sha256(original).hexdigest()))
    dictionary = b''.join(blocks)
    text = re.sub(r'\n// Lossless reader coverage dictionary\.\n'
                  r'pub\(super\) static BITMAP_BLOCKS: \[u8; \d+\] = \[.*?\];\n?', '', text, flags=re.S)
    text = text.rstrip() + '\n' if flat else text.rstrip() + ('\n\n// Lossless reader coverage dictionary.\n'
                           f'pub(super) static BITMAP_BLOCKS: [u8; {len(dictionary)}] = [\n'
                           + rows(dictionary) + '\n];\n')
    info = dict(encoding='four-byte dictionary and u16 indices; original two-bit coverage unchanged',
                original_source_sha256=ORIGINAL_SHA, original_bytes=sum(len(value) for _, value in bitmaps),
                dictionary_bytes=len(dictionary), index_bytes=2 * sum(table['indices'] for table in tables),
                rv32_descriptor_bytes=12 * len(tables), tables=tables)
    return text, info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--flat', action='store_true', help='restore the original direct coverage tables')
    parser.add_argument('--share-classes', action='store_true', help='alias identical frozen italic kerning classes')
    args = parser.parse_args()
    path = CRATE / 'src/data.rs'
    before = path.read_text()
    provenance = json.loads((CRATE / 'provenance.json').read_text())
    flat = args.flat or 'bitmap_storage' not in provenance
    after, info = generate(before, flat)
    sharing = None
    if args.share_classes or provenance.get('shared_classes'):
        after, sharing = share_classes(after)
    flat_bytes = sum(table['flash_bytes'] for table in provenance['arrays']) - (sharing['saved_bytes'] if sharing else 0)
    storage_bytes = flat_bytes if flat else (flat_bytes - info['original_bytes']
        + info['dictionary_bytes'] + info['index_bytes'] + info['rv32_descriptor_bytes'])
    if args.check:
        if after != before or not flat and provenance.get('bitmap_storage') != info:
            raise ValueError('reader bitmap representation is not the deterministic lossless encoding')
        if provenance['data_rs_sha256'] != hashlib.sha256(after.encode()).hexdigest():
            raise ValueError('reader bitmap source hash differs from provenance')
        if provenance.get('shared_classes') != sharing or provenance['flash_table_bytes'] != storage_bytes:
            raise ValueError('reader table storage differs from provenance')
    else:
        provenance.setdefault('unpacked_data_rs_sha256', provenance['data_rs_sha256'])
        provenance.setdefault('unpacked_flash_table_bytes', provenance['flash_table_bytes'])
        provenance['bitmap_storage'] = info
        provenance['flash_table_bytes'] = (provenance['unpacked_flash_table_bytes'] - info['original_bytes']
            + info['dictionary_bytes'] + info['index_bytes'] + info['rv32_descriptor_bytes'])
        provenance['tables_note'] = 'Logical original table values; bitmap_storage describes their lossless encoded placement.'
        provenance['data_rs_sha256'] = hashlib.sha256(after.encode()).hexdigest()
        if flat:
            provenance['flash_table_bytes'] = provenance.pop('unpacked_flash_table_bytes')
            for key in ('bitmap_storage', 'tables_note', 'unpacked_data_rs_sha256'):
                provenance.pop(key, None)
        if sharing:
            provenance['shared_classes'] = sharing
        provenance['flash_table_bytes'] = storage_bytes
        if not flat:
            provenance['unpacked_flash_table_bytes'] = flat_bytes
        path.write_text(after)
        (CRATE / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print('PASS: every original coverage byte reconstructed;', info['original_bytes'], 'original bytes,',
          info['original_bytes'] if flat else info['dictionary_bytes'] + info['index_bytes'] + info['rv32_descriptor_bytes'], 'RV32 storage bytes')


if __name__ == '__main__':
    main()
