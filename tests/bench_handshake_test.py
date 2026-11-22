#!/usr/bin/env python3
"""The bench's post-reset handshake (tools/bench_device.py): bounded readiness
on the port held open through the reset. In-memory peers only; never opens a
port or touches a device."""
import contextlib
import io
from pathlib import Path
import sys
from types import SimpleNamespace
import time
import unittest
from unittest.mock import patch
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import bench_device

BRIDGE = bench_device.load_bridge()
BUILD = 'a1a1a1a1a1a1'
HELLO = f'HELLO build={BUILD} caps=press,status,frame wake=physical-only'
STATUS = 'STATUS screen=0 gen=1 ready=1 refresh_ms=0 heap=0 dropped=0 capture=0'


class BootingPort:
    """A pyserial port to a device still booting: it discards the first
    `dropped` HELLOs, or holds `queued` HELLOs and answers them all at once
    when its bridge starts, or never answers."""
    port, timeout, write_timeout, in_waiting = '/dev/held-by-owner', 0, 0, 0

    def __init__(self, dropped=0, queued=0, answers=True):
        self.dropped, self.queued, self.answers = dropped, queued, answers
        self.output, self.session, self.held, self.writes = b'', 0, 0, []
        self.opened = self.closed = False

    def reset_input_buffer(self):
        self.output = b''

    def reply(self, identifier, body):
        payload = f'!N1 7 {self.session} {identifier} {body}'.encode()
        self.output += payload + f' *{zlib.crc32(payload):08x}\n'.encode()

    def write(self, data):
        self.writes.append(data)
        tokens = data.decode().split()
        if tokens != ['N1', 'HELLO']:
            self.reply(int(tokens[3]), STATUS)
        elif not self.answers:
            pass
        elif self.dropped:
            self.dropped -= 1
        elif self.held + 1 < self.queued:
            self.held += 1
        else:
            for _ in range(self.held + 1):
                self.session += 1
                self.reply(0, HELLO)
            self.held = self.queued = 0
        return len(data)

    def read(self, size):
        result, self.output = self.output[:size], self.output[size:]
        return result

    def open(self):
        self.opened = True

    def close(self):
        self.closed = True


def handshake(port, build=BUILD, budget=2):
    client = bench_device.held_client(BRIDGE, port, timeout=0.2)
    return client, bench_device.ready(BRIDGE, client, build, budget, attempt=0.05, quiet=0.05)


class Handshake(unittest.TestCase):
    def test_one_early_hello_is_lost_while_booting(self):
        # The former single request: discarded during boot, never answered.
        client = bench_device.held_client(BRIDGE, BootingPort(dropped=3), timeout=0.2)
        with self.assertRaisesRegex(BRIDGE.ProtocolTimeout, 'timed out'):
            client.hello(BUILD)

    def test_hello_is_resent_until_the_bridge_answers(self):
        port = BootingPort(dropped=3)
        client, body = handshake(port)
        self.assertEqual(body, HELLO)
        # Three lost, one readiness reply, then the strict pair and STATUS.
        self.assertEqual([data.split()[-1] for data in port.writes], [b'HELLO'] * 6 + [b'STATUS'])
        self.assertEqual(client.session, 3)

    def test_late_replies_are_drained_before_the_strict_pair(self):
        port = BootingPort(queued=3)
        client, _ = handshake(port)
        self.assertEqual(client.session, 5)
        self.assertTrue(client.request('STATUS'))

    def test_budget_is_bounded(self):
        started = time.monotonic()
        with self.assertRaisesRegex(ValueError, r'readiness budget \(Newt answers HELLO only once its image is confirmed'):
            handshake(BootingPort(answers=False), budget=0.3)
        self.assertLess(time.monotonic() - started, 1)

    def test_wrong_build_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'explicitly selected image'):
            handshake(BootingPort(dropped=1), build='other-build')

    def test_device_hello_never_reopens_or_closes_the_port(self):
        port = BootingPort(dropped=2)
        device = bench_device.Device({'port': port.port})
        device.esp = SimpleNamespace(_port=port)
        opened = SimpleNamespace(Serial=lambda **kw: self.fail('A second port open resets the X3'))
        with patch.dict(sys.modules, serial=opened), patch.object(bench_device, 'HELLO_BUDGET', 5), \
                contextlib.redirect_stdout(io.StringIO()):
            receipt = device.hello(BUILD)
        self.assertTrue(receipt['connection']['held'] and receipt['connection']['closed'])
        self.assertEqual(receipt['protocol']['session'], 3)  # readiness, then the strict pair
        self.assertFalse(port.opened or port.closed)  # Device.close releases esptool's port later.


if __name__ == '__main__':
    unittest.main()
