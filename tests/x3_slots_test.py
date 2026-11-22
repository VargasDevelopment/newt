#!/usr/bin/env python3
"""The X3 app-slot identity rule (docs/UPDATES.md, "Slots"): tools/x3_slots.py
on the bench and main layouts, the layouts without another firmware, the
repair and every refusal."""
from pathlib import Path
import hashlib
import struct
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tests'))
import x3_slots
from x3_slots import (HEAD, NEWT_MARK, OTADATA_SIZE, SECTOR, SLOTS, STATE_ABORTED, STATE_INVALID, STATE_NEW,
                      STATE_PENDING, STATE_UNDEFINED, STATE_VALID, Refused)


def head(project, newt=False, magic=x3_slots.DESC_MAGIC, first=0xe9):
    data = bytearray(HEAD)
    data[0] = first
    struct.pack_into('<I', data, x3_slots.DESC_AT, magic)
    data[x3_slots.DESC_AT + 48:x3_slots.DESC_AT + 48 + len(project)] = project.encode()
    if newt:
        data[x3_slots.NEWT_MARK_AT:x3_slots.NEWT_MARK_AT + len(NEWT_MARK)] = NEWT_MARK
    return bytes(data)


# CrossInk, CrossPoint and stock (arduino-lib-builder, as Newt itself
# reported before it named itself) are all another firmware.
CROSSINK, CROSSPOINT, NEWT = head('CrossInk'), head('crosspoint-reader'), head('Newt', newt=True)
OTHER, EMPTY = head('arduino-lib-builder'), b'\xff' * HEAD
# Every kind, and the near misses a looser rule would accept.
VECTORS = {
    'crossink': CROSSINK, 'crosspoint': CROSSPOINT, 'newt': NEWT, 'other': OTHER, 'empty': EMPTY,
    'older-newt': head('arduino-lib-builder', newt=True), 'newt-named-crossink': head('CrossInk', newt=True),
    'bad-magic': head('CrossInk', magic=0x12345678), 'bad-header': head('CrossInk', first=0),
    'crossink-prefix': head('CrossInkX'), 'crossink-case': head('crossink'),
    'erased-but-one': b'\xfe' + b'\xff' * (HEAD - 1),
    'mark-off-by-one': head('arduino-lib-builder')[:x3_slots.NEWT_MARK_AT + 1] + NEWT_MARK
                       + bytes(HEAD - x3_slots.NEWT_MARK_AT - 1 - len(NEWT_MARK)),
}
KINDS = {'crossink': 'other', 'crosspoint': 'other', 'newt': 'newt', 'other': 'other', 'empty': 'empty',
         'older-newt': 'newt', 'newt-named-crossink': 'newt', 'bad-magic': 'invalid', 'bad-header': 'invalid',
         'crossink-prefix': 'other', 'crossink-case': 'other', 'erased-but-one': 'invalid', 'mark-off-by-one': 'other'}
# The names people see: only the display table's exact projects are named.
NAMES_SHOWN = {'crossink': 'CrossInk', 'crosspoint': 'CrossPoint', 'other': 'the other firmware',
               'crossink-prefix': 'the other firmware', 'crossink-case': 'the other firmware',
               'mark-off-by-one': 'the other firmware'}


def ota(*records):
    """Both selection sectors: each record (seq, state), or None when erased."""
    data = bytearray(b'\xff' * OTADATA_SIZE)
    for index, record in enumerate(records):
        if record:
            seq, state = record
            struct.pack_into('<I', data, index * SECTOR, seq)
            struct.pack_into('<II', data, index * SECTOR + 24, state, x3_slots.seq_crc(seq))
    return bytes(data)


BENCH = ota((5, STATE_VALID), (6, STATE_VALID))  # seq 6 boots app1 = Newt
MAIN = ota((5, STATE_VALID), (6, STATE_VALID))  # seq 6 boots app1 = the other firmware


def confirmed(slot):
    """Records whose highest, confirmed sequence boots `slot`."""
    return ota((5, STATE_VALID), (6, STATE_VALID)) if slot else ota((7, STATE_VALID), (6, STATE_VALID))


def torn(cut, image=NEWT):
    """The head after a cut while writing sector 0 last: the bytes before the
    cut, the rest still erased. cut 0 is an interrupted first erase."""
    return image[:cut] + b'\xff' * (HEAD - cut)


def one_crossink_decide(selection, heads):
    """decide() and selection_write() as of ff4174d0's one-CrossInk rule,
    with any other firmware in CrossInk's place: the reference every layout
    holding another firmware must still meet."""
    images = tuple(x3_slots.classify(head) for head in heads)
    other = [i for i, image in enumerate(images) if image.kind == 'other']
    x3_slots.refuse(len(other) == 1, "Both slots hold another firmware; neither is Newt's" if other else 'Neither')
    x3_slots.refuse(all(image.kind != 'invalid' for image in images), 'A slot holds an unreadable image')
    layout = x3_slots.Layout(other[0], 1 - other[0], x3_slots.boot_slot(selection), images)
    writes = []
    for slot in (0, 1):
        if layout.boot == slot:
            writes.append(None)
            continue
        current = x3_slots.boot_record(selection)
        seq = x3_slots.selections(selection)[current].seq + 1
        while (seq - 1) % 2 != slot:
            seq += 1
        entry = struct.pack('<I', seq) + b'\xff' * 20 + struct.pack('<II', STATE_NEW, x3_slots.seq_crc(seq))
        writes.append((x3_slots.OTADATA + (1 - current) * SECTOR, entry.ljust(SECTOR, b'\xff')))
    return layout, writes


def current_decide(selection, heads):
    layout = x3_slots.decide(selection, heads)
    return layout, [x3_slots.selection_write(layout, selection, slot) for slot in (0, 1)]


class SlotRule(unittest.TestCase):
    def test_kinds(self):
        for name, data in VECTORS.items():
            with self.subTest(name):
                self.assertEqual(x3_slots.classify(data).kind, KINDS[name])
                self.assertEqual(x3_slots.classify(data).name, NAMES_SHOWN.get(name, ''))

    def test_bench_layout(self):
        layout = x3_slots.decide(BENCH, [CROSSINK, NEWT])
        self.assertEqual((layout.other, layout.newt, layout.boot), (0, 1, 1))
        self.assertEqual(layout.newt_offset, SLOTS[1])
        self.assertIsNone(x3_slots.selection_write(layout, BENCH, layout.newt))
        offset, sector = x3_slots.selection_write(layout, BENCH, layout.other)
        self.assertEqual(offset, x3_slots.OTADATA)  # the record not followed
        after = bytearray(BENCH)
        after[:SECTOR] = sector
        self.assertEqual(x3_slots.boot_slot(bytes(after)), 0)
        self.assertEqual(x3_slots.selections(bytes(after))[0], x3_slots.Selection(7, STATE_NEW, True))

    def test_main_layout(self):
        layout = x3_slots.decide(MAIN, [NEWT, CROSSINK])
        self.assertEqual((layout.other, layout.newt, layout.boot), (1, 0, 1))
        offset, sector = x3_slots.selection_write(layout, MAIN, layout.newt)
        after = bytearray(MAIN)
        after[offset - x3_slots.OTADATA:offset - x3_slots.OTADATA + SECTOR] = sector
        self.assertEqual(x3_slots.boot_slot(bytes(after)), 0)
        self.assertEqual(struct.unpack_from('<I', sector)[0], 7)
        self.assertEqual(sector[4:24], b'\xff' * 20)

    def test_refusals(self):
        for name, selection, heads, message in (
                ('both CrossInk', BENCH, [CROSSINK, CROSSINK], 'Both slots hold another firmware'),
                ('two other firmwares', BENCH, [OTHER, CROSSPOINT], 'Both slots hold another firmware'),
                ('neither, empty boots', BENCH, [NEWT, EMPTY], 'app1, the slot that boots, does not hold Newt'),
                ('neither, both empty', BENCH, [EMPTY, EMPTY], 'does not hold Newt'),
                ('unknown image', BENCH, [CROSSINK, VECTORS['bad-magic']], 'unreadable image'),
                ('bad header', BENCH, [VECTORS['bad-header'], OTHER], 'unreadable image'),
                ('no selection', ota(None, None), [CROSSINK, NEWT], 'No valid OTA selection'),
                ('bad CRCs', bytes(b ^ (i % SECTOR == 28) for i, b in enumerate(BENCH)), [CROSSINK, NEWT],
                 'No valid OTA selection'),
                ('invalid and aborted', ota((5, STATE_INVALID), (6, STATE_ABORTED)), [CROSSINK, NEWT],
                 'No valid OTA selection'),
                ('short selection read', BENCH[:-1], [CROSSINK, NEWT], 'Incomplete OTA selection'),
                ('short head read', BENCH, [CROSSINK, NEWT[:-1]], 'Incomplete slot header'),
                ('one head', BENCH, [CROSSINK], 'both app slot headers')):
            with self.subTest(name), self.assertRaisesRegex(Refused, message):
                x3_slots.decide(selection, heads)

    def test_highest_usable_sequence_boots(self):
        for selection, slot in ((ota((5, STATE_VALID), (6, STATE_INVALID)), 0), (ota((9, STATE_NEW), None), 0),
                                (ota(None, (2, STATE_VALID)), 1), (ota((8, STATE_VALID), (3, STATE_VALID)), 1)):
            self.assertEqual(x3_slots.boot_slot(selection), slot)

    def test_pending_selection_keeps_the_confirmed_fallback(self):
        # Round trip 1: CrossInk confirmed seq 7; Newt's seq 8 booted but never
        # confirmed. The next boot aborts 8 and starts CrossInk, so selecting
        # Newt rewrites record 8 and never touches CrossInk's record 7. Any
        # other firmware gets the same.
        pending = ota((7, STATE_VALID), (8, STATE_PENDING))
        layout = x3_slots.decide(pending, [CROSSINK, NEWT])
        self.assertEqual(layout.boot, 0)
        self.assertIsNone(x3_slots.selection_write(layout, pending, layout.other))
        offset, sector = x3_slots.selection_write(layout, pending, layout.newt)
        self.assertEqual((offset, struct.unpack_from('<I', sector)[0]), (x3_slots.OTADATA + SECTOR, 8))
        # No confirmed fallback behind a pending record: no safe choice.
        for selection in (ota(None, (8, STATE_PENDING)), ota((7, STATE_PENDING), (8, STATE_PENDING)),
                          ota((7, STATE_ABORTED), (8, STATE_PENDING))):
            with self.subTest(selection=selection[:4].hex()), self.assertRaisesRegex(Refused, 'no confirmed fallback'):
                x3_slots.decide(selection, [CROSSINK, NEWT])

    def test_writes_only_newt_slot(self):
        bench = x3_slots.decide(BENCH, [CROSSINK, NEWT])
        x3_slots.require_newt_write(bench, SLOTS[1], 4096)
        for offset, length in ((SLOTS[0], 4096), (0, 4096), (SLOTS[1], 0), (SLOTS[1], x3_slots.SLOT_SIZE + 1)):
            with self.subTest(offset=offset, length=length), self.assertRaises(Refused):
                x3_slots.require_newt_write(bench, offset, length)
        main = x3_slots.decide(MAIN, [NEWT, OTHER])
        x3_slots.require_newt_write(main, SLOTS[0], 4096)
        with self.assertRaisesRegex(Refused, 'does not hold the other firmware'):
            x3_slots.require_newt_write(main, SLOTS[1], 4096)
        empty = x3_slots.decide(BENCH, [CROSSPOINT, EMPTY])
        x3_slots.require_newt_write(empty, SLOTS[1], 4096)

    def test_other_firmware_layouts_keep_the_crossink_rule(self):
        # Every pair holding another firmware, under every selection: the
        # same layout or refusal, and the same selection sector for either slot.
        # 'exhausted' departs from the reference on purpose (#241); the
        # selection tests hold every copy of the rule to it.
        for a, b in ((a, b) for a in VECTORS for b in VECTORS if 'other' in (KINDS[a], KINDS[b])):
            for name, selection in ((n, s) for n, s in SELECTIONS.items() if n != 'exhausted'):
                outcomes = []
                for rule in (one_crossink_decide, current_decide):
                    try:
                        outcomes.append(rule(selection, [VECTORS[a], VECTORS[b]]))
                    except Refused as refusal:
                        outcomes.append(str(refusal))
                with self.subTest(a=a, b=b, selection=name):
                    self.assertEqual(outcomes[1], outcomes[0])

    def test_without_another_firmware_only_the_slot_that_does_not_boot_is_written(self):
        for running in (0, 1):
            records = confirmed(running)
            for kind, other in (('newt', NEWT), ('older newt', VECTORS['older-newt']), ('empty', EMPTY)):
                with self.subTest(running=running, other=kind):
                    heads = [NEWT, NEWT]
                    heads[1 - running] = other
                    layout = x3_slots.decide(records, heads)
                    self.assertEqual((layout.other, layout.newt, layout.boot), (None, 1 - running, running))
                    self.assertEqual((layout.newt_offset, layout.other_offset), (SLOTS[1 - running], None))
                    self.assertEqual(layout.report(records)['other'], None)
                    with self.assertRaisesRegex(Refused, 'the slot that does not boot'):
                        x3_slots.require_newt_write(layout, SLOTS[running], 4096)
                    x3_slots.require_newt_write(layout, layout.newt_offset, 4096)
                    # A fresh new record for the written slot, in the record not followed.
                    offset, sector = x3_slots.selection_write(layout, records, layout.newt)
                    self.assertEqual(offset, x3_slots.OTADATA + (1 - x3_slots.boot_record(records)) * SECTOR)
                    at = offset - x3_slots.OTADATA
                    after = bytearray(records)
                    after[at:at + SECTOR] = sector
                    self.assertEqual(x3_slots.boot_slot(bytes(after)), 1 - running)
                    self.assertEqual(x3_slots.selections(bytes(after))[at // SECTOR].state, STATE_NEW)
                    self.assertIsNone(x3_slots.selection_write(layout, records, running))
                    with self.assertRaisesRegex(Refused, 'Unknown slot'):  # layout.other is None here
                        x3_slots.selection_write(layout, records, None)

    def test_install_selection_never_returns_to_the_written_slot(self):
        # In place beside another firmware: the bench (Newt app1) and the main
        # unit (Newt app0). It is selected, settled, before the app changes;
        # Newt's fresh record then reuses the record its old selection held.
        for newt in (1, 0):
            with self.subTest(newt=newt):
                records = confirmed(newt)
                heads = [CROSSINK, CROSSINK]
                heads[newt] = NEWT
                layout = x3_slots.decide(records, heads)
                fallback, fresh = x3_slots.install_selection(layout, records)
                during = x3_slots.applied(records, fallback)
                self.assertEqual(x3_slots.boot_slot(during), 1 - newt)
                self.assertEqual(x3_slots.selections(during)[x3_slots.boot_record(during)].state, STATE_VALID)
                self.assertEqual(fresh[0], x3_slots.OTADATA + x3_slots.boot_record(records) * SECTOR)
                after = x3_slots.applied(during, fresh)
                self.assertEqual(x3_slots.selections(after)[x3_slots.boot_record(after)].state, STATE_NEW)
                self.assertEqual((x3_slots.boot_slot(after), x3_slots.fallback_slot(after)), (newt, 1 - newt))
        # Otherwise one fresh record; the running image stays the fallback.
        for heads, running in (([NEWT, NEWT], 0), ([NEWT, NEWT], 1), ([CROSSINK, NEWT], 0), ([NEWT, CROSSINK], 1)):
            with self.subTest(heads=[x3_slots.classify(head).kind for head in heads], running=running):
                records = confirmed(running)
                layout = x3_slots.decide(records, heads)
                fallback, fresh = x3_slots.install_selection(layout, records)
                self.assertIsNone(fallback)
                self.assertEqual(fresh, x3_slots.selection_write(layout, records, layout.newt))
                after = x3_slots.applied(records, fresh)
                self.assertEqual((x3_slots.boot_slot(after), x3_slots.fallback_slot(after)), (1 - running, running))
        # A settled selection is its own fallback; a new one needs a settled other record.
        self.assertEqual(x3_slots.fallback_slot(confirmed(1)), 1)
        with self.assertRaisesRegex(Refused, 'no settled record'):
            x3_slots.fallback_slot(ota((5, STATE_NEW), (6, STATE_NEW)))

    def test_without_another_firmware_the_running_newt_must_be_selected_and_confirmed(self):
        for running in (0, 1):
            heads = [NEWT, NEWT]
            seq = 6 if running else 5  # the slot (seq - 1) % 2
            for name, selection, message in (
                    ('new', ota((seq, STATE_NEW), None), r'not confirmed \(new\)'),
                    ('pending above', ota((seq, STATE_VALID), (seq + 1, STATE_PENDING)), 'still pending'),
                    ('other slot new', ota((seq, STATE_VALID), (seq + 1, STATE_NEW)), 'not confirmed')):
                with self.subTest(running=running, records=name), self.assertRaisesRegex(Refused, message):
                    x3_slots.decide(selection, heads)
            # Undefined is how records read without rollback: settled.
            self.assertEqual(x3_slots.decide(ota((seq, STATE_UNDEFINED), None), heads).newt, 1 - running)
            # A pending selection behind the boot record is no newer: allowed.
            self.assertEqual(x3_slots.decide(ota((seq + 2, STATE_VALID), (seq + 1, STATE_PENDING)), heads).boot,
                             running)

    def test_repair_only_a_damaged_slot_beside_the_running_newt(self):
        damaged = {'torn after one byte': torn(1), 'torn before the magic': torn(x3_slots.DESC_AT + 3),
                   'partly programmed': bytes([NEWT[0] | 0x10]) + NEWT[1:], 'bad magic': VECTORS['bad-magic'],
                   'partly erased': torn(0)[:0x40] + NEWT[0x40:]}
        for running in (0, 1):
            records = confirmed(running)
            for name, head in damaged.items():
                heads = [NEWT, NEWT]
                heads[1 - running] = head
                with self.subTest(running=running, damaged=name):
                    self.assertEqual(x3_slots.classify(head).kind, 'invalid')
                    with self.assertRaisesRegex(Refused, f'app{1 - running} holds an unreadable.*--repair-invalid'):
                        x3_slots.decide(records, heads)
                    layout = x3_slots.decide(records, heads, repair=True)
                    self.assertEqual((layout.other, layout.newt, layout.boot), (None, 1 - running, running))
                    x3_slots.require_newt_write(layout, layout.newt_offset, 4096)
            # Cut before its first byte or by an interrupted first erase, the
            # head reads erased: an ordinary install, and nothing to repair.
            heads = [NEWT, NEWT]
            heads[1 - running] = torn(0)
            self.assertEqual(x3_slots.decide(records, heads).images[1 - running].kind, 'empty')
            with self.assertRaisesRegex(Refused, 'nothing to repair'):
                x3_slots.decide(records, heads, repair=True)
            # The survivor must boot, be selected and be confirmed.
            seq = 6 if running else 5
            heads[1 - running] = torn(1)
            for name, selection, message in (
                    ('damaged slot boots', ota((seq, STATE_VALID), (seq + 1, STATE_VALID)), 'does not hold Newt'),
                    ('damaged slot pending', ota((seq, STATE_VALID), (seq + 1, STATE_PENDING)), 'still pending'),
                    ('survivor new', ota((seq, STATE_NEW), None), 'not confirmed')):
                with self.subTest(running=running, refusal=name), self.assertRaisesRegex(Refused, message):
                    x3_slots.decide(selection, heads, repair=True)
        for heads in ([CROSSINK, NEWT], [CROSSINK, torn(1)]):
            with self.assertRaises(Refused):
                x3_slots.decide(BENCH, heads, repair=True)


# OTA selection records (otadata) the slot-rule cases decide on.
SELECTIONS = {
    'bench': BENCH, 'pending': ota((5, STATE_VALID), (6, STATE_PENDING)), 'new': ota((9, STATE_NEW), None),
    'crossink-confirmed-newt-pending': ota((7, STATE_VALID), (8, STATE_PENDING)),
    'pending-first': ota((9, STATE_PENDING), (8, STATE_VALID)), 'pending-alone': ota(None, (8, STATE_PENDING)),
    'both-pending': ota((7, STATE_PENDING), (8, STATE_PENDING)),
    'tie': ota((4, STATE_VALID), (4, STATE_VALID)), 'second-invalid': ota((5, STATE_VALID), (6, STATE_INVALID)),
    'first-only': ota((2, STATE_VALID), None), 'second-only': ota(None, (3, STATE_VALID)),
    'none': ota(None, None), 'aborted': ota((5, STATE_ABORTED), (6, STATE_ABORTED)),
    'bad-crc': bytes(b ^ (i % SECTOR == 28) for i, b in enumerate(BENCH)), 'zero': ota((0, STATE_VALID), None),
    # #241: app1 boots at 0xfffffffe, so only 0xffffffff, an erased record, would select app0.
    'exhausted': ota((0xfffffffd, STATE_VALID), (0xfffffffe, STATE_VALID)),
    'last-sequence': ota((0xfffffffd, STATE_VALID), (0xfffffffc, STATE_VALID)),
}


def images():
    """(name, bytes, slot bytes, intact) for the shared image check. Each
    corruption but the digest's own is resealed, so one guard alone rejects it."""
    from bench_flash_fixtures import app_bytes
    valid = app_bytes('v1.4.0.1', project='CrossInk', newt=False)
    body = len(valid) - 32

    def changed(at, value=None, pack=None):
        data = bytearray(valid)
        if pack:
            struct.pack_into('<I', data, at, pack)
        else:
            data[at] = data[at] ^ 1 if value is None else value
        return bytes(data[:body]) + hashlib.sha256(data[:body]).digest()

    def segments(count):
        # Otherwise well formed: `count` segments of 16 equal bytes, which
        # cancel out of the XOR checksum, so it stays at its 0xef seed. Each
        # maps at its data's offset (0x20 + 24 per segment before it).
        header = bytearray(valid[:24])
        header[1] = count
        data = bytes(header) + b''.join(struct.pack('<II', 0x3c000020 + 24 * index, 16) + bytes([index]) * 16
                                        for index in range(count))
        data += b'\0' * (15 - len(data) % 16) + b'\xef'
        return data + hashlib.sha256(data).digest()

    beyond = changed(28, pack=len(valid))

    def length_off_by_one():
        # One byte more in the only segment, its checksum and digest redone,
        # so only the length's alignment is wrong.
        segment = struct.unpack_from('<I', valid, 28)[0]
        data = bytearray(valid[:24]) + struct.pack('<II', 0x3c000020, segment + 1) + valid[32:32 + segment] + b'\0'
        data += b'\0' * (15 - len(data) % 16) + bytes([valid[body - 1]])
        return bytes(data) + hashlib.sha256(data).digest()
    digest = bytearray(valid)
    digest[-1] ^= 1
    padded = valid.ljust(4096, b'\xff')
    return [('valid', valid, len(valid), True), ('in a larger slot', padded, len(padded), True),
            ('newt', app_bytes('a1' * 6), 8192, True), ('magic', changed(0, 0xe8), 8192, False),
            ('chip', changed(12, 9), 8192, False), ('no digest appended', changed(23, 0), 8192, True),
            ('digest flag 2', changed(23, 2), 8192, False),
            ('segment byte', changed(0x40), 8192, False), ('checksum', changed(body - 1), 8192, False),
            ('digest', bytes(digest), 8192, False), ('segment beyond slot', beyond, len(beyond), False),
            ('16 segments', segments(16), 8192, True), ('17 segments', segments(17), 8192, False),
            ('no segments', segments(0), 8192, False), ('a whole padding block', segments(1), 8192, True),
            ('digest beyond slot', valid, len(valid) - 1, False), ('erased', b'\xff' * 4096, 4096, False),
            # #168: a card file cut off inside its segment, its size as the slot.
            ('truncated', valid[:len(valid) // 2], len(valid) // 2, False),
            # Astra P2-1: the bootloader refuses both (verify_segment_header).
            ('mapped address off its data', changed(24, pack=0x3c000024), 8192, False),
            ('segment length not a multiple of 4', length_off_by_one(), 8192, False),
            # #368: a chosen file may end in erased padding, never in other bytes.
            ('erased padding', padded[:len(valid) + 16], len(valid) + 16, True),
            ('extra data', valid + b'\xff' * 3 + b'\0' + b'\xff' * 12, len(valid) + 16, True)]


if __name__ == '__main__':
    unittest.main(verbosity=2)
