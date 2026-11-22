#!/usr/bin/env python3
"""Host-only checks for the NEWT-33 Lead capture action plans."""
import argparse
from contextlib import contextmanager, redirect_stderr
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import Mock, patch
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
options = argparse.ArgumentParser()
options.add_argument('--script', type=Path, default=ROOT / 'artifacts/newt33-lead-device.py')
options.add_argument('--negative-controls', action='store_true')
args = options.parse_args()
SCRIPT = args.script
spec = importlib.util.spec_from_file_location('newt33_lead_device', SCRIPT)
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class FakeClock:
    def __init__(self):
        self.now = 100.0
        self.actions = []
        self.events = []
        self.idles = []

    def monotonic(self):
        return self.now

    def idle(self, seconds):
        self.idles.append(seconds)
        self.now += seconds

    def press(self, button, gap, label):
        self.actions.append((button, gap, label, self.now))
        # Model a bounded nonzero protocol/render round trip, then the exact
        # host-controlled quiet gap. No serial or hardware module is opened.
        self.now += 0.1
        self.idle(gap)

    def mark(self, label, **details):
        self.events.append((label, details, self.now))


continuous = FakeClock()
capture.run_continuous_input(continuous.press, continuous.idle, continuous.mark,
                             continuous.monotonic)
assert continuous.actions[-1][0:3] == ('back', 2, 'exit-after-continuous-input')
turns = continuous.actions[:-1]
assert turns and all(action[1] < 2 for action in turns)
assert continuous.actions[-1][3] - continuous.actions[0][3] > 30
complete = [event for event in continuous.events if event[0] == 'continuous-input-complete']
assert len(complete) == 1 and complete[0][1]['elapsed_seconds'] >= capture.CONTINUOUS_INPUT_SECONDS

producer = FakeClock()
capture.run_producer_capture(producer.press, producer.idle, producer.mark,
                             producer.monotonic)
assert [action[:3] for action in producer.actions] == [
    ('next', 0, 'producer-window-setup-turn'),
    ('next', 0.25, 'producer-window-input'),
    ('back', 2, 'exit-after-producer-window'),
]
assert abs(producer.actions[1][3] - 102.2) < 1e-9
assert abs(producer.actions[2][3] - 110.55) < 1e-9
assert capture.PRODUCER_START_DELAY_SECONDS > 2
assert any('raw PC target/admission/attempt' in event[1].get('note', '')
           for event in producer.events)

periodic = FakeClock()
capture.run_periodic(periodic.press, periodic.idle, periodic.mark, periodic.monotonic)
assert len(periodic.actions) == 44
assert periodic.actions[-1][0:3] == ('back', 2, 'exit')
assert periodic.idles.count(34) == 3

parser = capture.build_parser()
assert parser.parse_args(['--port', 'chosen', '--expect-build', 'abc', '--phase', 'settled',
                          '--scenario', 'continuous-input', '--power-reset', 'observed',
                          '--output', '/outside-project/not-written']).scenario == 'continuous-input'
position = parser.parse_args(['--port', 'chosen', '--expect-build', 'abc', '--phase', 'settled',
                              '--scenario', 'continuous-input', '--power-reset', 'observed',
                              '--output', '/outside-project/not-written', '--expect-spine', '2', '--expect-character', '180'])
assert (position.expect_spine, position.expect_character) == (2, 180)
try:
    with redirect_stderr(io.StringIO()):
        capture.main(['--port', 'chosen', '--expect-build', 'abc', '--phase', 'settled',
                      '--scenario', 'continuous-input', '--power-reset', 'observed',
                      '--output', '/outside-project/not-written', '--expect-spine', '2'])
except SystemExit as error:
    assert error.code == 2
else:
    raise AssertionError('matched position needs both coordinates')
try:
    with redirect_stderr(io.StringIO()):
        parser.parse_args(['--port', 'chosen', '--expect-build', 'abc', '--phase', 'settled',
                           '--power-reset', 'observed', '--output', '/outside-project/not-written'])
except SystemExit as error:
    assert error.code == 2
else:
    raise AssertionError('--scenario must be explicit')

try:
    with redirect_stderr(io.StringIO()):
        capture.main(['--port', 'chosen', '--expect-build', 'abc', '--phase', 'pending',
                      '--scenario', 'periodic', '--power-reset', 'observed',
                      '--output', '/outside-project/not-written'])
except SystemExit as error:
    assert error.code == 2
else:
    raise AssertionError('pending capture must retain its explicit --wait-reader gate')

source = SCRIPT.read_text()
assert "client.hello(args.expect_build)" in source
assert "args.output.with_suffix('.serial').open('wb')" in source
assert "report['protocol'] = client.receipt()" in source
print('NEWT-33 Lead capture script plans: PASS')


# Exercise the live entry boundary with a wholly fake driver, no serial import.
with tempfile.TemporaryDirectory(prefix='newt33-lease-') as scratch:
    root = Path(scratch).resolve()
    held = False
    events = []

    @contextmanager
    def lease(port):
        global held
        assert port == '/dev/fake-only'
        held = True
        events.append('lease')
        try:
            yield
        finally:
            held = False
            events.append('release')

    class FakeClient:
        def __init__(self, serial, port, timeout):
            assert held, 'capture opened outside lease'
            events.append('open')
            self.closed = False

        def hello(self, expected):
            assert held
            return 'HELLO build=' + expected

        def diagnostics(self):
            assert held
            return {'reader_open': True}

        def request(self, command):
            assert held
            return ['STATUS fake=1']

        def close(self):
            assert held, 'capture closed outside lease'
            events.append('close')
            self.closed = True

        def receipt(self):
            assert held and self.closed, 'receipt published outside lease'
            events.append('receipt')
            return {'connection': {'closed': True}}

    driver = SimpleNamespace(SerialClient=FakeClient, approved_serial=lambda: object(), lease_port=lease)
    spec = SimpleNamespace(loader=SimpleNamespace(exec_module=Mock()))
    with patch.object(capture.importlib.util, 'spec_from_file_location', return_value=spec), \
            patch.object(capture.importlib.util, 'module_from_spec', return_value=driver), \
            patch.dict(capture.SCENARIOS, {'periodic': lambda *a: None}):
        capture.main(['--port', '/dev/fake-only', '--expect-build', 'host-test', '--phase', 'settled',
                      '--scenario', 'periodic', '--power-reset', 'synthetic only', '--output', str(root / 'capture')])
    assert events == ['lease', 'open', 'close', 'receipt', 'release'], events
    assert json.loads((root / 'capture.json').read_text())['protocol']['connection']['closed']
    assert not held
print('NEWT-33 shared lease through opening, closure and receipt: PASS; no device access')

if args.negative_controls:
    with tempfile.TemporaryDirectory(prefix='newt33-lease-mutant-') as scratch:
        old = "with driver.lease_port(args.port), args.output.with_suffix('.serial').open('wb') as raw:"
        new = "with args.output.with_suffix('.serial').open('wb') as raw:"
        source = SCRIPT.read_text()
        assert source.count(old) == 1
        mutant = Path(scratch) / 'capture.py'
        mutant.write_text(source.replace(old, new))
        result = subprocess.run([sys.executable, '-B', __file__, '--script', str(mutant)],
                                capture_output=True, text=True, timeout=15)
        assert result.returncode != 0 and 'capture opened outside lease' in result.stderr, result.stderr
        print('PASS guard-removal control: Lead capture lease')
