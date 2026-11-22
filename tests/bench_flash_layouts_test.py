#!/usr/bin/env python3
"""bench_flash on every layout an SD install can leave (docs/082 section 8,
"Recovery"): Newt+Newt, Newt+Empty and Newt+Invalid with Newt running from
app0 and from app1, the repair and each of its refusals, and another firmware
(CrossInk, or any app without Newt's mark) installed from the card into
Newt's old slot. On each, what a failed first boot
returns to. Same scratch harness, CLI and guard-removal runner as
tests/bench_flash_test.py."""
import json
import sys
from types import SimpleNamespace
from unittest.mock import patch

import bench_flash_test as bench  # parses --tool-root, --single and --negative-controls
from bench_flash_fixtures import BUILD, CROSSINK, Fake, app_bytes, select
from bench_install_other_cases import InstallOtherCases
import bench_device as transport
import x3_slots
from x3_slots import OTADATA, SLOTS, SLOT_SIZE, STATE_NEW, STATE_PENDING, STATE_VALID

OLD = app_bytes('older-newt')  # the running Newt, the survivor
OTHER = app_bytes('V6.5.5', newt=False, elf='22' * 32)  # another firmware, named by no table entry
NEW = app_bytes('next-newt', extra=6000)  # two sectors: a torn first one leaves the second written


def confirmed(running):
    """Two valid records; the highest boots `running`."""
    return ((5, STATE_VALID), (6, STATE_VALID)) if running else ((7, STATE_VALID), (6, STATE_VALID))


def torn(cut):
    """The other slot after power loss while the installer wrote sector 0
    last: the bytes before the cut, the rest of that sector still erased."""
    data = bytearray(NEW)
    data[cut:4096] = b'\xff' * (4096 - cut)
    return bytes(data)


def erased_head(image, length=4096):
    """An interrupted erase of the first block: its start reads erased."""
    return b'\xff' * length + image[length:]


class LayoutTests(InstallOtherCases, bench.BenchHarness):
    def unit(self, slots, records, name):
        """A fresh unit: `slots` holds each slot's bytes (None for erased) and
        `records` each selection record (seq, state) or None."""
        self.device = Fake(self.image.table)
        memory = self.device.memory
        memory[SLOTS[0]:SLOTS[1] + SLOT_SIZE] = b'\xff' * (SLOTS[1] + SLOT_SIZE - SLOTS[0])
        for offset, data in zip(SLOTS, slots):
            if data is not None:
                memory[offset:offset + len(data)] = data
        memory[OTADATA:OTADATA + 0x2000] = b'\xff' * 0x2000
        for index, record in enumerate(records):
            if record:
                select(memory, OTADATA + index * 0x1000, *record)
        self.shared = self.root / ('units-' + name)
        self.session = self.root / ('session-' + name)

    def ota(self):
        return bytes(self.device.memory[OTADATA:OTADATA + 0x2000])

    def slot(self, index):
        return bytes(self.device.memory[SLOTS[index]:SLOTS[index] + SLOT_SIZE])

    def assert_installed_only(self, target, before):
        """One write of the target slot and one of the record the next boot
        did not follow, then one boot that confirmed the new selection."""
        records = x3_slots.selections(before)
        sector = OTADATA + (1 - x3_slots.boot_record(before)) * 0x1000
        self.assertEqual(self.device.writes, [(SLOTS[target], 4096), (sector, 4096)])
        self.assertEqual(self.device.boots, [target])
        self.assertEqual(self.device.hellos, [BUILD])
        after = x3_slots.selections(self.ota())
        self.assertEqual((x3_slots.boot_slot(self.ota()), after[(sector - OTADATA) // 0x1000].state),
                         (target, STATE_VALID))
        self.assertEqual(after[x3_slots.boot_record(before)], records[x3_slots.boot_record(before)])
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_newt_newt_installs_the_other_slot_with_a_session_each(self):
        for running in (0, 1):
            with self.subTest(running=running):
                self.unit((OLD, OLD), confirmed(running), f'newt-newt-{running}')
                before, survivor = self.ota(), self.slot(running)
                self.invoke()
                self.assert_installed_only(1 - running, before)
                self.assertTrue(self.slot(running) == survivor, 'The running Newt changed')
                # The new build now boots, so the writable slot moved.
                self.device.writes.clear()
                self.refused_untouched('writable slot differs from the session; start a new session')
                self.device.boots.clear()
                self.device.hellos.clear()
                before = self.ota()
                self.invoke(session=self.session.with_name(self.session.name + '-next'))
                self.assert_installed_only(running, before)

    def test_newt_empty_installs_the_other_slot(self):
        for running in (0, 1):
            for name, other in (('empty', None), ('erased-head', erased_head(NEW))):
                with self.subTest(running=running, other=name):
                    slots = [OLD, OLD]
                    slots[1 - running] = other
                    self.unit(slots, confirmed(running), f'{name}-{running}')
                    before, survivor = self.ota(), self.slot(running)
                    self.invoke()
                    self.assert_installed_only(1 - running, before)
                    self.assertTrue(self.slot(running) == survivor, 'The running Newt changed')

    def assert_in_place(self, newt, before):
        """Beside another firmware, the running Newt's slot rewritten: a settled
        record for that firmware into the record not followed, the app, then Newt's
        fresh record into the one that was followed."""
        followed = OTADATA + x3_slots.boot_record(before) * 0x1000
        other = OTADATA + 0x1000 - (followed - OTADATA)
        self.assertEqual(self.device.writes, [(other, 4096), (SLOTS[newt], 4096), (followed, 4096)])

    def test_other_firmware_in_newts_old_slot_is_kept(self):
        # After SD updates moved Newt, Newt's card install put another
        # firmware into the slot Newt used to run from: the one-other rule
        # writes only Newt's slot, whichever firmware it is.
        for running in (0, 1):
            for name, firmware in (('crossink', CROSSINK), ('unnamed', OTHER)):
                with self.subTest(running=running, firmware=name):
                    slots = [firmware, firmware]
                    slots[running] = OLD
                    self.unit(slots, confirmed(running), f'{name}-back-{running}')
                    before, other = self.ota(), self.slot(1 - running)
                    self.invoke()
                    self.assert_in_place(running, before)
                    self.assertEqual(self.device.boots, [running])
                    self.assertTrue(self.slot(1 - running) == other, 'The other firmware changed')
                    self.device.writes.clear()
                    self.invoke(action='boot-other')
                    self.assertEqual(len(self.device.writes), 1)
                    self.assertEqual(x3_slots.boot_slot(self.ota()), 1 - running)

    def test_in_place_install_returns_to_the_other_firmware_when_its_first_boot_fails(self):
        # USB rewrites the running Newt beside CrossInk: the bench (CrossInk
        # app0, Newt app1) and the main unit (Newt app0, CrossInk app1).
        for newt in (1, 0):
            for confirms in (True, False):
                with self.subTest(newt=newt, confirms=confirms):
                    slots = [CROSSINK, CROSSINK]
                    slots[newt] = OLD
                    self.unit(slots, confirmed(newt), f'in-place-{newt}-{confirms}')
                    before, crossink = self.ota(), self.slot(1 - newt)
                    self.device.confirms = confirms
                    if confirms:
                        self.invoke()
                        receipt = json.loads(sorted(self.session.glob('run-*/receipt.json'))[-1].read_text())
                        self.assertEqual(receipt['result']['first_boot_fallback'], f'app{1 - newt} (other)')
                    else:
                        with self.assertRaisesRegex(ValueError, 'timed out'):
                            self.invoke()
                        self.assertTrue((self.unit_dir / 'STOP').exists())
                    self.assert_in_place(newt, before)
                    self.assertTrue(self.slot(1 - newt) == crossink, 'CrossInk changed')
                    self.assertEqual((self.device.boots, self.device.hellos), ([newt], [BUILD]))
                    # The next power-on: a confirmed build keeps booting; an
                    # unconfirmed one is aborted for CrossInk.
                    self.device.close()
                    self.device.boot()
                    self.assertEqual(self.device.boots, [newt, newt if confirms else 1 - newt])

    def test_a_failed_first_boot_returns_to_the_image_that_ran(self):
        # Without an in-place write the followed record stays the fallback:
        # the running Newt without another firmware, or the running other firmware.
        for name, slots, running, kind in (('newt-newt', (OLD, OLD), 0, 'newt'), ('newt-newt', (OLD, OLD), 1, 'newt'),
                                           ('other-runs', (CROSSINK, OLD), 0, 'other'),
                                           ('other-runs', (OLD, OTHER), 1, 'other')):
            with self.subTest(layout=name, running=running):
                self.unit(slots, confirmed(running), f'returns-{name}-{running}')
                before = self.ota()
                self.device.confirms = False
                with self.assertRaisesRegex(ValueError, 'timed out'):
                    self.invoke()
                sector = OTADATA + (1 - x3_slots.boot_record(before)) * 0x1000
                self.assertEqual(self.device.writes, [(SLOTS[1 - running], 4096), (sector, 4096)])
                self.device.close()
                self.device.boot()
                self.assertEqual(self.device.boots, [1 - running, running])
                self.assertEqual(x3_slots.classify(self.slot(running)[:x3_slots.HEAD]).kind, kind)

    def test_in_place_install_writes_no_app_byte_before_its_fallback_is_proven(self):
        self.unit((CROSSINK, OLD), confirmed(1), 'fallback-unproven')
        old = self.slot(1)
        write = self.device.write
        def torn_record(layout, offset, payload):
            write(layout, offset, payload)
            self.device.memory[offset + 24] ^= 1  # the record's state reads otherwise
        with patch.object(self.device, 'write', side_effect=torn_record), \
                self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke()
        self.assertEqual(self.device.writes, [(OTADATA, 4096)])
        self.assertTrue(self.slot(1) == old, 'The app slot changed')
        self.assertEqual(self.device.resets, 0)

    def test_in_place_install_refuses_another_firmware_that_fails_the_image_check(self):
        broken = bytearray(CROSSINK)
        broken[0x210] ^= 1  # past the head: the slot still reads as another firmware
        self.unit((bytes(broken), OLD), confirmed(1), 'broken-other')
        self.refused_untouched(r'app0 \(other\), the image a failed first boot returns to, fails the image check')

    def test_damaged_slot_repair_writes_only_that_slot(self):
        damaged = {'torn after one byte': torn(1), 'torn in the descriptor': torn(0x22),
                   'partly erased head': erased_head(OLD, 0x40)}
        for running in (0, 1):
            for name, data in damaged.items():
                with self.subTest(running=running, damaged=name):
                    slots = [OLD, OLD]
                    slots[1 - running] = data
                    self.unit(slots, confirmed(running), f'repair-{running}-{name.replace(" ", "-")}')
                    self.assertEqual(x3_slots.classify(data[:x3_slots.HEAD]).kind, 'invalid')
                    # The installer never selected the damaged slot: the running image still boots.
                    self.device.boot()
                    self.assertEqual(self.device.boots, [running])
                    self.device.boots.clear()
                    before, survivor = self.ota(), self.slot(running)
                    self.refused_untouched(f'app{1 - running} holds an unreadable.*--repair-invalid')
                    self.invoke(repair=True)
                    self.assert_installed_only(1 - running, before)
                    self.assertTrue(self.slot(running) == survivor, 'The survivor changed')
                    receipt = json.loads(sorted(self.session.glob('run-*/receipt.json'))[-1].read_text())
                    self.assertIn('fallback-image', [step['step'] for step in receipt['steps']])
        # A cut before the first byte leaves the head erased: an ordinary install.
        self.unit((OLD, torn(0)), confirmed(0), 'torn-at-zero')
        self.refused_untouched('nothing to repair', repair=True)
        self.invoke()
        self.assertEqual(self.device.writes[0], (SLOTS[1], 4096))

    def repair_refused(self, records, message, name, running=1):
        slots = [OLD, OLD]
        slots[1 - running] = torn(1)
        self.unit(slots, records, name)
        self.refused_untouched(message, repair=True)

    def test_repair_refuses_when_the_damaged_slot_boots(self):
        # Records select the damaged slot: the survivor is not what boots, and
        # the other slot is the only Newt, so no install may write it either.
        self.unit((OLD, torn(1)), ((5, STATE_VALID), (6, STATE_VALID)), 'damaged-boots')
        message = 'app1, the slot that boots, does not hold Newt'
        self.refused_untouched(message)
        self.refused_untouched(message, repair=True)

    def test_repair_refuses_when_a_newer_selection_is_pending(self):
        self.repair_refused(((7, STATE_PENDING), (6, STATE_VALID)), 'A newer selection is still pending', 'pending')

    def test_repair_refuses_an_unconfirmed_survivor(self):
        self.repair_refused((None, (6, STATE_NEW)), r'selection of app1 is not confirmed \(new\)', 'unconfirmed')

    def test_repair_refuses_a_survivor_that_fails_the_image_check(self):
        survivor = bytearray(OLD)
        survivor[0x210] ^= 1  # in the segment past the head: the slot still reads as Newt
        self.unit((torn(1), bytes(survivor)), confirmed(1), 'broken-survivor')
        self.refused_untouched(r'app1 \(newt\), the image a failed first boot returns to, fails', repair=True)

    def test_install_refuses_when_only_the_reserved_sequence_selects_the_spare(self):
        # #241: app1 boots at 0xfffffffe; app0 would need 0xffffffff, an erased record.
        self.unit((OLD, OLD), ((0xfffffffd, STATE_VALID), (0xfffffffe, STATE_VALID)), 'exhausted')
        self.refused_untouched('No OTA sequence above 0xfffffffe selects app0')

    def test_repair_refuses_when_the_survivor_differs_from_the_backup(self):
        self.unit((torn(1), OLD), confirmed(1), 'survivor-drift')
        self.device.flip_before = SLOTS[1] + 0x300  # after the backup, before any write
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke(repair=True)
        self.assertEqual(self.device.writes, [])
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_repair_selects_only_a_proven_slot(self):
        self.unit((torn(1), OLD), confirmed(1), 'unproven')
        before = self.ota()
        self.device.corrupt_payload = True
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke(repair=True)
        self.assertEqual(self.device.writes, [(SLOTS[0], 4096)])
        self.assertEqual((self.ota(), self.device.resets), (before, 0))

    def test_boot_newt_selects_the_slot_holding_the_candidate(self):
        for running in (0, 1):
            with self.subTest(running=running):
                slots = [OLD, OLD]
                slots[1 - running] = self.image.image
                self.unit(slots, confirmed(running), f'boot-newt-{running}')
                before = self.ota()
                self.invoke(action='boot-newt')
                sector = OTADATA + (1 - x3_slots.boot_record(before)) * 0x1000
                self.assertEqual(self.device.writes, [(sector, 4096)])
                self.assertEqual((self.device.boots, x3_slots.boot_slot(self.ota())), ([1 - running], 1 - running))
                # Now the candidate boots: nothing to write (a new session, as the slots moved).
                self.invoke(action='boot-newt', session=self.session.with_name(self.session.name + '-next'))
                self.assertEqual(len(self.device.writes), 1)
        self.unit((OLD, OLD), confirmed(1), 'boot-newt-missing')
        self.refused_untouched('Neither Newt slot holds this candidate', action='boot-newt')

    def test_boot_other_needs_another_firmware(self):
        self.unit((OLD, OLD), confirmed(1), 'no-other')
        self.refused_untouched('No slot holds another firmware', action='boot-other')
        self.assertEqual(self.full_reads(), [])

    def test_transport_never_writes_the_newt_that_boots(self):
        written = []
        esptool = SimpleNamespace(loader=SimpleNamespace(),
                                  cmds=SimpleNamespace(write_flash=lambda esp, regions, **kw: written.extend(regions)))
        for running in (0, 1):
            self.unit((OLD, OLD), confirmed(running), f'transport-{running}')
            device = transport.Device(bench.IDENTITY)
            device.esp = SimpleNamespace()
            with patch.dict(sys.modules, esptool=esptool), self.subTest(running=running):
                with self.assertRaisesRegex(ValueError, 'Writes go only to the writable app slot'):
                    device.write(self.bench_layout(), SLOTS[running], b'\xff' * 4096)
                device.write(self.bench_layout(), SLOTS[1 - running], b'\xff' * 4096)
        self.assertEqual([offset for offset, _ in written], [SLOTS[1], SLOTS[0]])


CONTROLS = [
    ('x3_slots.py', "    refuse(images[boot].kind == 'newt',", '    refuse(True,',
     'test_repair_refuses_when_the_damaged_slot_boots'),
    ('x3_slots.py', '    refuse(current.seq == max(record.seq for record in records if record.usable),',
     '    refuse(True,', 'test_repair_refuses_when_a_newer_selection_is_pending'),
    ('x3_slots.py', '    refuse(current.state in (STATE_VALID, STATE_UNDEFINED),', '    refuse(True,',
     'test_repair_refuses_an_unconfirmed_survivor'),
    ('bench_flash.py', '            x3_slots.image_intact(backup[kept:kept + SLOT_SIZE], SLOT_SIZE),', '            True,',
     'test_repair_refuses_a_survivor_that_fails_the_image_check'),
    ('bench_flash.py', '            x3_slots.image_intact(backup[kept:kept + SLOT_SIZE], SLOT_SIZE),', '            True,',
     'test_in_place_install_refuses_another_firmware_that_fails_the_image_check'),
    # 314d5574's selection: nothing new for a slot that already boots.
    ('bench_flash.py', """    fallback, selection = (None, x3_slots.selection_write(layout, ota, target)) if boot_only else \\
        x3_slots.install_selection(layout, ota)""", '    fallback, selection = None, x3_slots.selection_write(layout, ota, target)',
     'test_in_place_install_returns_to_the_other_firmware_when_its_first_boot_fails'),
    ('x3_slots.py', '    if layout.boot == layout.newt:', '    if False:',
     'test_in_place_install_returns_to_the_other_firmware_when_its_first_boot_fails'),
    ('x3_slots.py', "    return fallback, selection_record(ota, layout.newt)",
     "    return fallback, None if layout.boot == layout.newt else selection_record(ota, layout.newt)",
     'test_in_place_install_returns_to_the_other_firmware_when_its_first_boot_fails'),
    ('bench_flash.py', "        step('fallback-md5', lambda: verify_md5(device, OTADATA, x3_slots.applied(ota, fallback)))",
     '        pass', 'test_in_place_install_writes_no_app_byte_before_its_fallback_is_proven'),
    ('bench_flash.py', "    step('before-outside-md5', outside)\n\n    def write(", '    pass\n\n    def write(',
     'test_repair_refuses_when_the_survivor_differs_from_the_backup'),
    ('bench_flash.py', "        step('written-range-md5', lambda: verify_md5(device, start, payload))", '        pass',
     'test_repair_selects_only_a_proven_slot'),
    ('bench_flash.py', "        if layout.images[slot].kind == 'newt' and device.md5(SLOTS[slot], len(image)) == expected:",
     '        if slot == layout.newt:', 'test_boot_newt_selects_the_slot_holding_the_candidate'),
    ('bench_device.py', "            require(offset == layout.newt_offset, 'Writes go only to the writable app slot')",
     '            pass', 'test_transport_never_writes_the_newt_that_boots'),
    ('bench_flash.py', "    require(target is not None, 'No slot holds another firmware; boot-other has nothing to select')",
     '    pass', 'test_boot_other_needs_another_firmware'),
    ('x3_slots.py', '    if seq >= 0xffffffff:', '    if False:',
     'test_install_refuses_when_only_the_reserved_sequence_selects_the_spare'),
    ('bench_flash.py', 'x3_slots.image_intact(backup[kept:kept + SLOT_SIZE], SLOT_SIZE),\n        f',
     'True,\n        f', 'test_install_other_refuses_a_corrupt_surviving_newt'),
    ('bench_flash.py', "record.get('phase') == 'writing' or", 'False or',
     'test_restore_other_recovers_its_own_interrupted_write'),
    ('bench_flash.py', "exist_ok=args.action != 'install-other'", 'exist_ok=True',
     'test_install_other_refuses_reusing_a_restore_session'),
    ('bench_flash.py', "    step('after-outside-md5', outside)\n    unit.outside_verified()",
     '    unit.outside_verified()', 'test_install_other_detects_outside_corruption_at_both_span_boundaries'),
    # install-other and restore-other (tests/bench_install_other_cases.py).
    ('bench_candidate.py', "    require(sha(image) == image_sha256, 'Image SHA-256 differs from its pin')", '    pass',
     'test_install_other_refuses_unsafe_images_before_opening_the_unit'),
    ('bench_candidate.py', '    require(x3_slots.image_length(image) == len(image) and x3_slots.image_intact(image, CAPACITY),',
     '    require(True,', 'test_install_other_refuses_unsafe_images_before_opening_the_unit'),
    ('bench_candidate.py', "    write_span(SLOTS[0], len(image))\n    require(image[2:4]", '    require(image[2:4]',
     'test_install_other_refuses_unsafe_images_before_opening_the_unit'),
    ('bench_candidate.py', "    require(x3_slots.classify(image[:HEAD]).kind == 'other',", '    require(True,',
     'test_install_other_refuses_unsafe_images_before_opening_the_unit'),
    ('bench_flash.py', "        require(images[target].kind in ('newt', 'empty'),", '        require(True,',
     'test_install_other_refuses_unsafe_layouts'),
    ('bench_flash.py', "        step('installed-md5', lambda: require(record.get('phase') == 'writing' or\n                                              device.md5(start, record['length']) == record['md5'],",
     "        step('installed-md5', lambda: require(True,", 'test_restore_other_writes_back_only_the_image_it_installed'),
    ('bench_flash.py', "    step('before-outside-md5', outside)\n    unit.begin_write()", '    unit.begin_write()',
     'test_install_other_proves_the_unit_before_and_the_slot_after_writing'),
    ('bench_flash.py', "    unit.outside_verified()\n    step('written-range-md5', lambda: verify_md5(device, start, payload))",
     '    unit.outside_verified()', 'test_install_other_proves_the_unit_before_and_the_slot_after_writing'),
    ('bench_device.py', "        require(boot in (0, 1) and offset == SLOTS[1 - boot], 'Writes go only to the slot that does not boot')",
     '        pass', 'test_transport_writes_only_the_slot_that_does_not_boot'),
]


if __name__ == '__main__':
    bench.main(LayoutTests, CONTROLS, __file__)
