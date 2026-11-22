"""USB recovery selects the proven other firmware's slot without writing either app."""
import json
from unittest.mock import patch

import bench_flash as tool
from bench_flash_fixtures import APP0, APP1, Fake, select
import x3_slots


class OtherFirmwareCases:
    def test_boot_other_preserves_both_apps_in_both_layouts(self):
        for layout in ('bench', 'main'):
            with self.subTest(layout=layout):
                self.device = Fake(self.image.table, layout=layout)
                self.shared = self.root / ('units-' + layout)
                session = self.root / ('session-' + layout)
                self.invoke(session=session)
                before = bytes(self.device.memory)
                self.device.writes.clear()
                # The other firmware confirms itself after the host's held window; no DEV HELLO.
                self.invoke(session=session, action='boot-other')
                at = 0xf000 if layout == 'main' else 0xe000
                self.assertEqual(self.device.writes, [(at, 4096)])
                self.assertEqual(bytes(self.device.memory[APP0:]), before[APP0:])
                self.assertEqual(len(self.device.hellos), 1)  # only the first Newt install
                ota = bytes(self.device.memory[0xe000:0x10000])
                self.assertEqual(x3_slots.boot_slot(ota), 1 if layout == 'main' else 0)
                self.assertEqual(x3_slots.selections(ota)[1 if layout == 'main' else 0].state, x3_slots.STATE_VALID)
                self.assertEqual(x3_slots.selections(ota)[0 if layout == 'main' else 1].state, x3_slots.STATE_VALID)
                receipt = json.loads(sorted(session.glob('run-*/receipt.json'))[-1].read_text())
                self.assertIn('other-slot-md5', [step['step'] for step in receipt['steps']])
                self.assertEqual(receipt['result']['written'], [hex(at)])
                self.invoke(session=session, action='boot-other')  # Proven no-op still holds/reset once.
                self.assertEqual(self.device.writes, [(at, 4096)])

    def test_boot_other_refuses_changed_backup_identity_or_bytes(self):
        self.device = Fake(self.image.table, layout='main')
        self.invoke()
        self.device.writes.clear()
        other = self.bench_layout().other_offset
        # Altering a byte beyond the excluded app-install span must be refused.
        self.device.memory[other + x3_slots.HEAD + 50] ^= 1
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke(action='boot-other')
        self.assertEqual(self.device.writes, [])
        self.device.memory[other + x3_slots.HEAD + 50] ^= 1
        original = self.device.md5
        def wrong_md5(offset, length):
            if offset == other and length == x3_slots.SLOT_SIZE:
                return '0' * 32
            return original(offset, length)
        with patch.object(self.device, 'md5', side_effect=wrong_md5):
            with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
                self.invoke(action='boot-other')
        self.assertEqual(self.device.writes, [])
        # A namespace/session receipt alone cannot assert the app descriptor.
        self.device.memory[other] = 0
        with self.assertRaisesRegex(ValueError, 'Neither slot|unreadable'):
            self.invoke(action='boot-other')
        self.assertEqual(self.device.writes, [])

    def test_boot_other_keeps_confirmed_fallback_when_target_pending(self):
        self.invoke()
        # app1/Newt confirmed, app0/CrossInk pending: bootloader follows Newt.
        select(self.device.memory, 0xf000, 8)
        select(self.device.memory, 0xe000, 9, x3_slots.STATE_PENDING)
        fallback = bytes(self.device.memory[0xf000:0x10000])
        self.device.writes.clear()
        self.invoke(action='boot-other')
        self.assertEqual(self.device.writes, [(0xe000, 4096)])
        self.assertEqual(bytes(self.device.memory[0xf000:0x10000]), fallback)
        self.assertEqual(self.device.boots[-1], 0)
