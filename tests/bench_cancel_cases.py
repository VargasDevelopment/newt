"""Cancellation cannot release a reset unit's lease before safe port closure."""
import signal
import sys
from types import SimpleNamespace
from unittest.mock import patch

import bench_device as transport
import bench_lease as safety
from bench_flash_fixtures import APP1, Fake, IDENTITY, MAC


class CancelCases:
    def cancelled_hold(self, failure):
        for cancellation in ('keyboard', 'close-keyboard', signal.SIGINT, signal.SIGTERM):
            with self.subTest(failure=failure, cancellation=cancellation):
                self.shared = self.root / ('units-' + str(cancellation))
                session = self.root / ('session-' + str(cancellation))
                fake = Fake(self.image.table)
                if failure == 'boot-newt':
                    fake.memory[APP1:APP1 + len(self.image.image)] = self.image.image
                device = transport.Device(IDENTITY)
                self.device = device
                now, events, sleeps, closes = [0.0], [], [0], [0]
                port = SimpleNamespace(close=None)
                original_handlers = {n: signal.getsignal(n) for n in (signal.SIGINT, signal.SIGTERM)}
                # A base lacking the SIGTERM guard must fail an assertion, not kill the test runner.
                def interrupt(number, frame):
                    raise KeyboardInterrupt()
                signal.signal(signal.SIGTERM, interrupt)
                def close():
                    closes[0] += 1
                    if cancellation == 'close-keyboard' and closes[0] == 1:
                        raise KeyboardInterrupt()
                    self.assertGreaterEqual(now[0], 30, 'cancel cannot close early')
                    with self.assertRaisesRegex(ValueError, 'bench is leased'):
                        with safety.lease_unit(MAC):
                            pass
                    fake.close()
                    events.append('closed-under-lease')
                port.close = close
                def connect():
                    fake.connect()
                    device.esp = SimpleNamespace(_port=port)
                def reset():
                    fake.reset()
                    if failure == 'reset':
                        raise OSError('reset acknowledgement lost')
                def hello(build):
                    if failure == 'success':
                        return {'hello': build}
                    raise ValueError('HELLO failed')
                def sleep(seconds):
                    sleeps[0] += 1
                    if sleeps[0] == 1 and cancellation != 'close-keyboard':
                        now[0] += 2
                        if cancellation == 'keyboard':
                            raise KeyboardInterrupt()
                        signal.raise_signal(cancellation)
                    else:
                        now[0] += seconds
                for name in ('read', 'md5', 'write'):
                    setattr(device, name, getattr(fake, name))
                device.connect, device.hello = connect, hello
                strategy = SimpleNamespace(HardReset=lambda held: SimpleNamespace(reset=reset))
                try:
                    with patch.dict(sys.modules, {'esptool.reset': strategy}), \
                            patch.object(transport.time, 'monotonic', side_effect=lambda: now[0]), \
                            patch.object(transport.time, 'sleep', side_effect=sleep):
                        with self.assertRaises((KeyboardInterrupt, SystemExit)) as caught:
                            action = failure if failure in ('slots', 'boot-newt', 'boot-other', 'restore') else 'install'
                            self.invoke(session=session, action=action)
                        if cancellation == signal.SIGTERM:
                            self.assertIsInstance(caught.exception, SystemExit)
                            self.assertEqual(caught.exception.code, 143)
                        self.assertEqual(events, ['closed-under-lease'])
                        self.assertIsNone(device.esp)
                        self.assertGreaterEqual(now[0], 30)
                        self.assertEqual(fake.resets, 1)
                        writes = {'slots': 0, 'boot-newt': 0, 'boot-other': 1}.get(failure, 3)
                        self.assertEqual(len(fake.writes), writes)
                        self.assertEqual((self.unit_dir / 'STOP').exists(), bool(writes))
                        self.assertTrue((self.unit_dir / 'lock').is_file())
                finally:
                    for number, handler in original_handlers.items():
                        signal.signal(number, handler)

    def test_reset_error_cancel_keeps_port_and_lease(self):
        self.cancelled_hold('reset')

    def test_hello_failure_cancel_keeps_port_and_lease(self):
        self.cancelled_hold('hello')

    def test_successful_hold_cancel_keeps_port_and_lease(self):
        self.cancelled_hold('success')

    def test_slots_cancel_keeps_readonly_port_and_lease(self):
        self.cancelled_hold('slots')

    def test_boot_newt_cancel_keeps_readonly_port_and_lease(self):
        self.cancelled_hold('boot-newt')

    def test_boot_other_cancel_keeps_port_and_lease(self):
        self.cancelled_hold('boot-other')

    def test_restore_cancel_keeps_port_and_lease(self):
        self.cancelled_hold('restore')
