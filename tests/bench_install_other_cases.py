"""install-other and restore-other: another firmware's pinned image into the
slot that does not boot, beside the running Newt, and back."""
import contextlib
import hashlib
import json
import struct
import sys
from types import SimpleNamespace
from unittest.mock import patch

import bench_candidate as candidate
import bench_device as transport
import bench_flash as tool
from bench_flash_fixtures import CROSSINK, IDENTITY, app_bytes
import x3_slots
from x3_slots import OTADATA, SLOTS, STATE_NEW, STATE_PENDING, STATE_VALID

STOCK = app_bytes('V6.5.5', newt=False, elf='44' * 32, extra=6000)  # two sectors, named by no table entry
OLDER = app_bytes('older-newt')


def rechipped(image, chip):
    """The same app for another chip, its digest recomputed (the checksum covers only segment data)."""
    data = bytearray(image[:-32])
    struct.pack_into('<H', data, 12, chip)
    return bytes(data) + hashlib.sha256(data).digest()


def records(running):
    """Two valid records; the highest boots `running`."""
    return ((5, STATE_VALID), (6, STATE_VALID)) if running else ((7, STATE_VALID), (6, STATE_VALID))


class InstallOtherCases:
    def image_file(self, name, data):
        path = self.root / f'{name}.bin'
        path.write_bytes(data)
        return path

    def other(self, action, *argv, session):
        argv = ['bench_flash.py', action, *map(str, argv), '--session', str(session)]
        with patch.object(tool, 'ROOT', self.root, create=True), \
                patch.object(tool, 'Device', side_effect=lambda identity: self.device), \
                patch.object(sys, 'argv', argv), \
                contextlib.redirect_stdout(self.output), contextlib.redirect_stderr(self.output):
            return tool.main()

    def install_stock(self, session, data=STOCK):
        return self.other('install-other', self.image_file('stock', data), '--image-sha256',
                          candidate.sha(data), session=session)

    def refused(self, message, run):
        before = bytes(self.device.memory)
        with self.assertRaisesRegex(ValueError, message):
            run()
        self.assertEqual(self.device.writes, [])
        self.assertTrue(self.device.memory == before, 'Device memory changed')
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_install_other_writes_only_the_slot_that_does_not_boot_and_restore_puts_it_back(self):
        for running, name, spare in ((0, 'older-newt', OLDER), (1, 'empty', None)):
            with self.subTest(running=running, spare=name):
                slots = [self.image.image, self.image.image]
                slots[1 - running] = spare
                self.unit(slots, records(running), f'other-{name}-{running}')
                start = SLOTS[1 - running]
                before = bytes(self.device.memory)
                first = self.session.with_name(self.session.name + '-install')
                self.install_stock(first)
                span = 2 * 4096
                self.assertEqual(self.device.writes, [(start, span)])
                self.assertTrue(self.device.memory[:start] == before[:start] and
                                self.device.memory[start + span:] == before[start + span:],
                                'A byte outside the written span changed')
                self.assertTrue(self.device.memory[start:start + span] ==
                                STOCK + b'\xff' * (span - len(STOCK)))
                # No selection: the running Newt still boots.
                self.assertEqual(self.device.boots, [running])
                # boot-other starts it; boot-newt returns. Each needs a new session.
                self.invoke(action='boot-other', session=self.session.with_name(self.session.name + '-other'))
                self.assertEqual(self.device.boots[-1], 1 - running)
                self.invoke(action='boot-newt', session=self.session.with_name(self.session.name + '-newt'))
                self.assertEqual(self.device.boots[-1], running)
                self.device.writes.clear()
                self.other('restore-other', '--from', first,
                           session=self.session.with_name(self.session.name + '-restore'))
                self.assertEqual(self.device.writes, [(start, span)])
                # Only the selection records differ from before the install (boot-other, boot-newt).
                self.assertTrue(self.device.memory[SLOTS[0]:] == before[SLOTS[0]:],
                                'The slots differ from before the install')
                self.assertEqual(self.device.boots[-1], running)
                self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_install_other_refuses_unsafe_images_before_opening_the_unit(self):
        self.unit((self.image.image, OLDER), records(0), 'images')
        cases = {
            'Image SHA-256 differs from its pin': (STOCK, candidate.sha(STOCK[:-1] + b'\0')),
            'Not one whole ESP32-C3 app image': (rechipped(STOCK, 9), None),  # ESP32-S3
            'App exceeds usable slot': (app_bytes('big', newt=False, extra=candidate.USABLE), None),
            'installs with install and its reviewed manifest': (self.image.image, None),
            'Not one whole': (STOCK[:-16], None),
        }
        for message, (data, pin) in cases.items():
            with self.subTest(message):
                path = self.image_file('unsafe', data)
                self.refused(message, lambda: self.other('install-other', path, '--image-sha256',
                                                         pin or candidate.sha(data), session=self.session))
                self.assertEqual(self.device.connects, 0)

    def test_install_other_refuses_unsafe_layouts(self):
        cases = {
            'replaces only an older Newt or nothing': ((self.image.image, CROSSINK), records(0)),
            'holds invalid; install-other replaces only': ((self.image.image, b'\0' * 0x200), records(0)),
            'A newer selection is still pending': ((self.image.image, OLDER), ((7, STATE_PENDING), (6, STATE_VALID))),
            r'selection of app1 is not confirmed \(new\)': ((OLDER, self.image.image), (None, (6, STATE_NEW))),
            'the slot that boots, does not hold Newt': ((STOCK, self.image.image), records(0)),
        }
        for index, (message, (slots, selection)) in enumerate(cases.items()):
            with self.subTest(message):
                self.unit(slots, selection, f'layout-{index}')
                self.refused(message, lambda: self.install_stock(self.session))

    def test_restore_other_writes_back_only_the_image_it_installed(self):
        self.unit((self.image.image, OLDER), records(0), 'restore')
        first = self.session.with_name('install')
        self.install_stock(first)
        self.device.writes.clear()
        record_path = first / tool.OTHER_RECORD
        record = json.loads(record_path.read_text())
        self.assertEqual(record.pop('phase'), 'verified')
        record_path.write_text(json.dumps(record))
        # Something else replaced it since: not restore-other's to overwrite.
        self.device.memory[SLOTS[1]:SLOTS[1] + len(CROSSINK)] = CROSSINK
        self.refused('no longer holds the image install-other wrote',
                     lambda: self.other('restore-other', '--from', first, session=self.session.with_name('r1')))
        self.device.memory[SLOTS[1]:SLOTS[1] + len(STOCK)] = STOCK
        # The installed slot boots now: never written.
        self.invoke(action='boot-other', session=self.session.with_name('r2'))
        self.device.writes.clear()
        self.refused('the slot that boots, does not hold Newt',
                     lambda: self.other('restore-other', '--from', first, session=self.session.with_name('r3')))
        # A changed install backup is no payload.
        self.invoke(action='boot-newt', session=self.session.with_name('r4'))
        self.device.writes.clear()
        backup = first / 'backup.bin'
        data = bytearray(backup.read_bytes())
        data[SLOTS[1] + 0x300] ^= 1
        backup.write_bytes(data)
        self.refused('Install session backup corrupt',
                     lambda: self.other('restore-other', '--from', first, session=self.session.with_name('r5')))

    def test_install_other_proves_the_unit_before_and_the_slot_after_writing(self):
        self.unit((self.image.image, OLDER), records(0), 'drift')
        self.device.flip_before = 0x9100  # NVS changes after the backup
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.install_stock(self.session)
        self.assertEqual(self.device.writes, [])
        self.assertFalse((self.unit_dir / 'STOP').exists())
        self.unit((self.image.image, OLDER), records(0), 'corrupt')
        self.device.corrupt_payload = True
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.install_stock(self.session)
        self.assertEqual((self.device.writes, self.device.resets), ([(SLOTS[1], 2 * 4096)], 0))
        self.assertTrue((self.unit_dir / 'STOP').exists())

    def test_install_other_refuses_a_corrupt_surviving_newt(self):
        damaged = bytearray(self.image.image)
        damaged[0x210] ^= 1  # past the descriptor, still classifies as Newt
        self.unit((bytes(damaged), OLDER), records(0), 'survivor')
        self.assertFalse(x3_slots.image_intact(self.slot(0), x3_slots.SLOT_SIZE))
        self.refused('surviving Newt, fails the image check', lambda: self.install_stock(self.session))

    def test_restore_other_recovers_its_own_interrupted_write(self):
        self.unit((self.image.image, OLDER), records(0), 'interrupted')
        first = self.session
        before = bytes(self.device.memory)

        def interrupted(boot, start, payload):
            self.device.memory[start:start + 4096] = payload[:4096]
            raise OSError('Injected interruption after first sector')

        with patch.object(self.device, 'write_spare', side_effect=interrupted), \
                self.assertRaisesRegex(OSError, 'after first sector'):
            self.install_stock(first)
        self.assertEqual(x3_slots.classify(self.slot(1)[:x3_slots.HEAD]).kind, 'other')
        self.assertFalse(x3_slots.image_intact(self.slot(1), x3_slots.SLOT_SIZE))
        self.assertTrue((self.unit_dir / 'STOP').exists())
        # Simulate the Lead/owner recovery review in this synthetic unit only.
        # The production command must still stop until that review clears the journal.
        with self.assertRaisesRegex(ValueError, 'Bench stopped'):
            self.other('restore-other', '--from', first, session=self.session.with_name('blocked'))
        (self.unit_dir / 'STOP').unlink()
        (self.unit_dir / 'write.json').unlink()
        self.other('restore-other', '--from', first, session=self.session.with_name('recovery'))
        self.assertEqual(self.device.writes, [(SLOTS[1], 8192)])
        self.assertTrue(self.device.memory == before)

    def test_install_other_refuses_reusing_a_restore_session(self):
        self.unit((self.image.image, OLDER), records(0), 'reuse')
        first, second = self.session, self.session.with_name('restore')
        self.install_stock(first)
        self.other('restore-other', '--from', first, session=second)
        self.device.writes.clear()
        before = bytes(self.device.memory)
        with self.assertRaises(FileExistsError):
            self.install_stock(second)
        self.assertEqual(self.device.writes, [])
        self.assertTrue(self.device.memory == before)

    def test_install_other_detects_outside_corruption_at_both_span_boundaries(self):
        for boot in (0, 1):
            start = SLOTS[1 - boot]
            for boundary in (start - 1, start + 8192):
                with self.subTest(boot=boot, boundary=hex(boundary)):
                    self.unit((self.image.image, self.image.image), records(boot), f'boundary-{boundary}')
                    self.device.flip_during = boundary
                    with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
                        self.install_stock(self.session)
                    self.assertEqual(self.device.writes, [(start, 8192)])
                    self.assertEqual(self.device.resets, 0)
                    self.assertTrue((self.unit_dir / 'STOP').exists())
                    self.assertEqual(json.loads((self.unit_dir / 'STOP').read_text())['outside_md5'], 'unknown')

    def test_transport_writes_only_the_slot_that_does_not_boot(self):
        written = []
        esptool = SimpleNamespace(loader=SimpleNamespace(),
                                  cmds=SimpleNamespace(write_flash=lambda esp, regions, **kw: written.extend(regions)))
        device = transport.Device(IDENTITY)
        device.esp = SimpleNamespace()
        with patch.dict(sys.modules, esptool=esptool):
            for boot in (0, 1):
                for offset in (SLOTS[boot], OTADATA, 0):
                    with self.subTest(boot=boot, offset=offset), \
                            self.assertRaisesRegex(ValueError, 'only to the slot that does not boot'):
                        device.write_spare(boot, offset, b'\xff' * 4096)
                device.write_spare(boot, SLOTS[1 - boot], b'\xff' * 4096)
        self.assertEqual([offset for offset, _ in written], [SLOTS[1], SLOTS[0]])
