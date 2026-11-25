#!/usr/bin/env python3
"""Host preview tests and negative controls for #115 (reflow round-trip anchor).

Changing layout settings (orientation or text size) without turning a page
reuses a RAM-only anchor instead of re-anchoring to each intermediate page's
char_start, preventing backward drift across round-trips. During a reflow,
ReflowCheckpoint commits the anchor locator and marks NWBR as an anchor for
the proposed settings so a mid-reflow power cut restores without drift and
the first ordinary save afterwards writes the rendered page's real char_start.
"""
import functools
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

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER, WAIT_MINUTE, QUEUED = range(9)
DIAG_PAIR = re.compile(r'([a-z_]+)=([0-9a-fA-F]+)')
FRAME_ORIENT = re.compile(r'FRAME [^\n]*orientation=([a-z-]+)')


def buttons(cw=False):
    """(activate, next, prev, back) physical button codes for the current orientation."""
    return (LEFT, BACK, CONFIRM, RIGHT) if cw else (CONFIRM, RIGHT, LEFT, BACK)


def settings_keys(*, cw=False, orient_presses=0, size_presses=0):
    """From a reading page, open Reading settings, cycle Orientation/TextSize, Save."""
    act, nxt, _, _ = buttons(cw)
    keys = [act, nxt, nxt, act]  # Menu -> Reading settings (row 0 = Orientation)
    keys += [act] * orient_presses
    if size_presses:
        keys += [nxt] + [act] * size_presses + [nxt, nxt, nxt, act]
    else:
        keys += [nxt, nxt, nxt, nxt, act]
    return tuple(keys)


def make_synthetic_epub(path, *, title='Reflow Ledger', chapter1_paras=22, chapter2_paras=8):
    """Build a minimal valid CC0 synthetic EPUB with two multi-page chapters."""
    def chapter_xhtml(heading, count, seed):
        lines = []
        for idx in range(1, count + 1):
            lines.append(
                f'<p>Entry {seed}-{idx:02d}: The tide pulled softly along the timber pier '
                f'while lantern light crossed the wet stone steps and steady footsteps '
                f'measured every plank between the harbour bell and the north boathouse.</p>'
            )
        return (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>'
            f'{heading}</title></head><body><h1>{heading}</h1>'
            + ''.join(lines) + '</body></html>'
        )

    opf = (
        '<?xml version="1.0" encoding="utf-8"?>'
        '<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">'
        f'<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title>'
        '<dc:language>en</dc:language><dc:identifier id="id">newt-reflow-cc0</dc:identifier>'
        '</metadata><manifest>'
        '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'
        '<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>'
        '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>'
        '</manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>'
    )
    ncx = (
        '<?xml version="1.0" encoding="utf-8"?>'
        '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">'
        '<head><meta name="dtb:uid" content="newt-reflow-cc0"/></head>'
        f'<docTitle><text>{title}</text></docTitle><navMap>'
        '<navPoint id="n1" playOrder="1"><navLabel><text>Harbour Notes</text></navLabel>'
        '<content src="c1.xhtml"/></navPoint>'
        '<navPoint id="n2" playOrder="2"><navLabel><text>Lantern Watch</text></navLabel>'
        '<content src="c2.xhtml"/></navPoint>'
        '</navMap></ncx>'
    )
    container = (
        '<?xml version="1.0"?>'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
        '<rootfiles><rootfile full-path="content.opf" '
        'media-type="application/oebps-package+xml"/></rootfiles></container>'
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_STORED) as zf:
        zf.writestr('mimetype', 'application/epub+zip')
        zf.writestr('META-INF/container.xml', container)
        zf.writestr('content.opf', opf)
        zf.writestr('toc.ncx', ncx)
        zf.writestr('c1.xhtml', chapter_xhtml('Harbour Notes', chapter1_paras, 'A'))
        zf.writestr('c2.xhtml', chapter_xhtml('Lantern Watch', chapter2_paras, 'B'))


def parse_receipt(directory):
    text = (directory / 'receipt.txt').read_text()
    diag_line = [line for line in text.splitlines() if line.startswith('DIAG ')][-1]
    diag = {k: v for k, v in DIAG_PAIR.findall(diag_line)}
    orient = FRAME_ORIENT.findall(text)
    return diag, (orient[-1] if orient else None)


def durable_slots(directory):
    """Return (nwtr_dict, nwbr_dict) from the newest valid NRS2 slots on disk."""
    state = directory / 'card/Newt/State'
    def newest(path, magic):
        if not path.is_file():
            return None
        data = path.read_bytes()
        if len(data) != 4096 or data[:4] != b'NRS2':
            return None
        slots = [at for at in range(512, len(data), 512)
                 if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == magic]
        if not slots:
            return None
        at = max(slots, key=lambda pos: struct.unpack_from('<I', data, pos + 28)[0])
        seq = struct.unpack_from('<I', data, at + 28)[0]
        book = struct.unpack_from('<Q', data, at + 32)[0]
        if magic == b'NWTR':
            spine, character = struct.unpack_from('<II', data, at + 40)
            crc = struct.unpack_from('<I', data, at + 68)[0]
            return {'seq': seq, 'book': book, 'spine': spine, 'character': character, 'crc': crc}
        checkpoint, settings = struct.unpack_from('<II', data, at + 40)
        return {'seq': seq, 'book': book, 'checkpoint': checkpoint, 'settings': settings}

    nbs = sorted(state.glob('nb*.nrs'))
    return (newest(nbs[0], b'NWTR') if nbs else None), newest(state / 'newt-resume.nrs', b'NWBR')


def seed_world(binary, directory, *, orientation='portrait', with_sd_book=True, second_book=False):
    options = preview_driver.parse(['--reader-orientation', orientation])
    preview_driver.world(binary, directory, options)
    if with_sd_book:
        make_synthetic_epub(directory / 'card/Newt/Books/a-reflow.epub')
        if second_book:
            make_synthetic_epub(directory / 'card/Newt/Books/b-other.epub',
                                title='Second Volume', chapter1_paras=12, chapter2_paras=6)


def run_step(binary, directory, keys, boot='power-on', conf=None):
    if conf is not None:
        preview_driver.write_conf(directory, conf)
    preview_driver.fresh(directory)
    frames, _ = rust_preview.run(binary, directory, keys, boot)
    diag, orient = parse_receipt(directory)
    return frames[-1], diag, orient


@functools.cache
def play_sd(binary, keys, conf_items=()):
    with tempfile.TemporaryDirectory(prefix='newt-reflow-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=True)
        return run_step(binary, directory, keys, conf=dict(conf_items) if conf_items else None)


# Common sequences from Portrait Home with a-reflow.epub as first Library book.
OPEN_BOOK = (CONFIRM, CONFIRM)
TO_MID_PAGE = (*OPEN_BOOK, RIGHT, RIGHT, RIGHT)
P_TO_LCCW = settings_keys(cw=False, orient_presses=1)
LCCW_TO_P = settings_keys(cw=False, orient_presses=2)
P_TO_LCW = settings_keys(cw=False, orient_presses=2)
LCW_TO_P = settings_keys(cw=True, orient_presses=1)
SIZE_30_TO_22 = settings_keys(cw=False, size_presses=2)  # Px30 -> Px34 -> Px22
SIZE_22_TO_30 = settings_keys(cw=False, size_presses=3)  # Px22 -> Px24 -> Px27 -> Px30
SIZE_22_TO_34 = settings_keys(cw=False, size_presses=4)  # Px22 -> Px24 -> Px27 -> Px30 -> Px34


def orientation_roundtrip(binary):
    """Portrait -> Landscape Left -> Portrait -> Landscape Right -> Portrait lands byte-for-byte."""
    start_frame, start_diag, start_orient = play_sd(binary, TO_MID_PAGE)
    start_char = int(start_diag['actual_character'])
    if start_orient != 'portrait' or start_char == 0:
        return f'expected mid-chapter portrait page, got orient={start_orient} char={start_char}'

    _, lccw_diag, lccw_orient = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW))
    lccw_char = int(lccw_diag['actual_character'])
    if lccw_orient != 'landscape-ccw' or not (lccw_char < start_char):
        return f'expected landscape-ccw page starting before anchor ({start_char}), got orient={lccw_orient} char={lccw_char}'

    p2_frame, p2_diag, p2_orient = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW, *LCCW_TO_P))
    if p2_orient != 'portrait' or int(p2_diag['actual_character']) != start_char or p2_frame != start_frame:
        return f'Portrait -> Landscape Left -> Portrait drifted ({start_char} -> {p2_diag["actual_character"]})'

    _, lcw_diag, lcw_orient = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW, *LCCW_TO_P, *P_TO_LCW))
    if lcw_orient != 'landscape-cw' or int(lcw_diag['actual_character']) != lccw_char:
        return f'Landscape Right did not match Landscape Left ({lccw_char} vs {lcw_diag["actual_character"]}, orient={lcw_orient})'

    p3_frame, p3_diag, p3_orient = play_sd(
        binary, (*TO_MID_PAGE, *P_TO_LCCW, *LCCW_TO_P, *P_TO_LCW, *LCW_TO_P))
    if p3_orient != 'portrait' or int(p3_diag['actual_character']) != start_char or p3_frame != start_frame:
        return f'full Portrait -> L-Left -> Portrait -> L-Right -> Portrait drifted ({start_char} -> {p3_diag["actual_character"]})'


def text_size_roundtrip(binary):
    """Portrait Px30 -> Px22 -> Px30 lands byte-for-byte on the starting frame."""
    start_frame, start_diag, _ = play_sd(binary, TO_MID_PAGE)
    start_char = int(start_diag['actual_character'])
    _, small_diag, _ = play_sd(binary, (*TO_MID_PAGE, *SIZE_30_TO_22))
    small_char = int(small_diag['actual_character'])
    if not (small_char < start_char):
        return f'fixture Px22 page did not start strictly before Px30 anchor ({small_char} vs {start_char})'
    back_frame, back_diag, _ = play_sd(binary, (*TO_MID_PAGE, *SIZE_30_TO_22, *SIZE_22_TO_30))
    if int(back_diag['actual_character']) != start_char or back_frame != start_frame:
        return f'text-size round-trip drifted ({start_char} -> {back_diag["actual_character"]})'


def page_turn_clears_anchor(binary):
    """Turning a page clears the anchor so a later reflow follows the new page."""
    _, start_diag, _ = play_sd(binary, TO_MID_PAGE)
    start_char = int(start_diag['actual_character'])
    _, next_p_diag, _ = play_sd(binary, (*TO_MID_PAGE, RIGHT))
    next_p_char = int(next_p_diag['actual_character'])
    _, turned_diag, _ = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW, RIGHT, RIGHT))
    turned_char = int(turned_diag['actual_character'])
    if not (turned_char >= next_p_char > start_char):
        return f'two Next turns in landscape-ccw did not pass portrait page end ({turned_char} vs {next_p_char})'
    _, after_diag, _ = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW, RIGHT, RIGHT, *LCCW_TO_P))
    after_char = int(after_diag['actual_character'])
    if after_char <= start_char:
        return f'page turn did not clear anchor (expected > {start_char}, got {after_char})'


def boundary_press_keeps_anchor(binary):
    """A boundary press at the end of the book that does not move the page keeps the anchor."""
    # Jump to chapter 2 (Menu -> Chapters -> Down -> Confirm), then advance to the last page.
    to_ch2_end = (*OPEN_BOOK, CONFIRM, RIGHT, CONFIRM, RIGHT, CONFIRM, *(RIGHT,) * 8)
    last_frame, last_diag, _ = play_sd(binary, to_ch2_end)
    last_char = int(last_diag['actual_character'])
    last_spine = int(last_diag['actual_spine'])
    if last_spine != 1 or last_char == 0:
        return f'expected non-zero last page of spine 1, got spine={last_spine} char={last_char}'
    _, still_diag, _ = play_sd(binary, (*to_ch2_end, RIGHT))
    if int(still_diag['actual_character']) != last_char:
        return 'fixture did not reach the end-of-book boundary'
    _, lccw_diag, _ = play_sd(binary, (*to_ch2_end, *P_TO_LCCW))
    lccw_char = int(lccw_diag['actual_character'])
    if not (lccw_char < last_char):
        return f'expected landscape last page to start before portrait last page ({lccw_char} vs {last_char})'
    _, lccw_bound_diag, _ = play_sd(binary, (*to_ch2_end, *P_TO_LCCW, RIGHT))
    if int(lccw_bound_diag['actual_character']) != lccw_char:
        return 'landscape Next press moved instead of hitting end-of-book boundary'
    back_frame, back_diag, _ = play_sd(binary, (*to_ch2_end, *P_TO_LCCW, RIGHT, *LCCW_TO_P))
    if int(back_diag['actual_character']) != last_char or back_frame != last_frame:
        return f'boundary press lost the reflow anchor ({last_char} -> {back_diag["actual_character"]})'


def exit_sleep_and_failed_save(binary):
    """Exit+reopen, sleep+wake, book switch and failed NVS save respect anchor lifetime."""
    _, start_diag, _ = play_sd(binary, TO_MID_PAGE)
    start_char = int(start_diag['actual_character'])

    # 1. Exit to Home and reopen clears the RAM-only anchor.
    _, after_exit_diag, _ = play_sd(binary, (*TO_MID_PAGE, *P_TO_LCCW, BACK, BACK, *LCCW_TO_P))
    if not (int(after_exit_diag['actual_character']) < start_char):
        return f'exit and reopen did not clear anchor ({after_exit_diag["actual_character"]} vs {start_char})'

    # 2. Sleep and wake clears the RAM-only anchor.
    _, after_sleep_diag, _ = play_sd(
        binary, (*TO_MID_PAGE, *P_TO_LCCW, POWER, rust_preview.WAKE, *LCCW_TO_P))
    if not (int(after_sleep_diag['actual_character']) < start_char):
        return f'sleep and wake did not clear anchor ({after_sleep_diag["actual_character"]} vs {start_char})'

    # 3. Book switch clears the anchor.
    with tempfile.TemporaryDirectory(prefix='newt-reflow-switch-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=True, second_book=True)
        # Open book A, reflow to landscape-ccw (arms anchor), go Home -> Library -> open book B ->
        # Home -> Library -> reopen book A -> reflow back to portrait: anchor must be cleared.
        seq = (*TO_MID_PAGE, *P_TO_LCCW, BACK, CONFIRM, RIGHT, CONFIRM,
               BACK, CONFIRM, CONFIRM, *LCCW_TO_P)
        _, switch_diag, _ = run_step(binary, directory, seq)
        if not (int(switch_diag['actual_character']) < start_char):
            return f'book switch did not clear anchor ({switch_diag["actual_character"]} vs {start_char})'

    # 4. Failed NVS save before reflow does not change orientation or arm a stale anchor.
    with tempfile.TemporaryDirectory(prefix='newt-reflow-nvsfail-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=True)
        # Fail nvs-set:newt_prefs during Save, then Back out of Settings+Menu.
        keys_fail = (*TO_MID_PAGE, *P_TO_LCCW, BACK, BACK)
        _, fail_diag, fail_orient = run_step(
            binary, directory, keys_fail,
            conf={'store.refuse': 'nvs-set:newt_prefs'})
        if fail_orient != 'portrait' or int(fail_diag['actual_character']) != start_char:
            return f'failed NVS save altered state (orient={fail_orient} char={fail_diag["actual_character"]})'


def mid_reflow_power_cut(binary, reference=None):
    """Power cut mid-reflow restores to the page containing the anchor without drift,
    and the first ordinary save afterwards writes the rendered page's real char_start."""
    ref_binary = reference or binary
    _, start_diag, _ = play_sd(ref_binary, TO_MID_PAGE)
    start_char = int(start_diag['actual_character'])
    _, small_diag, _ = play_sd(ref_binary, (*TO_MID_PAGE, *SIZE_30_TO_22))
    small_char = int(small_diag['actual_character'])

    # Clean reference for Switch 2: Portrait(Px30) -> Portrait(Px22) -> Portrait(Px34).
    ref_frame, ref_diag, ref_orient = play_sd(ref_binary, (*TO_MID_PAGE, *SIZE_30_TO_22, *SIZE_22_TO_34))
    ref_char = int(ref_diag['actual_character'])
    assert ref_orient == 'portrait' and ref_char != start_char and small_char < ref_char < start_char, (
        f'fixture needs non-page-start anchor in Portrait Px34 above Px22 start '
        f'(small={small_char}, ref={ref_char}, anchor={start_char})'
    )

    with tempfile.TemporaryDirectory(prefix='newt-reflow-cut-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=True)
        seq = (*TO_MID_PAGE, *SIZE_30_TO_22, *SIZE_22_TO_34)
        # Refuse card-read on the book ONLY on the final Save press of Switch 2, after
        # preferences + ReflowCheckpoint commit nb*.nrs and NWBR, cutting open_selected_image.
        run_step(binary, directory, seq,
                 conf={'store.refuse': 'card-read:/newt-sd/Newt/Books/', 'store.refuse.after': str(len(seq))})
        mid_nb, mid_nwbr = durable_slots(directory)

        # Simulate power cut: lose RAM, retained RTC and resume picture; clear read fault.
        (directory / 'retained.bin').unlink(missing_ok=True)
        (directory / 'card/Newt/Cache/resume.pic').unlink(missing_ok=True)
        boot_frame, boot_diag, boot_orient = run_step(
            binary, directory, (), boot='power-on',
            conf={'store.refuse': '', 'store.refuse.after': '0'})
        if boot_diag.get('reader_open') != '1' or boot_orient != 'portrait':
            return f'cold boot after mid-reflow cut failed to reopen ({boot_diag}, orient={boot_orient})'
        if int(boot_diag['actual_character']) != ref_char or boot_frame != ref_frame:
            return f'cold boot after mid-reflow cut drifted ({boot_diag["actual_character"]} vs expected {ref_char})'
        if mid_nb is None or mid_nwbr is None:
            return f'missing durable slots mid-reflow ({mid_nb}, {mid_nwbr})'
        if mid_nb['character'] != start_char:
            return f'mid-reflow nb*.nrs did not hold anchor {start_char} (got {mid_nb})'
        if mid_nwbr['checkpoint'] != mid_nb['crc'] or (mid_nwbr['settings'] & 8) == 0:
            return f'mid-reflow NWBR did not point to anchored checkpoint ({mid_nb} vs {mid_nwbr})'

        # First ordinary save afterwards (30s idle via WAIT_MINUTE) writes real char_start and clears bit 3.
        save_frame, save_diag, _ = run_step(binary, directory, (WAIT_MINUTE,), boot='power-on')
        if int(save_diag['actual_character']) != ref_char or save_frame != ref_frame:
            return 'ordinary save after mid-reflow recovery changed the displayed page'
        after_nb, after_nwbr = durable_slots(directory)
        if after_nb is None or after_nwbr is None:
            return f'missing durable slots after ordinary save ({after_nb}, {after_nwbr})'
        if after_nb['character'] != ref_char:
            return f'first ordinary save did not write real char_start {ref_char} (got {after_nb})'
        if after_nwbr['checkpoint'] != after_nb['crc'] or (after_nwbr['settings'] & 8) != 0:
            return f'first ordinary save did not clear NWBR anchor bit ({after_nb} vs {after_nwbr})'


def sample_book_roundtrip_and_turn(binary):
    """Embedded sample book preserves position across round-trip and clears anchor on turn."""
    with tempfile.TemporaryDirectory(prefix='newt-reflow-sample-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=False)
        to_mid = (BACK, RIGHT, RIGHT)
        start_frame, start_diag, start_orient = run_step(binary, directory, to_mid)
        start_char = int(start_diag['actual_character'])
        if start_orient != 'portrait' or start_char == 0:
            return f'sample did not reach mid-chapter portrait page (orient={start_orient} char={start_char})'

    with tempfile.TemporaryDirectory(prefix='newt-reflow-sample-rt-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=False)
        _, lccw_diag, _ = run_step(binary, directory, (*to_mid, *P_TO_LCCW))
        lccw_char = int(lccw_diag['actual_character'])
        if not (lccw_char < start_char):
            return f'sample landscape page did not start before portrait anchor ({lccw_char} vs {start_char})'

    with tempfile.TemporaryDirectory(prefix='newt-reflow-sample-full-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=False)
        end_frame, end_diag, end_orient = run_step(
            binary, directory, (*to_mid, *P_TO_LCCW, *LCCW_TO_P, *P_TO_LCW, *LCW_TO_P))
        if end_orient != 'portrait' or int(end_diag['actual_character']) != start_char or end_frame != start_frame:
            return f'sample orientation round-trip drifted ({start_char} -> {end_diag["actual_character"]})'

    with tempfile.TemporaryDirectory(prefix='newt-reflow-sample-turn-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, orientation='portrait', with_sd_book=False)
        _, turn_diag, _ = run_step(binary, directory, (*to_mid, *P_TO_LCCW, RIGHT, RIGHT, *LCCW_TO_P))
        if not (int(turn_diag['actual_character']) > start_char):
            return f'sample page turn did not clear anchor ({turn_diag["actual_character"]} <= {start_char})'


NO_ANCHOR = (
    'reader_image.rs',
    '        let anchor = self.reflow_anchor.filter(|saved| saved.book == book)\n'
    '            .unwrap_or(ReflowAnchor { book, locator: fallback });\n'
    '        self.reflow_anchor = Some(anchor);\n'
    '        anchor.locator',
    '        let _ = (&self.reflow_anchor, book);\n'
    '        fallback',
)
NO_CLEAR_EXTERNAL_TURN = (
    'reader_image_action.rs',
    '                    if before.map(|p| (p.spine, p.page)) != after.map(|p| (p.spine, p.page)) {\n'
    '                        self.reflow_anchor = None;\n'
    '                    }',
    '                    let _ = (before, after);',
)
NO_CLEAR_SAMPLE_TURN = (
    'reader_image_sample_action.rs',
    '                    self.reflow_anchor = None;\n'
    '                    self.reading_refresh.page_changed();',
    '                    self.reading_refresh.page_changed();',
)
NO_REFLOW_NWBR = (
    'reader_book_run.rs',
    '                            let (destination_settings, anchor) = match reflow {\n'
    '                                Some((_, settings)) => (settings, true),\n'
    '                                None => (resources.settings, false),\n'
    '                            };\n'
    '                            checkpoint.destination(io, card, resources.pages, &opened_digest,\n'
    '                                destination_settings, true, anchor, revision.source_verified(), request);',
    '                            if commit_reading {\n'
    '                                checkpoint.destination(io, card, resources.pages, &opened_digest,\n'
    '                                    resources.settings, true, false, revision.source_verified(), request);\n'
    '                            }',
)

CASES = [
    (orientation_roundtrip, [(NO_ANCHOR, 'drifted')]),
    (text_size_roundtrip, [(NO_ANCHOR, 'drifted')]),
    (page_turn_clears_anchor, [(NO_CLEAR_EXTERNAL_TURN, 'did not clear anchor')]),
    (boundary_press_keeps_anchor, [(NO_ANCHOR, 'lost the reflow anchor')]),
    (exit_sleep_and_failed_save, []),
    (mid_reflow_power_cut, [
        (NO_REFLOW_NWBR, 'mid-reflow NWBR did not point to anchored checkpoint'),
        (NO_ANCHOR, 'cold boot after mid-reflow cut drifted'),
    ]),
    (sample_book_roundtrip_and_turn, [
        (NO_ANCHOR, 'sample orientation round-trip drifted'),
        (NO_CLEAR_SAMPLE_TURN, 'sample page turn did not clear anchor'),
    ]),
]


def run_case(case, binary, reference):
    return case(binary, reference) if case is mid_reflow_power_cut else case(binary)


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, _ in CASES:
        problem = run_case(case, binary, binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} reader reflow cases run the firmware (controls not in this tier)')
        return
    built = {}
    for case, controls in CASES:
        for edit, expected in controls:
            if edit not in built:
                built[edit] = rust_preview.control([edit], optimize=False)
            problem = run_case(case, built[edit], binary)
            assert problem is not None, f'{case.__name__}: passes with {edit[0]} changed'
            assert expected in problem, (
                f'{case.__name__}: unexpected failure with {edit[0]} changed: '
                f'expected {expected!r}, got {problem!r}'
            )
    print(f'PASS: {len(CASES)} reader reflow cases run the firmware; each control fails')


if __name__ == '__main__':
    main()
