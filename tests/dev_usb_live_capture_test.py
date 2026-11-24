#!/usr/bin/env python3
"""DEV USB live frame capture over ESP32-C3 HWCDC's 256-byte ring buffer (#38).

rust/tests/dev_usb_live_capture/capture.rs compiles the real firmware bridge
(rust/firmware/dev_usb.rs) and real board channel (rust/salamander/board/dev_usb.rs)
against a cycle-accurate model of ESP32-C3 USB_SERIAL_JTAG, Arduino HWCDC's
256-byte wrapping TX ring buffer, 64-byte EP1 IN FIFO auto-flush, and Linux
cdc_acm's 512-byte Bulk IN URB. Negative controls break each part of the fix
and verify the exact Apps stall and Settings `ERROR capture-expired` regressions.
"""
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from gate_tier import negative_controls

BINARY = 'dev_usb_live_capture'
TESTS = 4
# (what breaks, path under rust/, good text, broken text, test that must fail)
CONTROLS = (
    ('Bridge skips flush_tx_tail on Apps live capture (reproduces Apps stall)',
     'firmware/dev_usb.rs',
     '        if channel.flush_tx_tail() {\n',
     '        let _ = channel; if true {\n',
     'apps_live_capture_completes_across_all_256_ring_offsets'),
    ('Bridge skips flush_tx_tail on Settings live capture (reproduces ERROR capture-expired)',
     'firmware/dev_usb.rs',
     '        if channel.flush_tx_tail() {\n',
     '        let _ = channel; if true {\n',
     'settings_live_capture_never_expires_under_idle_serial_log'),
    ('board flush_tx_tail ignores txfifo_writable and clears while 64-byte packet is still in HW FIFO',
     'salamander/board/dev_usb.rs',
     '    wake_transmitter()\n}',
     '    let _ = wake_transmitter();\n    true\n}',
     'flush_tx_tail_waits_for_ring_drain_and_hw_fifo_before_sending_zlp'),
    ('board flush_tx_tail does not wait for HWCDC TX ring buffer to empty first',
     'salamander/board/dev_usb.rs',
     '    if available_for_write() < HWCDC_TX_RING_BYTES {\n        let _ = wake_transmitter();\n        return false;\n    }\n',
     '',
     'flush_tx_tail_waits_for_ring_drain_and_hw_fifo_before_sending_zlp'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_binary(run, workspace, target):
    """Compiles the test; a compiler error is a failure, never a control's kill."""
    wire = run.cargo(workspace, target, 'test', '-p', 'newt-core', '--test', BINARY, '--no-run',
                     '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == BINARY]
    if len(outputs) != 1:
        raise RuntimeError(f'one compiled {BINARY} test executable required')
    return outputs[0]


def control(run, workspace, target, support, name, source, good, bad, test):
    path = workspace / source
    text = path.read_text()
    support.require(text.count(good) == 1, f'control "{name}": its anchor is not in rust/{source} exactly once')
    path.write_text(text.replace(good, bad))
    try:
        binary = test_binary(run, workspace, target)
        number = len(run.commands)
        try:
            run.run([binary, test, '--exact'], workspace)
        except RuntimeError:
            stdout = (run.output / f'{number:03d}.stdout').read_text()
            support.require('test result: FAILED. 0 passed; 1 failed' in stdout and 'panicked at' in stdout,
                            f'control "{name}": {test} must fail on an assertion')
        else:
            raise RuntimeError(f'control "{name}": {test} passed with the rule broken')
    finally:
        path.write_text(text)
    print(f'PASS control: {test} fails when {name}', flush=True)


def main():
    support = load('dev_usb_live_capture_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/dev-usb-live-capture'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 300)
        run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        target = output / 'target'
        try:
            result = run.run([test_binary(run, workspace, target)], workspace)
            support.require(f'test result: ok. {TESTS} passed; 0 failed; 0 ignored' in result,
                            f'all {TESTS} {BINARY} tests ran and passed')
            print('PASS DEV USB live capture flushes 64-byte HWCDC tail boundaries without stalls or lease expiry',
                  flush=True)
            if negative_controls(__file__):
                run.env['RUSTFLAGS'] = '-Ccodegen-units=1'
                for row in CONTROLS:
                    control(run, workspace, target, support, *row)
        except RuntimeError:
            for log in sorted((output / 'logs').glob('*.std*'))[-2:]:
                print(log.read_text(), file=sys.stderr, end='')
            raise


if __name__ == '__main__':
    main()
