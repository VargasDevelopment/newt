#!/usr/bin/env python3
"""Native Latin-core bitmaps for the reader's other text sizes.

The 30 px atlas stays the authority for every metric (advance, kerning, ascent,
line height). This tool adds only the drawn shapes of its 341 Latin-core
glyphs per face at each other reading size, so text at those sizes is hinted
and rasterized natively instead of resampled. Shapes are stored 1-bit: the
draw paths ink only 2-bit coverage levels 2 and 3, so the low bit is dropped
at generation instead of in flash.

--font-source-dir DIR  verify the pinned TTFs, prove that this pipeline
                       reproduces the pinned 30 px bitmaps byte for byte, then
                       write src/native.rs, native/*.bin and the provenance
                       record. Needs system FreeType at the pinned version.
--check                stdlib only: the checked-in outputs match provenance.
"""
import argparse
import ctypes as C
import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FONTS = ROOT / 'assets/fonts'
CRATE = ROOT / 'rust/reader-atlas'
OUTPUT = CRATE / 'src/native.rs'
PROVENANCE = CRATE / 'provenance.json'
SIZES = (22, 24, 27, 34)
PROFILE_SIZE = 30
STYLES = (('Regular', 'NewtReadingAtlas'), ('Bold', 'NewtReadingAtlasBold'),
          ('Italic', 'NewtReadingAtlasItalic'), ('BoldItalic', 'NewtReadingAtlasBoldItalic'))


def blob(px):
    return CRATE / f'native/{px}px.bin'


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def pinned_atlas(name):
    """The 341 Latin-core codepoints and their pinned 30 px glyphs, in atlas order."""
    data = json.loads((FONTS / f'{name}.json').read_text())
    codepoints = list(range(data['first'], data['last'] + 1)) + data['extra_codepoints']
    glyphs = []
    for table, records in (('bitmap_hex', data['glyphs']), ('extra_bitmap_hex', data['extra_glyphs'])):
        pixels = bytes.fromhex(data[table])
        for width, height, _advance, left, top, offset in records:
            glyphs.append(((width, height, left, top), pixels[offset:offset + (width + 3) // 4 * height]))
    return codepoints, glyphs


def proportional_figures(path):
    """GSUB 'pnum' single substitutions, as the 30 px atlas used them."""
    raw = Path(path).read_bytes()
    read = lambda at, fmt: struct.unpack_from('>' + fmt, raw, at)
    tables = {read(12 + 16 * i, '4s')[0]: read(12 + 16 * i + 8, 'I')[0] for i in range(read(4, 'H')[0])}
    gsub = tables.get(b'GSUB')
    if gsub is None:
        return {}
    features, lookups = (gsub + v for v in read(gsub + 6, 'HH'))
    selected = set()
    for i in range(read(features, 'H')[0]):
        tag, relative = read(features + 2 + 6 * i, '4sH')
        if tag == b'pnum':
            start = features + relative
            selected.update(read(start + 4, 'H' * read(start + 2, 'H')[0]))
    result = {}
    for index in sorted(selected):
        start = lookups + read(lookups + 2 + 2 * index, 'H')[0]
        kind, _flags, count = read(start, 'HHH')
        for relative in read(start + 6, 'H' * count):
            at = start + relative
            if kind == 7:
                _, kind, extension = read(at, 'HHI')
                at += extension
            if kind != 1:
                raise ValueError('unsupported pnum lookup type')
            fmt, coverage = read(at, 'HH')
            ckind, ccount = read(at + coverage, 'HH')
            if ckind == 1:
                covered = list(read(at + coverage + 4, 'H' * ccount))
            elif ckind == 2:
                covered = []
                for i in range(ccount):
                    first, last, _start = read(at + coverage + 4 + 6 * i, 'HHH')
                    covered.extend(range(first, last + 1))
            else:
                raise ValueError('unsupported coverage format')
            if fmt == 1:
                delta = read(at + 4, 'h')[0]
                result.update((g, (g + delta) & 0xffff) for g in covered)
            elif fmt == 2:
                result.update(zip(covered, read(at + 6, 'H' * read(at + 4, 'H')[0])))
            else:
                raise ValueError('unsupported pnum format')
    return result


class Bitmap(C.Structure):
    _fields_ = [('rows', C.c_uint), ('width', C.c_uint), ('pitch', C.c_int),
                ('buffer', C.POINTER(C.c_ubyte)), ('num_grays', C.c_ushort),
                ('pixel_mode', C.c_ubyte), ('palette_mode', C.c_ubyte), ('palette', C.c_void_p)]


# Public FreeType ABI prefixes only (FT_GlyphSlotRec up to bitmap_top, FT_FaceRec up to glyph).
class Slot(C.Structure):
    _fields_ = [('library', C.c_void_p), ('face', C.c_void_p), ('next', C.c_void_p),
                ('glyph_index', C.c_uint), ('generic', C.c_void_p * 2), ('metrics', C.c_long * 8),
                ('linear', C.c_long * 2), ('advance', C.c_long * 2), ('format', C.c_uint),
                ('bitmap', Bitmap), ('bitmap_left', C.c_int), ('bitmap_top', C.c_int)]


class Face(C.Structure):
    _fields_ = [('counts', C.c_long * 5), ('names', C.c_char_p * 2), ('fixed', C.c_int),
                ('sizes', C.c_void_p), ('charmaps_count', C.c_int), ('charmaps', C.c_void_p),
                ('generic', C.c_void_p * 2), ('bbox', C.c_long * 4), ('units', C.c_ushort),
                ('metrics', C.c_short * 7), ('glyph', C.POINTER(Slot))]


FaceHandle = C.POINTER(Face)


class FreeType:
    def __init__(self, profile):
        self.profile = profile
        self.ft = C.CDLL('libfreetype.so.6')
        self.ft.FT_New_Face.argtypes = [C.c_void_p, C.c_char_p, C.c_long, C.POINTER(C.c_void_p)]
        self.ft.FT_Set_Char_Size.argtypes = [FaceHandle, C.c_long, C.c_long, C.c_uint, C.c_uint]
        self.ft.FT_Get_Char_Index.argtypes = [FaceHandle, C.c_ulong]
        self.ft.FT_Load_Glyph.argtypes = [FaceHandle, C.c_uint, C.c_int32]
        self.library = C.c_void_p()
        self.check(self.ft.FT_Init_FreeType(C.byref(self.library)))
        version = [C.c_int() for _ in range(3)]
        self.ft.FT_Library_Version(self.library, *map(C.byref, version))
        if '.'.join(str(v.value) for v in version) != profile['version']:
            raise ValueError('FreeType must be the pinned version ' + profile['version'])

    @staticmethod
    def check(code):
        if code:
            raise ValueError(f'FreeType error {code}')

    def render(self, path, px, codepoints):
        """Points stay pinned; DPI scales with px, so 30 px is exactly the pinned 150 DPI."""
        # A plain handle, cast once: procedure_closure_test must place every call.
        handle = C.c_void_p()
        self.check(self.ft.FT_New_Face(self.library, str(path).encode(), 0, C.byref(handle)))
        face = C.cast(handle, FaceHandle)
        points, dpi = self.profile['point_size'] * 64, self.profile['dpi'] * px // PROFILE_SIZE
        self.check(self.ft.FT_Set_Char_Size(face, points, points, dpi, dpi))
        figures, cutoffs = proportional_figures(path), self.profile['coverage_thresholds_8bit']
        glyphs = []
        for cp in codepoints:
            index = self.ft.FT_Get_Char_Index(face, cp)
            if not index:
                raise ValueError(f'pinned source lacks U+{cp:04X}')
            self.check(self.ft.FT_Load_Glyph(face, figures.get(index, index), self.profile['load_flags']))
            slot = face.contents.glyph.contents
            bitmap, stride = slot.bitmap, (slot.bitmap.width + 3) // 4
            if bitmap.pixel_mode != 2 or bitmap.width > 255 or bitmap.rows > 255:
                raise ValueError('unexpected FreeType bitmap')
            pixels = bytearray(stride * bitmap.rows)
            for y in range(bitmap.rows):
                for x in range(bitmap.width):
                    grey = bitmap.buffer[y * bitmap.pitch + x]
                    pixels[y * stride + x // 4] |= sum(grey >= c for c in cutoffs) << (6 - 2 * (x % 4))
            glyphs.append(((bitmap.width, bitmap.rows, slot.bitmap_left, slot.bitmap_top), bytes(pixels)))
        self.check(self.ft.FT_Done_Face(face))
        return glyphs


def packed(width, height, shape):
    """Row-aligned 1-bit rows, first pixel in the high bit: ink where coverage >= 2."""
    stride, source = (width + 7) // 8, (width + 3) // 4
    out = bytearray(stride * height)
    for y in range(height):
        for x in range(width):
            if (shape[y * source + x // 4] >> (6 - 2 * (x % 4))) & 3 >= 2:
                out[y * stride + x // 8] |= 0x80 >> (x % 8)
    return bytes(out)


def generate(font_dir):
    sources = json.loads((FONTS / 'sources.json').read_text())
    pins = {entry['id']: entry for entry in sources['sources']}
    lexend = sources['fallback_order'][0]['faces']
    paths = {}
    for style, _ in STYLES:
        entry = pins[lexend[style]]
        path = font_dir / entry['path']
        if sha256(path.read_bytes()) != entry['sha256']:
            raise ValueError('pinned source mismatch: ' + entry['path'])
        paths[style] = path
    freetype = FreeType(sources['rasterizer'])
    for style, name in STYLES:
        codepoints, pinned = pinned_atlas(name)
        if freetype.render(paths[style], PROFILE_SIZE, codepoints) != pinned:
            raise ValueError(f'{style}: pipeline does not reproduce the pinned 30 px atlas')
    rust = ['// Generated by tools/make-native-reader-sizes.py; do not edit. Font data OFL-1.1.',
            '// Native 1-bit Latin-core shapes per reading size; metrics stay in the 30 px atlas.',
            'use super::{NativeFace, NativeGlyph, NativeSize};',
            'const fn g(width: u8, height: u8, left: i8, top: i8, offset: u16) -> NativeGlyph {',
            '    NativeGlyph { width, height, left, top, offset }', '}']
    entries, outputs = [], {}
    for px in SIZES:
        pixels, faces = bytearray(), []
        for style, name in STYLES:
            records, base = [], len(pixels)
            for (width, height, left, top), shape in freetype.render(paths[style], px, pinned_atlas(name)[0]):
                if len(pixels) - base > 0xffff:
                    raise ValueError(f'{px} px {style}: face offsets exceed u16')
                records.append(f'g({width},{height},{left},{top},{len(pixels) - base})')
                pixels.extend(packed(width, height, shape))
            faces += ([f'        NativeFace {{ base: {base}, glyphs: ['] + ['            ' + ', '.join(records[at:at + 8]) + ','
                                         for at in range(0, len(records), 8)] + ['        ] },'])
        blob(px).parent.mkdir(exist_ok=True)
        blob(px).write_bytes(pixels)
        outputs[f'{px}px.bin'] = dict(bytes=len(pixels), sha256=sha256(pixels))
        entries += [f'    NativeSize {{ px: {px}, bitmap: &BITMAP_{px}PX, faces: ['] + faces + ['    ] },']
        # Named statics, not anonymous include_bytes! constants: the linked
        # image's data guards attribute every font byte to a reviewed object.
        rust.append(f'static BITMAP_{px}PX: [u8; {len(pixels)}] = *include_bytes!("../native/{px}px.bin");')
    text = '\n'.join(rust + [f'pub(super) static NATIVE: [NativeSize; {len(SIZES)}] = ['] + entries + ['];']) + '\n'
    OUTPUT.write_text(text)
    provenance = json.loads(PROVENANCE.read_text())
    provenance['native_sizes'] = dict(
        generator='tools/make-native-reader-sizes.py', sizes=list(SIZES), glyphs_per_face=341,
        render='pinned rasterizer profile at 14 pt; DPI = 150 * px / 30; 30 px reproduced byte for byte',
        storage='1-bit rows, (width + 7) / 8 bytes each, high bit first; ink where 2-bit coverage >= 2',
        sources={style: lexend[style] for style, _ in STYLES}, native_rs_sha256=sha256(text.encode()),
        bitmaps=outputs)
    PROVENANCE.write_text(json.dumps(provenance, indent=2) + '\n')


def check():
    record = json.loads(PROVENANCE.read_text())['native_sizes']
    if record['sizes'] != list(SIZES) or sha256(OUTPUT.read_bytes()) != record['native_rs_sha256']:
        raise ValueError('src/native.rs differs from provenance')
    for px in SIZES:
        if sha256(blob(px).read_bytes()) != record['bitmaps'][f'{px}px.bin']['sha256']:
            raise ValueError(f'native/{px}px.bin differs from provenance')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--font-source-dir', type=Path)
    mode.add_argument('--check', action='store_true')
    args = parser.parse_args()
    if args.check:
        check()
    else:
        generate(args.font_source_dir)
    print('PASS: native reader sizes', ', '.join(f'{px} px' for px in SIZES))


if __name__ == '__main__':
    main()
