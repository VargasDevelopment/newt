"""Session-selected identity and held-port confirmation cases for BenchTests.

Uses the same scratch-injected CLI/transport harness and negative controls.
"""
import json
import sys
from types import SimpleNamespace
from unittest.mock import patch

import bench_device as transport
import bench_flash as tool
import bench_lease as safety
from bench_flash_fixtures import Fake, IDENTITY, MAC, RULE
import x3_slots


class SessionCases:
    def test_explicit_unit_rule_keeps_the_identity_checks(self):
        main_mac = '02:00:00:00:4d:01'  # synthetic second unit
        self.rule.chmod(0o644)
        self.rule.write_text(RULE.replace(MAC.upper(), main_mac.upper()))
        # A missing default proves this is the explicit CLI path, not fallback.
        with patch.object(safety, 'UDEV_RULE', self.root / 'missing-default'):
            self.invoke(rule=self.rule)
        self.assertEqual(self.seen_identities[0]['mac'], main_mac)
        self.assertIn(main_mac.upper(), self.seen_identities[0]['port'])
        self.assertTrue((self.shared / main_mac.replace(':', '') / 'write.json').exists())
        receipt = json.loads(next(self.session.glob('run-*/receipt.json')).read_text())
        self.assertEqual(receipt['identity'], self.seen_identities[0])
        self.assertEqual(receipt['identity_rule'], str(self.rule))

    def test_explicit_invalid_rule_never_falls_back(self):
        for contents, mode in ((RULE, 0o666), (RULE + RULE, 0o444)):
            self.rule.chmod(0o644)
            self.rule.write_text(contents)
            self.rule.chmod(mode)
            with self.assertRaises(ValueError):
                self.invoke(rule=self.rule)
        with self.assertRaises(FileNotFoundError):
            self.invoke(rule=self.root / 'missing')
        alias = self.root / 'alias.rules'
        alias.symlink_to(self.rule)
        with self.assertRaises(OSError):
            self.invoke(rule=alias)
        other = self.root / 'unowned.rules'
        other.write_text(RULE)
        with self.assertRaisesRegex(ValueError, 'root-owned'):
            self.invoke(rule=other)
        self.assertEqual(self.device.connects, 0)
        self.assertEqual(self.seen_identities, [])

    def test_other_selection_confirms_before_a_resetting_close(self):
        # Another firmware answers no HELLO; the held window must precede a resetting close.
        self.device = Fake(self.image.table, layout='main')
        self.invoke()
        self.device.writes.clear()
        boots, resets = len(self.device.boots), self.device.resets
        events = []
        original_wait = self.device.confirm_window
        with patch.object(self.device, 'confirm_window') as hold:
            def confirm():
                self.assertTrue(self.device.port_open)
                self.assertEqual(self.device.resets, resets + 1)
                events.append('wait-30')
                original_wait()
            hold.side_effect = confirm
            original_close = self.device.close
            def close():
                if self.device.port_open:
                    events.append('close-reset')
                    self.device.reset()
                original_close()
            with patch.object(self.device, 'close', side_effect=close):
                self.invoke(action='boot-other')
        self.assertEqual(events, ['wait-30', 'close-reset'])
        self.assertEqual(self.device.boots[boots:], [1, 1])
        records = x3_slots.selections(bytes(self.device.memory[0xe000:0x10000]))
        self.assertEqual(records[1].state, x3_slots.STATE_VALID)
        self.assertEqual(len(self.device.hellos), 1)  # only the Newt install
        self.assertEqual([offset for offset, _ in self.device.writes], [0xf000])

    def test_transport_close_holds_after_success_and_reset_error(self):
        for fails in (False, True):
            with self.subTest(fails=fails):
                now, events = [0.0], []
                def reset():
                    events.append('reset')
                    now[0] += 2
                    if fails:
                        raise OSError('reset acknowledgement lost')
                def sleep(seconds):
                    events.append(('wait', seconds))
                    now[0] += seconds
                def close():
                    self.assertGreaterEqual(now[0], 30)
                    events.append('close')
                device = transport.Device(IDENTITY)
                device.esp = SimpleNamespace(hard_reset=reset, _port=SimpleNamespace(close=close))
                strategy = SimpleNamespace(HardReset=lambda port: SimpleNamespace(reset=reset))
                with patch.dict(sys.modules, {'esptool.reset': strategy}), \
                        patch.object(transport.time, 'monotonic', side_effect=lambda: now[0]), \
                        patch.object(transport.time, 'sleep', side_effect=sleep):
                    if fails:
                        with self.assertRaises(OSError):
                            device.reset()
                    else:
                        device.reset()
                    device.close()
                    device.close()  # Idempotent; never another reset/close.
                self.assertEqual(events, ['reset', ('wait', 30), 'close'])

    def test_reset_error_never_retries_or_reopens(self):
        now, events = [0.0], []
        def close():
            self.assertGreaterEqual(now[0], 30, 'a reset error must not close/reopen before confirmation')
            events.append('close')
        port = SimpleNamespace(close=close, open=lambda: self.fail('reset must never reopen'))
        class RetryingReset:
            def __init__(self, held_port):
                self.port = held_port
            def reset(self):
                events.append('pulse')
                now[0] += 2
                raise OSError('reset reached unit but acknowledgement failed')
            def __call__(self):
                # Pinned ResetStrategy.__call__ closes and reconnects on error.
                try:
                    self.reset()
                except OSError:
                    self.port.close()
                    self.port.open()
                    self.reset()
        device = transport.Device(IDENTITY)
        device.esp = SimpleNamespace(_port=port, hard_reset=RetryingReset(port))
        strategy = SimpleNamespace(HardReset=RetryingReset)
        def sleep(seconds):
            events.append(('wait', seconds))
            now[0] += seconds
        with patch.dict(sys.modules, {'esptool.reset': strategy}), \
                patch.object(transport.time, 'monotonic', side_effect=lambda: now[0]), \
                patch.object(transport.time, 'sleep', side_effect=sleep):
            with self.assertRaises(OSError):
                device.reset()
            device.close()
        self.assertEqual(events, ['pulse', ('wait', 30), 'close'])
