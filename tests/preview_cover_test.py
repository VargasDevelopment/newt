#!/usr/bin/env python3
"""The firmware makes its own covers, end to end, on Books and Home.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes). Each world is a card with no Newt/Covers and
synthetic books whose covers are made here: baseline grayscale JPEGs declared
the EPUB3 way (cover-image), the EPUB2 way (<meta name="cover">) and by the
guide (straight to the image, or to an XHTML cover page), stored and
deflated, one with the common "image/jpg" media type, a progressive JPEG (the
firmware draws its first scan, at 1/8 scale), and books that must get no
cover: one whose cover form the firmware does not decode, the known gap, and
one whose cover page carries an internal DTD subset. Cover work starts after
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

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, RIGHT = 0, 1, 3
SHELF = (CONFIRM,)
# rust/firmware/reader_cover_idle.rs CoverResult.
PRODUCED, CANCELLED, MISSING, UNSUPPORTED, IO_ERROR = 2, 3, 5, 6, 9
ROWS = re.compile(r'COVER work=attempt result=(\d+)')
HOME_READY = 'HOME_COVER eligible=1 attempted=1 step=7 ready=1 painted=1'
# A cover record: NLC2 header (signature, book size, even mtime, route) and,
# for art, the 144x188 one-bit raster.
SIGNATURE = b'NLC2\x90\0\xbc\0'
WIDTH, HEIGHT, ROW_BYTES = 144, 188, 18
ART, NO_ART = 28 + ROW_BYTES * HEIGHT, 28
# Routes as the card holds them (little-endian rust/core/src/cover.rs NO_ART words).
CJB6, CJB5 = b'CJB6', b'CJB5'
MTIME = 1790812800
# Book name: how its cover is declared and stored (epub() forms).
SUPPORTED = {
    'Alder': {'form': 'epub3'},
    'Birch': {'form': 'epub3', 'deflated': True},
    'Cedar': {'form': 'epub2'},
    'Dogwood': {'form': 'epub2', 'deflated': True},
    'Elm': {'form': 'epub3', 'media': 'image/jpg'},
    'Fir': {'form': 'epub3', 'deflated': True, 'frame': 0xc2},
    'Ginkgo': {'form': 'guide-image'},
    'Hazel': {'form': 'guide-page', 'deflated': True},
}
# Books that get a bare record: (form, the attempt's result).
NO_COVER = {
    'Willow': ({'form': 'subset-page'}, UNSUPPORTED),
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


def jpeg(width, height, dc, frame=0xc0):
    """A grayscale JPEG of flat 8x8 blocks: quantizers 1, DC category k coded
    as the 4-bit k, every AC an end of block (rust/core/tests/cover_tolerance.rs
    builds the same). `dc(x, y)` is block (x, y)'s DC value; `frame` is its
    SOF marker (0xc0 baseline). A progressive frame (0xc2) holds the DC first
    scan at point transform 1, as encoders write it, then one AC scan."""
    progressive = frame == 0xc2
    out = b'\xff\xd8' + segment(0xdb, bytes(1) + bytes([1] * 64))
    out += segment(frame, bytes([8]) + struct.pack('>HH', height, width) + bytes([1, 1, 0x11, 0]))
    dc_counts, ac_counts = [0] * 16, [0] * 16
    dc_counts[3], ac_counts[1] = 12, 1
    out += segment(0xc4, bytes([0x00, *dc_counts, *range(12), 0x10, *ac_counts, 0]))
    out += segment(0xda, bytes([1, 1, 0x00, 0, 0, 1] if progressive else [1, 1, 0x00, 0, 63, 0]))
    bits, previous = [], 0
    blocks = [(x, y) for y in range(-(-height // 8)) for x in range(-(-width // 8))]
    for x, y in blocks:
        value = dc(x, y) >> progressive
        diff, previous = value - previous, value
        category = abs(diff).bit_length()
        for value, length in ((category, 4), (diff + (1 << category) - 1 if diff < 0 else diff, category)):
            bits += [value >> i & 1 for i in reversed(range(length))]
        if not progressive:
            bits += [0, 0]
    out += entropy(bits)
    if progressive:
        # Every block's AC band ends at once.
        out += segment(0xda, bytes([1, 1, 0x00, 1, 63, 0])) + entropy([0, 0] * len(blocks))
    return out + b'\xff\xd9'


def grey(seed, x, y):
    """The grey of art(seed)'s 8x8 block (x, y): diagonal bands that rise and
    fall smoothly, so a cover scaled up from block greys stays close."""
    step = (x * 8 + y * 4 + seed * 48) % 512
    return min(step, 511 - step)


def art(seed, frame=0xc0):
    """A 288x376 cover (twice the slot) of diagonal bands; `seed` shifts them."""
    return jpeg(288, 376, lambda x, y: (grey(seed, x, y) - 128) * 8, frame)


def page(doctype, source):
    """An XHTML cover page whose body holds one image."""
    return ('<?xml version="1.0" encoding="UTF-8"?>' + doctype
            + '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head>'
            + f'<body><div class="cover"><img src="{source}" alt=""/></div></body></html>')


def epub(path, title, image, form='epub3', deflated=False, media='image/jpeg'):
    """The sample under `title`, with `image` as its cover declared by `form`:
    epub3 (cover-image), epub2 (<meta name="cover">), guide-image (the guide
    names the image), guide-page (the guide names a cover page) or
    subset-page (that page's internal DTD subset spells the image's path)."""
    with zipfile.ZipFile(SAMPLE) as source:
        opf = source.read('OEBPS/content.opf').decode()
        opf = opf.replace('The Small Lantern', title).replace('small-lantern:1', title)
        items = f'<item id="art" href="images/art.jpg" media-type="{media}"/>'
        pages = {}
        if form == 'epub3':
            items = items.replace('/>', ' properties="cover-image"/>')
        elif form == 'epub2':
            opf = opf.replace('</metadata>', '<meta name="cover" content="art"/></metadata>')
        elif form == 'guide-image':
            opf = opf.replace('</spine>', '</spine><guide><reference type="cover" href="images/art.jpg"/></guide>')
        else:
            # The page sits in its own folder, so its image path is relative to it.
            opf = opf.replace('</spine>', '</spine><guide><reference type="cover" href="text/cover.xhtml#top"/></guide>')
            items += '<item id="cover-page" href="text/cover.xhtml" media-type="application/xhtml+xml"/>'
            pages['OEBPS/text/cover.xhtml'] = (page('<!DOCTYPE html>', '../images/art.jpg') if form == 'guide-page'
                else page('<!DOCTYPE html [<!ENTITY art "../images/art.jpg">]>', '&art;'))
        opf = opf.replace('</manifest>', items + '</manifest>')
        compression = zipfile.ZIP_DEFLATED if deflated else zipfile.ZIP_STORED
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_STORED) as archive:
            for entry in source.infolist():
                archive.writestr(entry, opf if entry.filename == 'OEBPS/content.opf' else source.read(entry))
            for name, text in pages.items():
                archive.writestr(name, text, compression)
            archive.writestr('OEBPS/images/art.jpg', image, compression)
    os.utime(path, (MTIME, MTIME))


@functools.cache
def books():
    """Newt/Books for every world, made once per run."""
    folder = Path(tempfile.mkdtemp(prefix='newt-cover-books-'))
    for seed, (name, form) in enumerate(SUPPORTED.items()):
        form = dict(form)
        epub(folder / f'{name}.epub', name, art(seed, form.pop('frame', 0xc0)), **form)
    for name, (form, _) in NO_COVER.items():
        form = dict(form)
        epub(folder / f'{name}.epub', name, art(0, form.pop('frame', 0xc0)), **form)
    return folder


def record(book, route):
    """A bare cover record for `book` on the `route` (CJB5 or CJB6)."""
    return SIGNATURE + struct.pack('<QQ', (books() / f'{book}.epub').stat().st_size, MTIME) + route


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
            for book, route in records.items():
                (self.covers / f'{book}.epub.nlc').write_bytes(record(book, route))
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


def ink_problem(raster, seed):
    """Whether `raster` shows art(seed). The cover is the art at half size, so
    each source block is a 4x4 cell; a cell's black share follows its grey
    within any 4x4 ordered dither."""
    for cy in range(HEIGHT // 4):
        for cx in range(WIDTH // 4):
            black = sum(raster[(cy * 4 + y) * ROW_BYTES + cx // 2] >> (7 - (cx % 2 * 4 + x)) & 1
                        for y in range(4) for x in range(4))
            expected = 16 * (255 - grey(seed, cx, cy)) / 255
            if abs(black - expected) > 2:
                return f'cell ({cx}, {cy}) has {black} of 16 black, not about {expected:.0f}'


def art_problem(world, book):
    """Whether `book` has a cover record that shows its own art."""
    data = world.cover(book)
    if data is None or len(data) != ART or data[:8] != SIGNATURE:
        return f'{book} has no {ART}-byte cover record ({None if data is None else len(data)})'
    if problem := ink_problem(data[28:], list(SUPPORTED).index(book)):
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
                    if world.cover(book) != record(book, CJB6):
                        return f'{book} has {world.cover(book)!r}, not a bare CJB6 record'
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
    """A CJB5 bare record (from before #46, as v0.3.0 wrote it) is retried
    once and then never again; a current CJB6 one is used as it is."""
    # Past the first shelf only the gap is retried: the books the walk to it
    # passes, and its shelf's other books, are current.
    gap_shelf = shelves()[-1]
    records = {**{book: CJB6 for book in BOOKS[4:]}, 'Alder': CJB5, 'Birch': CJB6, GAP: CJB5}
    with tempfile.TemporaryDirectory(prefix='newt-covers-') as scratch:
        world = World(binary, scratch, records)
        _, results, _ = world.run(SHELF)
        if problem := art_problem(world, 'Alder'):
            return f'the CJB5 record was not retried: {problem}'
        if world.cover('Birch') != record('Birch', CJB6) or results != [PRODUCED] * 3:
            return f'the CJB6 record was not kept as it is (attempts {results})'
        _, results, _ = world.run(to_book(gap_shelf[0]))
        if results != [UNSUPPORTED] or world.cover(GAP) != record(GAP, CJB6):
            return f'the unsupported book\'s CJB5 retry ended {results} with {world.cover(GAP)!r}'
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
        [('cover_xml.rs', 'c.state.doctype(subset!=0);', 'c.state.doctype(false);')]]),
    (home, [
        # A produced cover does not advance the publication counter.
        [('cover_idle_step.rs', '    if outcome.result==R::Produced{published();}\n', '')],
        # Home hands the cover source its book name without the terminator.
        [('app_runtime/home_cover.rs', 'prepare_home_cover(&mut covers, Some(&identity.filename), pixels)',
          'prepare_home_cover(&mut covers, newt_core::library_name::name_bytes(&identity.filename), pixels)')]]),
    (old_records, [
        # A CJB5 bare record reads as current.
        [('cover_probe.rs', 'match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){',
          'match cover::record((payload!=0).then_some(crc^u32::MAX),'
          'if le32(&header[24..])==cover::RETRIED_NO_ART[1]{cover::NO_ART}else{le32(&header[24..])}){')],
        # A retry records its verdict on the old route, so it retries again.
        [('cover_session.rs', 'self.commit(&self.header(cover::NO_ART),&[])',
          'self.commit(&self.header(cover::RETRIED_NO_ART[1]),&[])')]]),
    (transient, [
        # An I/O fault records the book as having no cover.
        [('cover_session.rs', 'if !verdict(result)||!self.unfaulted(){self.no_art=false;return(result,false);}',
          'if !verdict(result)&&result!=R::IoError{self.no_art=false;return(result,false);}')]]),
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
