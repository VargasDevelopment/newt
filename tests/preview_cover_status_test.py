#!/usr/bin/env python3
"""End-to-end host preview tests and negative controls for #48 cover status and no-cover reasons."""
import os
from pathlib import Path
import re
import struct
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, RIGHT, WAIT_MINUTE = 0, 1, 3, 7
MTIME = 1790812800
SAMPLE = ROOT / 'tests/fixtures/the-small-lantern.epub'
GEN = re.compile(r'STATUS screen=(\d+) gen=(\d+) ')
ATTEMPTS = re.compile(r'COVER work=attempt result=(\d+)')
PRODUCED, DECLINED, MISSING, UNSUPPORTED, INCOMPLETE, INVALID, IO_ERROR = 2, 4, 5, 6, 7, 8, 9


def segment(marker, body):
    return bytes([0xff, marker]) + (len(body) + 2).to_bytes(2, 'big') + body


def minimal_jpeg(frame=0xc0, *, truncate_eoi=False, corrupt_scan=False):
    dc_counts, ac_counts = [0, 0, 0, 12] + [0] * 12, [0, 1] + [0] * 14
    out = b'\xff\xd8' + segment(0xdb, bytes([0]) + bytes([1] * 64))
    out += segment(frame, bytes([8]) + struct.pack('>HHB', 8, 8, 1) + bytes([1, 0x11, 0]))
    out += segment(0xc4, bytes([0, *dc_counts, *range(12), 0x10, *ac_counts, 0]))
    out += segment(0xda, bytes([1, 1, 0x00, 0, 63, 0]))
    out += bytes([0x00, 0x00]) if corrupt_scan else bytes([0x03])
    return out if truncate_eoi else out + b'\xff\xd9'


def make_epub(path, title, cover_bytes=None):
    with zipfile.ZipFile(SAMPLE) as src:
        opf = src.read('OEBPS/content.opf').decode()
        opf = opf.replace('The Small Lantern', title).replace('small-lantern:1', title)
        if cover_bytes is not None:
            item = '<item id="art" href="images/art.jpg" media-type="image/jpeg" properties="cover-image"/>'
            opf = opf.replace('</manifest>', item + '</manifest>')
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_STORED) as dst:
            for entry in src.infolist():
                dst.writestr(entry, opf if entry.filename == 'OEBPS/content.opf' else src.read(entry))
            if cover_bytes is not None:
                dst.writestr('OEBPS/images/art.jpg', cover_bytes, zipfile.ZIP_DEFLATED)
    os.utime(path, (MTIME, MTIME))


def parse_pbm(data):
    _, size, pixels = data.split(b'\n', 2)
    w, h = map(int, size.split())
    stride = (w + 7) // 8
    return [pixels[y * stride:(y + 1) * stride] for y in range(h)]


def crop(rows, x0, y0, w, h):
    return bytes(
        (rows[y][x >> 3] >> (7 - (x & 7))) & 1
        for y in range(y0, y0 + h) for x in range(x0, x0 + w)
    )


def slot_box(rows, index):
    return crop(rows, 74 + index % 2 * 236, 138 + index // 2 * 232, 144, 188)


def note_strip(rows):
    return crop(rows, 50, 675, 428, 30)


def run_once(binary, directory, keys, conf=None):
    if conf is not None:
        preview_driver.write_conf(directory, conf)
    preview_driver.fresh(directory)
    _, log = rust_preview.run(binary, directory, keys)
    rows = parse_pbm((directory / 'frame.pbm').read_bytes())
    receipt = (directory / 'receipt.txt').read_text() if (directory / 'receipt.txt').exists() else ''
    gens = [(int(s), int(g)) for s, g in GEN.findall(receipt)]
    return rows, [int(r) for r in ATTEMPTS.findall(log)], gens, log


def seed_world(binary, directory, books_spec):
    preview_driver.first_boots(binary, directory, [preview_driver.SKIP_QUICK_START])
    books = directory / 'card/Newt/Books'
    books.mkdir(parents=True, exist_ok=True)
    for name, payload in books_spec:
        path = books / f'{name}.epub'
        if payload == 'zero':
            path.write_bytes(b'')
            os.utime(path, (MTIME, MTIME))
        else:
            make_epub(path, name, payload)


def making_and_batch_settle(binary):
    """Four permanent no-cover verdicts show Making cover before idle, add zero
    refreshes during generation, do one batched settle repaint, save CJCM/CJCU/
    CJCI/CJCX on SD, and restore cleanly on cold restart."""
    spec = [
        ('A Missing', None),
        ('B Unsupported', minimal_jpeg(frame=0xc9)),
        ('C Incomplete', minimal_jpeg(truncate_eoi=True)),
        ('D Damaged', b'\xff\xd8\xff\xfe\x00\x01\xff\xd9'),
    ]
    with tempfile.TemporaryDirectory(prefix='newt-48-settle-') as td:
        d = Path(td)
        seed_world(binary, d, spec)
        # Freeze at the start of idle cover work: all 4 cards show "Making cover", note is blank.
        making, attempts, _, _ = run_once(binary, d, [CONFIRM], {'freeze': 'cover.work'})
        if attempts:
            return f'freeze ran after cover attempts: {attempts}'
        if any(note_strip(making)):
            return 'Making cover frame drew a bottom note'

        # Unfrozen: all 4 settle with ZERO extra refreshes during generation and 1 batched repaint (gen=3).
        settled, attempts, gens, _ = run_once(binary, d, [CONFIRM], {'freeze': ''})
        if attempts != [MISSING, UNSUPPORTED, INCOMPLETE, INVALID]:
            return f'expected [5,6,7,8], got {attempts}'
        if gens != [(6, 3)]:
            return f'expected exactly 1 batched settle repaint (screen=6 gen=3), got {gens}'
        if slot_box(making, 0) == slot_box(settled, 0):
            return 'Making cover card pixels matched settled No cover card'

        # Verify exact 28-byte versioned bare records on SD.
        expected_routes = {
            'A Missing': b'CJCM', 'B Unsupported': b'CJCU',
            'C Incomplete': b'CJCI', 'D Damaged': b'CJCX',
        }
        covers = d / 'card/Newt/Covers'
        for name, route in expected_routes.items():
            raw = (covers / f'{name}.epub.nlc').read_bytes()
            if len(raw) != 28 or raw[24:28] != route:
                return f'{name}: wrote {raw[24:28]!r} ({len(raw)} B), expected {route!r}'

        # Cold restart: zero attempts, zero extra refreshes (gen=2), and 4 distinct notes via paint_focus.
        notes = []
        for idx in range(4):
            rows, attempts, gens, _ = run_once(binary, d, [CONFIRM] + [RIGHT] * idx, {})
            if attempts:
                return f'cold restart slot {idx} re-decoded covers: {attempts}'
            want_gen = 2 + idx
            if gens != [(6, want_gen)]:
                return f'cold restart slot {idx} expected gen={want_gen}, got {gens}'
            if idx == 0 and rows != settled:
                return 'cold restart frame differed from settled Books frame'
            strip = note_strip(rows)
            if not any(strip):
                return f'slot {idx} had a blank note after cold restart'
            notes.append(strip)
        if len(set(notes)) != 4:
            return 'the four permanent reasons did not draw four distinct notes'

        # Corrupt bare route word (CJCZ) falls back to Making cover and regenerates CJCM.
        bad = bytearray((covers / 'A Missing.epub.nlc').read_bytes())
        bad[24:28] = b'CJCZ'
        (covers / 'A Missing.epub.nlc').write_bytes(bytes(bad))
        corrupt_making, _, _, _ = run_once(binary, d, [CONFIRM], {'freeze': 'cover.work'})
        if slot_box(corrupt_making, 0) != slot_box(making, 0):
            return 'corrupt bare route CJCZ did not fall back to Making cover'
        if any(note_strip(corrupt_making)):
            return 'corrupt bare route showed a bogus note while unsettled'
        _, attempts, _, _ = run_once(binary, d, [CONFIRM], {'freeze': ''})
        if attempts != [MISSING] or (covers / 'A Missing.epub.nlc').read_bytes()[24:28] != b'CJCM':
            return f'corrupt bare route did not regenerate CJCM: {attempts}'


def declined_low_heap_settle(binary):
    """Low heap (Declined) preserves PENDING_MARK_SHOWN, triggers 1 batched
    settle repaint, clears Making cover to No cover, and shows 'Cover will be
    made later' across focus moves."""
    spec = [('A One', None), ('B Two', None), ('C Three', None), ('D Four', None)]
    with tempfile.TemporaryDirectory(prefix='newt-48-declined-') as td:
        d = Path(td)
        seed_world(binary, d, spec)
        making, _, _, _ = run_once(binary, d, [CONFIRM], {'cover.heap.free': str(64 * 1024), 'freeze': 'cover.work'})
        settled, attempts, gens, _ = run_once(binary, d, [CONFIRM], {'cover.heap.free': str(64 * 1024), 'freeze': ''})
        if attempts != [DECLINED]:
            return f'low heap expected [DECLINED], got {attempts}'
        if gens != [(6, 3)]:
            return f'low heap expected 1 batched settle repaint (gen=3), got {gens}'
        if slot_box(settled, 0) == slot_box(making, 0):
            return 'low heap left Making cover stuck on screen'
        note0 = note_strip(settled)
        if not any(note0):
            return 'low heap left the deferred note blank'
        # Focus move to slot 1 keeps the deferred note and writes no .nlc files.
        focused, _, _, _ = run_once(binary, d, [CONFIRM, RIGHT], {'cover.heap.free': str(64 * 1024), 'freeze': ''})
        if note_strip(focused) != note0:
            return 'focus move on Declined shelf lost the deferred note'
        covers = d / 'card/Newt/Covers'
        if covers.exists() and list(covers.glob('*.nlc')):
            return f'Declined persisted bare records: {list(covers.glob("*.nlc"))}'


def failures_budget_stops_whole_shelf(binary):
    """When failures exceeds FAILURE_BUDGET (> 4) on slot 0 of shelf 2, slots
    1..3 have unset settled bits but still settle out of Making cover cleanly."""
    spec = [(f'Book {i:02}', minimal_jpeg()) for i in range(1, 9)]
    with tempfile.TemporaryDirectory(prefix='newt-48-making2-') as td_making:
        dm = Path(td_making)
        seed_world(binary, dm, spec[4:])
        making2, _, _, _ = run_once(binary, dm, [CONFIRM], {'freeze': 'cover.work'})
    with tempfile.TemporaryDirectory(prefix='newt-48-budget-') as td:
        d = Path(td)
        seed_world(binary, d, spec)
        # Refuse cover staging reads so every attempt ends in IoError (failures++).
        # Shelf 1 runs 4 IoErrors (failures=4); moving to shelf 2 + WAIT_MINUTE
        # clears the 15 s failure cooldown and hits the 5th IoError on slot 0
        # (failures=5 > 4), stopping slots 1..3 without setting their slot bits.
        refuse = {'store.refuse': 'card-read:/newt-sd/Newt/Covers/'}
        settled2, attempts, _, _ = run_once(binary, d, [CONFIRM, RIGHT, RIGHT, RIGHT, RIGHT, WAIT_MINUTE], refuse)
        if attempts.count(IO_ERROR) < 5:
            return f'expected >=5 IoError attempts to trip failures > 4, got {attempts}'
        # Slot 1 on shelf 2 was never attempted, so it must have transitioned from Making cover -> No cover.
        if slot_box(settled2, 1) == slot_box(making2, 1):
            return 'failures > 4 left unattempted slot 1 stuck on Making cover'
        if not any(note_strip(settled2)):
            return 'failures > 4 left the deferred note blank'


def non_retryable_blank_note(binary):
    """A 0-byte book cannot ever have a cover made: it shows No cover with a
    blank note immediately (never Making cover, never 'Cover will be made later',
    and zero extra refreshes even alongside a Declined book)."""
    spec = [('A Empty', 'zero'), ('B Normal', None)]
    with tempfile.TemporaryDirectory(prefix='newt-48-nonretry-') as td:
        d = Path(td)
        seed_world(binary, d, spec)
        rows, attempts, gens, _ = run_once(binary, d, [CONFIRM], {'cover.heap.free': str(64 * 1024)})
        if attempts != [DECLINED]:
            return f'expected [DECLINED] on B Normal, got {attempts}'
        # A Empty (focused slot 0) must keep a blank note even though B Normal hit Declined.
        if any(note_strip(rows)):
            return 'non-retryable 0-byte book showed a cover note instead of blank'
        # Focus B Normal (slot 1): must show 'Cover will be made later'.
        rows1, _, _, _ = run_once(binary, d, [CONFIRM, RIGHT], {'cover.heap.free': str(64 * 1024)})
        if not any(note_strip(rows1)):
            return 'retryable B Normal beside 0-byte book lost its deferred note'
        # A shelf with ONLY the 0-byte book never draws Making cover and does 0 extra refreshes.
    with tempfile.TemporaryDirectory(prefix='newt-48-emptyonly-') as td:
        d = Path(td)
        seed_world(binary, d, [('A Empty', 'zero')])
        rows, _, gens, _ = run_once(binary, d, [CONFIRM], {})
        if gens != [(6, 2)]:
            return f'0-byte-only shelf did extra refresh: {gens}'
        if any(note_strip(rows)):
            return '0-byte-only shelf drew a note'


def home_quiet_h2_and_retry(binary):
    """Home (H2) adds zero extra grayscale refreshes when a coverless book
    settles, and retries legacy CJB6/CJB7 records to paint art when available."""
    with tempfile.TemporaryDirectory(prefix='newt-48-home-') as td:
        d = Path(td)
        seed_world(binary, d, [('A Coverless', None)])
        # Open book from Books (which settles CJCM), return to Home for cached baseline gen.
        run_once(binary, d, [CONFIRM, CONFIRM], {})
        _, cached_attempts, cached_gens, cached_log = run_once(binary, d, [BACK], {})
        if cached_attempts:
            return f'Home cached CJCM re-decoded cover: {cached_attempts}'
        # Re-open book, remove covers, return to Home: settles to CJCM with ZERO extra Home refreshes.
        run_once(binary, d, [CONFIRM, CONFIRM], {})
        covers = d / 'card/Newt/Covers'
        if covers.exists():
            for p in covers.glob('*'):
                p.unlink()
        _, attempts, gens, log = run_once(binary, d, [BACK], {})
        if attempts != [MISSING]:
            return f'Home coverless book expected [MISSING], got {attempts}'
        if gens != cached_gens or log.count('HOME_COVER ') != cached_log.count('HOME_COVER '):
            return f'Home H2 added an extra refresh on coverless book: {gens} vs cached {cached_gens}'
        if (covers / 'A Coverless.epub.nlc').read_bytes()[24:28] != b'CJCM':
            return 'Home did not persist CJCM'

    # Legacy CJB7 bare record on a book WITH art retries on Home and paints art.
    with tempfile.TemporaryDirectory(prefix='newt-48-home-retry-') as td:
        d = Path(td)
        seed_world(binary, d, [('A HasCover', minimal_jpeg())])
        run_once(binary, d, [CONFIRM, CONFIRM], {})
        covers = d / 'card/Newt/Covers'
        covers.mkdir(parents=True, exist_ok=True)
        bp = d / 'card/Newt/Books/A HasCover.epub'
        st = bp.stat()
        bare_cjb7 = b'NLC2\x90\0\xbc\0' + struct.pack('<QQ', st.st_size, int(st.st_mtime) & ~1) + b'CJB7'
        (covers / 'A HasCover.epub.nlc').write_bytes(bare_cjb7)
        _, attempts, _, log = run_once(binary, d, [BACK], {})
        if attempts != [PRODUCED] or 'HOME_COVER eligible=1 attempted=1 step=7 ready=1 painted=1' not in log:
            return f'Home failed to retry legacy CJB7 cover: attempts={attempts}'


CASES = [
    (making_and_batch_settle, [
        # Extra refresh during generation (repaints on every Missing verdict).
        [('reader_cover_producer.rs',
          'let settle=shelf&&self.idle.pending_mark_shown()&&self.idle.shelf_done(shelf_count);',
          'let settle=shelf&&self.idle.pending_mark_shown()&&(self.idle.shelf_done(shelf_count)||result==CoverResult::Missing);')],
        # Cold restart ignores persisted BareRecord::Current(reason).
        [('library_covers.rs',
          'BareRecord::Current(reason) => CoverFact::from_reason(reason),',
          'BareRecord::Current(_) => CoverFact::Pending,')],
    ]),
    (declined_low_heap_settle, [
        # Opus must-change #1 negative control: wiping bit 6 (PENDING_MARK_SHOWN) on Declined.
        [('cover_idle_step.rs',
          'state.mark_settled_slots(all);state.mark_paused_retryable();return R::Declined;',
          'state.settled=all;return R::Declined;')],
    ]),
    (failures_budget_stops_whole_shelf, [
        # Opus must-change #2 negative control: failures > FAILURE_BUDGET fails to settle remaining slots.
        [('reader_cover_idle.rs',
          'all != 0 && (self.failures > FAILURE_BUDGET || (self.settled_slots() & all) == all)',
          'all != 0 && (self.settled_slots() & all) == all')],
    ]),
    (non_retryable_blank_note, [
        # Treating 0-byte book as Pending instead of Unavailable shows Deferred note.
        [('library_covers.rs',
          'self.last_fact = CoverFact::Unavailable;\n        if !valid_library_book_name(filename)',
          'self.last_fact = CoverFact::Pending;\n        if !valid_library_book_name(filename)')],
    ]),
    (home_quiet_h2_and_retry, [
        # Violating H2: advancing cover_generation and repainting Home when a coverless book settles.
        [('reader_cover_producer.rs',
          'let settle=shelf&&self.idle.pending_mark_shown()&&self.idle.shelf_done(shelf_count);',
          'let settle=(shelf&&self.idle.pending_mark_shown()&&self.idle.shelf_done(shelf_count))||(!shelf&&result==CoverResult::Missing);'
          'if !shelf&&result==CoverResult::Missing{Self::note_cover_publication(&mut self.cover_generation);}')],
        # Breaking CJB7 retry: treating CJB7 as current instead of Retry.
        [('cover_probe.rs',
          'match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){',
          'match cover::record((payload!=0).then_some(crc^u32::MAX),'
          'if le32(&header[24..])==u32::from_le_bytes(*b"CJB7"){cover::no_art_route(cover::NoCoverReason::Missing)}else{le32(&header[24..])}){')],
    ]),
]


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, _ in CASES:
        problem = case(binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} cover status flows run the firmware (controls not in this tier)')
        return
    for case, controls in CASES:
        for edits in controls:
            control = rust_preview.build(edits=edits)
            assert case(control) is not None, f'{case.__name__}: passes with {edits[0][0]} changed'
    print(f'PASS: {len(CASES)} cover status flows run the firmware; each control fails')


if __name__ == '__main__':
    main()
