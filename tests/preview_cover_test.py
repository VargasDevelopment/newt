#!/usr/bin/env python3
"""The firmware makes its own covers, end to end, on Books and Home.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes). Each world is a card with no Newt/Covers and
synthetic books whose covers are made here: baseline grayscale JPEGs declared
the EPUB3 way (cover-image), the EPUB2 way (<meta name="cover">), by the
guide (straight to the image, or to an XHTML cover page) and by a broken
cover meta that names no item (the image item is then found by the meta's
text as its href, by the id "cover" or by the file name "cover") or by a
first spine page that holds only the cover image, stored and
deflated, one with the common "image/jpg" media type, a progressive JPEG (the
firmware draws its first scan, at 1/8 scale), grey and RGB PNGs and a wide
PNG at the row cap; and books that must get no cover: one whose cover form
the firmware does not decode (the known gap), PNGs interlaced or past the
row cap, one whose cover page carries an internal DTD subset, and one whose
first spine page holds text beside its image. Cover work starts after
2 s without input; the preview presses
only after 4 s without a refresh, press or card access, so staying on a
screen lets that work finish. Each control is the same build with one
firmware change in its generated copy (never in rust/firmware).

These are host facts only. The preview's heap is fixed and its file metadata
is complete, so heap declines and the device's third full-book read stay
hardware checks.
"""
import functools
import os
from pathlib import Path
import re
import shutil
import struct
import sys
import tempfile
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, RIGHT = 0, 1, 3
SHELF = (CONFIRM,)
# rust/firmware/reader_cover_idle.rs CoverResult.
PRODUCED, CANCELLED, DECLINED, MISSING, UNSUPPORTED, INVALID, IO_ERROR = 2, 3, 4, 5, 6, 8, 9
ROWS = re.compile(r'COVER work=attempt result=(\d+)')
HOME_READY = 'HOME_COVER eligible=1 attempted=1 step=7 ready=1 painted=1'
# A cover record: NLC2 header (signature, book size, even mtime, route) and,
# for art, the 144x188 one-bit raster.
SIGNATURE = b'NLC2\x90\0\xbc\0'
WIDTH, HEIGHT, ROW_BYTES = 144, 188, 18
ART, NO_ART = 28 + ROW_BYTES * HEIGHT, 28
# Routes as the card holds them (little-endian rust/core/src/cover.rs NO_ART words).
CJB7, CJB6, CJB5, CJB3 = b'CJB7', b'CJB6', b'CJB5', b'CJB3'
MTIME = 1790812800
# Book name: how its cover is declared and stored (epub() forms), and its
# image (picture() kinds, a JPEG by default).
SUPPORTED = {
    'Alder': {'form': 'epub3'},
    'Birch': {'form': 'epub3', 'deflated': True},
    'Cedar': {'form': 'epub2'},
    'Dogwood': {'form': 'epub2', 'deflated': True},
    'Elm': {'form': 'epub3', 'media': 'image/jpg'},
    'Fir': {'form': 'epub3', 'deflated': True, 'frame': 0xc2},
    'Ginkgo': {'form': 'guide-image'},
    'Hazel': {'form': 'guide-page', 'deflated': True},
    'Ironwood': {'form': 'meta-href'},
    'Juniper': {'form': 'meta-cover-id'},
    'Kapok': {'form': 'meta-cover-file', 'deflated': True},
    'Larch': {'form': 'spine-page'},
    'Magnolia': {'form': 'epub3', 'image': 'png-grey'},
    # Adobe APP14 transform 0 RGB JPEGs (component IDs R/G/B, green/blue-heavy):
    # baseline and progressive interleaved DC first scan.
    'Maple': {'form': 'epub3', 'image': 'jpeg-rgb'},
    'Nutmeg': {'form': 'epub2', 'image': 'png-rgb', 'deflated': True},
    'Poplar': {'form': 'epub3', 'image': 'jpeg-rgb', 'deflated': True, 'frame': 0xc2},
    'Rowan': {'form': 'epub3', 'image': 'png-wide'},
    # YCbCr with Y sampled 1x2 and 4x1, past the decoder's native 11, 21 and 22.
    'Oak': {'form': 'epub2', 'sampling': 0x12},
    'Spruce': {'form': 'epub3', 'deflated': True, 'frame': 0xc2, 'sampling': 0x41},
}
# The PNG whose decode block is the largest a cover takes, larger than the
# planned peak that admits cover work.
WIDE = 'Rowan'
# Books that get a bare record: (form, the attempt's result).
NO_COVER = {
    # Text beside the image: a first page, not a cover page.
    'Sumac': ({'form': 'spine-text-page'}, MISSING),
    'Willow': ({'form': 'subset-page'}, UNSUPPORTED),
    'Teak': ({'form': 'epub3', 'image': 'png-too-wide'}, UNSUPPORTED),
    'Tupelo': ({'form': 'epub3', 'image': 'png-interlaced'}, UNSUPPORTED),
    # Non-interleaved progressive Adobe RGB (R-only first scan): refused, never red-only art.
    'Walnut': ({'form': 'epub3', 'image': 'jpeg-rgb-split', 'frame': 0xc2}, UNSUPPORTED),
    # Arithmetic coding (SOF9): the decoders take Huffman frames only.
    'Yew': ({'form': 'epub3', 'frame': 0xc9}, UNSUPPORTED),
}
# The known gap, last on the shelves.
GAP = 'Yew'
# Names sort into shelf order.
BOOKS = sorted([*SUPPORTED, *NO_COVER])
SAMPLE = ROOT / 'tests/fixtures/the-small-lantern.epub'


def segment(marker, body):
    return bytes([0xff, marker]) + (len(body) + 2).to_bytes(2, 'big') + body


def entropy(bits):
    """Scan bits padded with ones, a zero stuffed after each 0xff."""
    bits = bits + [1] * (-len(bits) % 8)
    data = bytearray()
    for at in range(0, len(bits), 8):
        data.append(int(''.join(map(str, bits[at:at + 8])), 2))
        if data[-1] == 0xff:
            data.append(0)
    return bytes(data)


def encode_units(units, progressive):
    """Huffman-encode `(component, dc_value)` blocks with all-zero AC."""
    bits, previous = [], [0, 0, 0]
    for component, value in units:
        value >>= progressive
        diff, previous[component] = value - previous[component], value
        category = abs(diff).bit_length()
        for val, length in ((category, 4), (diff + (1 << category) - 1 if diff < 0 else diff, category)):
            bits += [val >> i & 1 for i in reversed(range(length))]
        if not progressive:
            bits += [0, 0]
    return entropy(bits)


def jpeg(width, height, dc, frame=0xc0, sampling=None):
    """A grayscale JPEG of flat 8x8 blocks: quantizers 1, DC category k coded
    as the 4-bit k, every AC an end of block (rust/core/tests/cover_tolerance.rs
    builds the same). `dc(x, y)` is block (x, y)'s DC value; `frame` is its
    SOF marker (0xc0 baseline). A progressive frame (0xc2) holds the DC first
    scan at point transform 1, as encoders write it, then one AC scan. With a
    Y `sampling` it is YCbCr, its chroma flat; the AC scan is Y's alone."""
    progressive = frame == 0xc2
    h, v = (sampling >> 4, sampling & 15) if sampling else (1, 1)
    components = [(1, sampling or 0x11)] + ([(2, 0x11), (3, 0x11)] if sampling else [])
    out = b'\xff\xd8' + segment(0xdb, bytes(1) + bytes([1] * 64))
    out += segment(frame, bytes([8]) + struct.pack('>HHB', height, width, len(components))
                   + b''.join(bytes([id, factors, 0]) for id, factors in components))
    dc_counts, ac_counts = [0] * 16, [0] * 16
    dc_counts[3], ac_counts[1] = 12, 1
    # Chroma (tables 1) codes the same way as Y (tables 0).
    for table in range(len(components) // 2 + 1):
        out += segment(0xc4, bytes([table, *dc_counts, *range(12), 0x10 | table, *ac_counts, 0]))
    scan = bytes([len(components)]) + b''.join(bytes([id, 0x00 if id == 1 else 0x11]) for id, _ in components)
    out += segment(0xda, scan + bytes([0, 0, 1] if progressive else [0, 63, 0]))
    columns, rows = -(-width // 8), -(-height // 8)
    # Each coded block as (component, DC) in scan order; MCU padding past the image is 0.
    units = []
    for row in range(-(-rows // v)):
        for column in range(-(-columns // h)):
            units += [(0, dc(x, y) if x < columns and y < rows else 0)
                      for y in range(row * v, row * v + v) for x in range(column * h, column * h + h)]
            units += [(1, 0), (2, 0)] if sampling else []
    out += encode_units(units, progressive)
    if progressive:
        # Every block's AC band ends at once.
        out += segment(0xda, bytes([1, 1, 0x00, 1, 63, 0])) + entropy([0, 0] * (columns * rows))
    return out + b'\xff\xd9'


def grey(seed, x, y):
    """The grey of art(seed)'s 8x8 block (x, y): diagonal bands that rise and
    fall smoothly, so a cover scaled up from block greys stays close."""
    step = (x * 8 + y * 4 + seed * 48) % 512
    return min(step, 511 - step)


def rgb_for_grey(level):
    """RGB with BT.601 grey `level`, keeping R=0 up to 178 so R-only draws mostly black."""
    g = min(255, round(level * 1000 / 587))
    b = min(255, round(max(0, level * 1000 - g * 587) / 114))
    r = min(255, round(max(0, level * 1000 - g * 587 - b * 114) / 299))
    return r, g, b


def jpeg_rgb(width, height, level, frame=0xc0, split=False):
    """Adobe APP14 transform-0 RGB JPEG (IDs R/G/B, shared Huffman table 0)."""
    progressive, cids = frame == 0xc2, b'RGB'
    dc_counts, ac_counts = [0, 0, 0, 12] + [0] * 12, [0, 1] + [0] * 14
    scan_comps = cids[:1] if split else cids
    columns, rows = -(-width // 8), -(-height // 8)
    units = [(ci, (rgb_for_grey(level(x, y))[ci] - 128) * 8)
             for y in range(rows) for x in range(columns) for ci in range(len(scan_comps))]
    out = (b'\xff\xd8' + segment(0xee, b'Adobe\x00\x64\x00\x00\x00\x00\x00')
           + segment(0xdb, bytes(1) + bytes([1] * 64))
           + segment(frame, bytes([8]) + struct.pack('>HHB', height, width, 3)
                     + b''.join(bytes([cid, 0x11, 0]) for cid in cids))
           + segment(0xc4, bytes([0, *dc_counts, *range(12), 0x10, *ac_counts, 0]))
           + segment(0xda, bytes([len(scan_comps)]) + b''.join(bytes([cid, 0]) for cid in scan_comps)
                     + bytes([0, 0, 1] if progressive else [0, 63, 0]))
           + encode_units(units, progressive))
    if progressive:
        out += segment(0xda, bytes([1, cids[0], 0, 1, 63, 0])) + entropy([0, 0] * (columns * rows))
    return out + b'\xff\xd9'


def art(seed, frame=0xc0, sampling=None):
    """A 288x376 cover (twice the slot) of diagonal bands; `seed` shifts them."""
    return jpeg(288, 376, lambda x, y: (grey(seed, x, y) - 128) * 8, frame, sampling)


def png(width, height, row, colour=0, interlace=0):
    """An 8-bit PNG whose row `y` holds the greys `row(y)`, grey (colour 0) or
    RGB (2). An interlaced one keeps these rows: the firmware refuses it by
    its header."""
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    rows = b''.join(b'\0' + (bytes(v for g in row(y) for v in (g, g, g)) if colour == 2 else row(y))
                    for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, colour, 0, 0, interlace))
            + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))


# Grey 8-bit: the row is the width and the ring two bytes more, so this width
# puts row and ring at the 10 KiB cap (rust/core/src/cover/png.rs ROW_BYTES).
CAP_WIDTH = (10 * 1024 - 2) // 2


def picture(kind, seed, frame=0xc0, sampling=None):
    """The cover image of `kind`: art(seed) as a JPEG or PNG, or a flat grey
    PNG at (png-wide) or past (png-too-wide) the row cap."""
    bands = lambda y: bytes(grey(seed, x // 8, y // 8) for x in range(288))
    return {'jpeg': lambda: art(seed, frame, sampling),
            'jpeg-rgb': lambda: jpeg_rgb(288, 376, lambda x, y: grey(seed, x, y), frame),
            'jpeg-rgb-split': lambda: jpeg_rgb(288, 376, lambda x, y: grey(seed, x, y), frame, split=True),
            'png-grey': lambda: png(288, 376, bands),
            'png-rgb': lambda: png(288, 376, bands, colour=2),
            'png-interlaced': lambda: png(288, 376, bands, interlace=1),
            # 996 rows put it at 144x28 in the slot, rows 80 to 107.
            'png-wide': lambda: png(CAP_WIDTH, 996, lambda y: bytes([128]) * CAP_WIDTH),
            'png-too-wide': lambda: png(CAP_WIDTH + 1, 8, lambda y: bytes([128]) * (CAP_WIDTH + 1))}[kind]()


def page(doctype, body):
    """An XHTML page with `body` in its body."""
    return ('<?xml version="1.0" encoding="UTF-8"?>' + doctype
            + '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head>'
            + f'<body><div class="cover">{body}</div></body></html>')


def img(source):
    return f'<img src="{source}" alt=""/>'


def epub(path, title, image, form='epub3', deflated=False, media='image/jpeg'):
    """The sample under `title`, with `image` as its cover declared by `form`:
    epub3 (cover-image), epub2 (<meta name="cover">), guide-image (the guide
    names the image), guide-page (the guide names a cover page), subset-page
    (that page's internal DTD subset spells the image's path), or a cover
    meta that names no item id, with the image's href as its text
    (meta-href), the image's id "cover" (meta-cover-id) or the image's file
    named "cover" (meta-cover-file), or the first spine page holding only the
    image, in SVG (spine-page) or with text beside it (spine-text-page)."""
    with zipfile.ZipFile(SAMPLE) as source:
        opf = source.read('OEBPS/content.opf').decode()
        opf = opf.replace('The Small Lantern', title).replace('small-lantern:1', title)
        file = ('images/cover.jpg' if form == 'meta-cover-file'
                else 'images/art.png' if media == 'image/png' else 'images/art.jpg')
        items = f'<item id="{"cover" if form == "meta-cover-id" else "art"}" href="{file}" media-type="{media}"/>'
        pages = {}
        if form == 'epub3':
            items = items.replace('/>', ' properties="cover-image"/>')
        elif form == 'epub2':
            opf = opf.replace('</metadata>', '<meta name="cover" content="art"/></metadata>')
        elif form.startswith('meta-'):
            content = 'images/art.jpg' if form == 'meta-href' else 'missing-id'
            opf = opf.replace('</metadata>', f'<meta name="cover" content="{content}"/></metadata>')
            if form == 'meta-cover-file':
                # Only images count: a page with the id "cover" is passed over,
                # though as an image it would outrank the file name. Its own
                # image is one the firmware cannot decode.
                items = '<item id="cover" href="text/title.xhtml" media-type="application/xhtml+xml"/>' + items
                pages['OEBPS/text/title.xhtml'] = page('', img('../images/title.jpg'))
                pages['OEBPS/images/title.jpg'] = jpeg(8, 8, lambda x, y: 0, frame=0xc9)
        elif form.startswith('spine-'):
            opf = opf.replace('<spine>', '<spine><itemref idref="cover-page" linear="no"/>')
            items += '<item id="cover-page" href="text/cover.xhtml" media-type="application/xhtml+xml"/>'
            pages['OEBPS/text/cover.xhtml'] = page('', (
                '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
                'viewBox="0 0 288 376">\n  <image width="288" height="376" xlink:href="../images/art.jpg"/>\n</svg>'
                if form == 'spine-page' else img('../images/art.jpg') + '<p>Chapter One</p>'))
        elif form == 'guide-image':
            opf = opf.replace('</spine>', '</spine><guide><reference type="cover" href="images/art.jpg"/></guide>')
        else:
            # The page sits in its own folder, so its image path is relative to it.
            opf = opf.replace('</spine>', '</spine><guide><reference type="cover" href="text/cover.xhtml#top"/></guide>')
            items += '<item id="cover-page" href="text/cover.xhtml" media-type="application/xhtml+xml"/>'
            pages['OEBPS/text/cover.xhtml'] = (page('<!DOCTYPE html>', img('../images/art.jpg')) if form == 'guide-page'
                else page('<!DOCTYPE html [<!ENTITY art "../images/art.jpg">]>', img('&art;')))
        opf = opf.replace('</manifest>', items + '</manifest>')
        compression = zipfile.ZIP_DEFLATED if deflated else zipfile.ZIP_STORED
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_STORED) as archive:
            for entry in source.infolist():
                archive.writestr(entry, opf if entry.filename == 'OEBPS/content.opf' else source.read(entry))
            for name, text in pages.items():
                archive.writestr(name, text, compression)
            archive.writestr(f'OEBPS/{file}', image, compression)
    os.utime(path, (MTIME, MTIME))


@functools.cache
def books():
    """Newt/Books for every world, made once per run."""
    folder = Path(tempfile.mkdtemp(prefix='newt-cover-books-'))
    forms = [(name, seed, form) for seed, (name, form) in enumerate(SUPPORTED.items())]
    for name, seed, form in forms + [(name, 0, form) for name, (form, _) in NO_COVER.items()]:
        form = dict(form)
        kind = form.pop('image', 'jpeg')
        if kind.startswith('png'):
            form['media'] = 'image/png'
        epub(folder / f'{name}.epub', name, picture(kind, seed, form.pop('frame', 0xc0), form.pop('sampling', None)), **form)
    return folder


def record(book, route):
    """A bare cover record for `book` on the `route` (CJB3, CJB5, CJB6 or CJB7)."""
    return SIGNATURE + struct.pack('<QQ', (books() / f'{book}.epub').stat().st_size, MTIME) + route


def prepared(book, raster):
    """An art cover record for `book`: its check is the raster's CRC-32, so
    it carries no route."""
    return record(book, struct.pack('<I', zlib.crc32(raster))) + raster


class World:
    """A card with Newt/Books and no Newt/Covers, Quick Start skipped."""

    def __init__(self, binary, scratch, records=None, **conf):
        self.binary, self.directory = binary, Path(scratch)
        preview_driver.first_boots(binary, self.directory, [preview_driver.SKIP_QUICK_START])
        shutil.copytree(books(), self.directory / 'card/Newt/Books')
        self.covers = self.directory / 'card/Newt/Covers'
        assert not self.covers.exists(), 'the card already holds Newt/Covers'
        if records:
            self.covers.mkdir()
            for book, data in records.items():
                (self.covers / f'{book}.epub.nlc').write_bytes(data)
        preview_driver.write_conf(self.directory, conf)

    def run(self, keys):
        """(final frame as rows of bits, attempt result codes, serial log)."""
        preview_driver.fresh(self.directory)
        _, log = rust_preview.run(self.binary, self.directory, keys)
        _, size, pixels = (self.directory / 'frame.pbm').read_bytes().split(b'\n', 2)
        width = int(size.split()[0])
        frame = [pixels[at:at + (width + 7) // 8] for at in range(0, len(pixels), (width + 7) // 8)]
        return frame, [int(code) for code in ROWS.findall(log)], log

    def cover(self, book):
        path = self.covers / f'{book}.epub.nlc'
        return path.read_bytes() if path.exists() else None

    def temporaries(self):
        return sorted(path.name for path in self.covers.glob('*.tmp')) if self.covers.exists() else []


def to_book(name):
    """Keys from Home to Books with `name` selected, which shows its shelf."""
    return (CONFIRM,) + (RIGHT,) * BOOKS.index(name)


def shelves():
    """The books of each shelf of four, in slot order."""
    return [BOOKS[at:at + 4] for at in range(0, len(BOOKS), 4)]


def region(frame, left, top):
    """The 144x188 one-bit region at (left, top), packed as a cover raster."""
    out = bytearray()
    for y in range(top, top + HEIGHT):
        bits = [frame[y][x // 8] >> (7 - x % 8) & 1 for x in range(left, left + WIDTH)]
        out += bytes(int(''.join(map(str, bits[at:at + 8])), 2) for at in range(0, WIDTH, 8))
    return bytes(out)


def slot(frame, index):
    """Books shelf slot `index` (rust/library-current-ui origin())."""
    return region(frame, 74 + index % 2 * 236, 138 + index // 2 * 232)


def shade(book):
    """The grey of each 4x4 cell of `book`'s cover. Art is drawn at half size,
    so each of its 8x8 blocks is one cell; the wide PNG is a mid-grey band on
    white."""
    if book == WIDE:
        return lambda cx, cy: 128 if 20 <= cy < 27 else 255
    seed = list(SUPPORTED).index(book)
    return lambda cx, cy: grey(seed, cx, cy)


def ink_problem(raster, shade):
    """Whether `raster` shows `shade`: a cell's black share follows its grey
    within any 4x4 ordered dither."""
    for cy in range(HEIGHT // 4):
        for cx in range(WIDTH // 4):
            black = sum(raster[(cy * 4 + y) * ROW_BYTES + cx // 2] >> (7 - (cx % 2 * 4 + x)) & 1
                        for y in range(4) for x in range(4))
            expected = 16 * (255 - shade(cx, cy)) / 255
            if abs(black - expected) > 2:
                return f'cell ({cx}, {cy}) has {black} of 16 black, not about {expected:.0f}'


def art_problem(world, book):
    """Whether `book` has a cover record that shows its own art."""
    data = world.cover(book)
    if data is None or len(data) != ART or data[:8] != SIGNATURE:
        return f'{book} has no {ART}-byte cover record ({None if data is None else len(data)})'
    if problem := ink_problem(data[28:], shade(book)):
        return f'{book}\'s cover does not show its art: {problem}'


# Each case returns None when the firmware behaved, else what went wrong.
def books_shelf(binary):
    """Books makes each visible shelf's covers and repaints with them; each
    book that must get no cover gets a bare record with its "no cover" result."""
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch)
        shown = {}
        for names in shelves():
            frame, results, _ = world.run(to_book(names[0]))
            expected = [NO_COVER[book][1] if book in NO_COVER else PRODUCED for book in names]
            if results != expected:
                return f'the shelf of {names} ended {results}, not {expected}'
            for index, book in enumerate(names):
                shown[book] = slot(frame, index)
                if book in NO_COVER:
                    if world.cover(book) != record(book, CJB7):
                        return f'{book} has {world.cover(book)!r}, not a bare CJB7 record'
                elif problem := art_problem(world, book):
                    return problem
                elif shown[book] != world.cover(book)[28:]:
                    return f'the shelf does not show {book}\'s new cover'
        if len({world.cover(book) for book in SUPPORTED}) != len(SUPPORTED):
            return 'two books share one cover'
        if any(shown[book] == shown[GAP] for book in SUPPORTED):
            return 'a shelf cover looks like the "No cover" slot'
        if world.temporaries():
            return f'cover work left {world.temporaries()}'


def home(binary):
    """Home makes the current book's cover, reads it again and paints it, for
    each supported cover form."""
    for book in SUPPORTED:
        with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
            world = World(binary, scratch)
            world.run(to_book(book) + (CONFIRM,))
            # Books made its shelves before the book opened; leave Home its own work.
            shutil.rmtree(world.covers)
            frame, results, log = world.run((BACK,))
            if problem := art_problem(world, book):
                return problem
            if results != [PRODUCED] or HOME_READY not in log.split(f'result={PRODUCED} ', 1)[-1]:
                return f'Home\'s attempts for {book} ended {results} with no later "{HOME_READY}"'
            if region(frame, 192, 94) != world.cover(book)[28:]:
                return f'Home does not paint {book}\'s new cover'


def old_records(binary):
    """Bare records from older decoders (CJB6 as v0.4.1 wrote it, and the
    oldest, CJB3) are retried once and then never again; a current CJB7 one,
    and art an older firmware made, are used as they are."""
    # Past the first shelf only the gap is retried: the books the walk to it
    # passes, and its shelf's other books, are current.
    gap_shelf = shelves()[-1]
    kept = prepared('Dogwood', bytes(range(256)) * 13 + bytes(56))
    records = {**{book: record(book, CJB7) for book in BOOKS[4:]}, 'Alder': record('Alder', CJB6),
               'Birch': record('Birch', CJB7), 'Cedar': record('Cedar', CJB3), 'Dogwood': kept, GAP: record(GAP, CJB3)}
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch, records)
        _, results, _ = world.run(SHELF)
        for book, route in (('Alder', 'CJB6'), ('Cedar', 'CJB3')):
            if problem := art_problem(world, book):
                return f'the {route} record was not retried: {problem}'
        if world.cover('Birch') != record('Birch', CJB7) or world.cover('Dogwood') != kept or results != [PRODUCED] * 2:
            return f'the CJB7 record or the older art was not kept as it is (attempts {results})'
        _, results, _ = world.run(to_book(gap_shelf[0]))
        if results != [UNSUPPORTED] or world.cover(GAP) != record(GAP, CJB7):
            return f'the unsupported book\'s CJB3 retry ended {results} with {world.cover(GAP)!r}'
        _, results, _ = world.run(to_book(gap_shelf[0]))
        if results:
            return f'the unsupported book was retried again: {results}'


def transient(binary):
    """A card fault and a press during cover work write no permanent record,
    so a later visit still makes the book's cover."""
    # The book's staging copy (its container, OPF, then deflated image) is
    # read only by cover work.
    book = 'Birch'
    stage = f'/newt-sd/Newt/Covers/{book}.epub.cover-stage.tmp'
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch, **{'store.refuse': f'card-read:{stage}'})
        _, results, _ = world.run(SHELF)
        if IO_ERROR not in results or world.cover(book) is not None or world.temporaries():
            return f'a read fault ended {results}, left {world.cover(book)!r} and {world.temporaries()}'
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        # Back during its work: Home has no book, so no other cover work runs.
        # A cancelled attempt logs no row (input is pending), so only Alder's shows.
        world = World(binary, scratch, **{'card.back': stage})
        _, results, _ = world.run(SHELF)
        if results != [PRODUCED] or world.cover(book) is not None or world.temporaries():
            return f'Back during cover work ended {results}, left {world.cover(book)!r} and {world.temporaries()}'
        preview_driver.write_conf(world.directory, {'card.back': ''})
        _, results, _ = world.run(SHELF)
        if results != [PRODUCED] * 3 or (problem := art_problem(world, book)):
            return f'the next visit ended {results}: {art_problem(world, book)}'


def low_heap(binary):
    """With too little free heap for the wide PNG's decode block, Memory's live
    check refuses it: no record and no temporary, so a later visit with the
    heap back makes its cover."""
    shelf = next(names for names in shelves() if WIDE in names)
    # Only the wide PNG's shelf has cover work; the walk passes current books.
    records = {book: record(book, CJB7) for book in BOOKS if book not in shelf}
    # Room for 64 KiB kept free and 53 KiB more: enough to admit cover work
    # (its planned peak includes the staging write block) and for every other
    # book's blocks, too little for the wide PNG's block, a little over 53 KiB.
    low = {'cover.heap.free': str((64 + 53) * 1024)}
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch, records, **low)
        _, results, _ = world.run(to_book(shelf[0]))
        if DECLINED not in results or world.cover(WIDE) is not None or world.temporaries():
            left = world.cover(WIDE)
            return f'a low heap ended {results}, left {left and len(left)} record bytes and {world.temporaries()}'
        if problem := art_problem(world, next(book for book in shelf if book in SUPPORTED and book != WIDE)):
            return f'a low heap stopped other covers: {problem}'
        preview_driver.write_conf(world.directory, {'cover.heap.free': ''})
        _, results, _ = world.run(to_book(shelf[0]))
        if problem := art_problem(world, WIDE):
            return f'the next visit ended {results}: {problem}'


def commit_heap_drop(binary):
    """An admitted CRC block survives a later heap dip during the pending open,
    for both artwork and a no-art record."""
    for book, result in (('Alder', PRODUCED), (GAP, UNSUPPORTED)):
        records = {name: record(name, CJB7) for name in BOOKS if name != book}
        with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
            world = World(binary, scratch, records, **{'cover.heap.drop.on': '.nlc.tmp'})
            _, results, _ = world.run(to_book(book))
            # The dip may decline a later job's admission; this commit must finish.
            if results[:1] != [result] or world.temporaries():
                return f'commit heap dip ended {results}, left {world.temporaries()}'
            if book in SUPPORTED:
                if problem := art_problem(world, book):
                    return problem
            elif world.cover(book) != record(book, CJB7):
                return 'commit heap dip lost the no-art record'


def changed_book(binary):
    """A book whose bytes change after cover work has read it whole, keeping
    its size and times as a same-size copy on FAT can, gets no record and no
    temporary from that attempt; a later visit makes the cover of its new
    bytes."""
    book = 'Alder'
    edit = {'card.edit': f'/newt-sd/Newt/Books/{book}.epub', 'card.edit.on': f'{book}.epub.cover-stage.tmp'}
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch, **edit)
        # A filler member makes the book many CRC reads long, with the middle
        # byte that changes inside it, so the book's cover stays readable.
        path = world.directory / f'card/Newt/Books/{book}.epub'
        with zipfile.ZipFile(path, 'a') as archive:
            archive.writestr('OEBPS/filler.bin', bytes(64 * 1024))
        os.utime(path, (MTIME, MTIME))
        _, results, _ = world.run(SHELF)
        if results[:1] != [INVALID] or world.cover(book) is not None or world.temporaries():
            return f'the changed book\'s attempt ended {results[:1]}, left {world.cover(book)!r} and {world.temporaries()}'
        preview_driver.write_conf(world.directory, {key: '' for key in edit})
        _, results, _ = world.run(SHELF)
        if results != [PRODUCED] or (problem := art_problem(world, book)):
            return f'the next visit ended {results}: {art_problem(world, book)}'


def damaged_records(binary):
    """Library's cover-fault card holds prepared covers cut short. Its fixture
    holds cover work back, so the reviewed frame shows the fault; let go,
    Books redraws each cover from its book's PNG and shows the art."""
    def visit(fixture, conf):
        with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
            directory = Path(scratch)
            preview_driver.first_boots(binary, directory, [preview_driver.SKIP_QUICK_START])
            preview_driver.library(fixture, directory / 'card')
            preview_driver.write_conf(directory, conf)
            covers = directory / 'card/Newt/Covers'
            damaged = {path.name: path.read_bytes() for path in covers.glob('*.nlc')} if covers.exists() else {}
            preview_driver.fresh(directory)
            rust_preview.run(binary, directory, SHELF)
            records = {name: (covers / name).read_bytes() for name in damaged}
            return (directory / 'frame.pbm').read_bytes(), damaged, records
    no_covers, _, _ = visit('no-covers', {})
    shelf, damaged, records = visit('cover-fault', {})
    if len(damaged) != 4:
        return f'the cover-fault card holds {sorted(damaged)}, not four damaged records'
    for name, data in records.items():
        if data == damaged[name] or len(data) != ART or data[:8] != SIGNATURE or not any(data[28:]):
            return f'{name} was not redrawn: {len(damaged[name])} damaged bytes became {len(data)}'
    if shelf == no_covers:
        return 'Books shows the no-cover card after redrawing every cover'


# (case, its controls: [(generated firmware file, exact text, replacement)]).
CASES = [
    (books_shelf, [
        # A produced cover neither queues nor takes its repaint.
        [('reader_cover_producer.rs',
          '                if result==CoverResult::Produced{self.idle.defer_repaint();}\n'
          '                if result==CoverResult::Cancelled{return false;}\n'
          '                crate::app_power::delay_ms(10);\n'
          '                if result!=CoverResult::Produced||poll.stopped(){return false;}\n'
          '                self.idle.consume_repaint();true',
          '                let _=result;false')],
        # The decode counts its rows but draws none, so every cover is blank.
        [('cover_decode.rs',
          'for x in 0..usize::from(width){if bits[x/8]&(0x80>>(x%8))!=0{let target=x+self.left;out[target/8]|=0x80>>(target%8);}}',
          'let _=(bits,out);')],
        # A cover page's internal DTD subset is read, so its entities expand.
        [('cover_xml.rs', 'self.0.doctype(subset)', 'self.0.doctype(false)')]]),
    (home, [
        # A produced cover does not advance the publication counter.
        [('cover_idle_step.rs', '    if outcome.result==R::Produced{published();}\n', '')],
        # Home hands the cover source its book name without the terminator.
        [('app_runtime/home_cover.rs', 'prepare_home_cover(&mut covers, Some(&identity.filename), pixels)',
          'prepare_home_cover(&mut covers, newt_core::library_name::name_bytes(&identity.filename), pixels)')]]),
    (old_records, [
        # A CJB6 bare record reads as current.
        [('cover_probe.rs', 'match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){',
          'match cover::record((payload!=0).then_some(crc^u32::MAX),'
          'if le32(&header[24..])==u32::from_le_bytes(*b"CJB6"){cover::NO_ART}else{le32(&header[24..])}){')],
        # A CJB3 bare record reads as current, as its old flag-migration walk
        # kept it.
        [('cover_probe.rs', 'match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){',
          'match cover::record((payload!=0).then_some(crc^u32::MAX),'
          'if le32(&header[24..])==u32::from_le_bytes(*b"CJB3"){cover::NO_ART}else{le32(&header[24..])}){')],
        # Art is checked against the wrong CRC, so it is made again.
        [('cover_probe.rs', 'match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){',
          'match cover::record((payload!=0).then_some(crc),le32(&header[24..])){')],
        # A retry records its verdict on the old route, so it retries again.
        [('cover_session.rs', 'self.commit(&self.header(cover::NO_ART),&[])',
          'self.commit(&self.header(u32::from_le_bytes(*b"CJB3")),&[])')]]),
    (low_heap, [
        # Memory admits a block however little heap would stay free.
        [('cover_memory.rs', ' || self.heap.free_bytes() < charge + RESERVE', '')]]),
    (transient, [
        # An I/O fault records the book as having no cover.
        [('cover_session.rs', 'if !verdict(result)||!self.unfaulted(){self.no_art=false;return(result,false);}',
          'if !verdict(result)&&result!=R::IoError{self.no_art=false;return(result,false);}')]]),
    (commit_heap_drop, [
        [('cover_session.rs',
          '        // Admit all commit workspace before writing a pending record.\n'
          '        let Some(mut block)=self.memory.block(io::CRC_BYTES)else{return R::Declined;};\n', ''),
         ('cover_session.rs',
          '        if self.source.crc(block.as_mut())!=Some(self.source_crc)||',
          '        let Some(mut block)=self.memory.block(io::CRC_BYTES)else{return R::Declined;};\n'
          '        if self.source.crc(block.as_mut())!=Some(self.source_crc)||')]]),
    (changed_book, [
        # A changed book's second whole read is not compared with its first.
        [('cover_session.rs', 'if self.source.crc(block.as_mut())!=Some(self.source_crc)||',
          'if self.source.crc(block.as_mut()).is_none()||')]]),
    (damaged_records, [
        # A record cut short counts as current, so its cover is never redrawn.
        [('cover_probe.rs', 'if file.size()!=28&&file.size()!=(28+cover::PIXELS)as u64{return Probe::Generate;}',
          'if file.size()!=28&&file.size()!=(28+cover::PIXELS)as u64{return Probe::Current;}')]]),
]


def main():
    # The gate passes its one preview build; controls relink from it.
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    try:
        for case, _ in CASES:
            problem = case(binary)
            assert problem is None, f'{case.__name__}: {problem}'
        if not negative_controls(__file__):
            print(f'PASS: {len(CASES)} cover flows run the firmware (controls not in this tier)')
            return
        for case, controls in CASES:
            for edits in controls:
                control = rust_preview.build(edits=edits)
                assert case(control) is not None, f'{case.__name__}: passes with {edits[0][0]} changed'
        print(f'PASS: {len(CASES)} cover flows run the firmware; each control fails')
    finally:
        shutil.rmtree(books(), ignore_errors=True)


if __name__ == '__main__':
    main()
