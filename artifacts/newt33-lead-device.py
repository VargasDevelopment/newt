#!/usr/bin/env python3
"""Lead-only DEV capture. Never flashes; requires an explicitly chosen port/build.

Run only after the Lead has separately authorized hardware work, installed and
read back the exact image, and prepared the authored book/cache. Pending runs
start on Home, then wait for the operator to open it after capture starts. This
worker did not execute this script. Use the existing approved platformio
Python/pyserial.

Scenario labels record what the host requested. They do not prove device state.
In particular, only the raw serial PC target/admission/attempt rows establish
that the page-cache producer ran during a producer-running capture.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time


ROOT = Path(__file__).resolve().parents[1]
CONTINUOUS_INPUT_SECONDS = 32.0
CONTINUOUS_INPUT_GAP_SECONDS = 0.75
PRODUCER_START_DELAY_SECONDS = 2.1
PRODUCER_FOLLOWUP_SECONDS = 8.0


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True)
    parser.add_argument('--expect-build', required=True)
    parser.add_argument('--phase', choices=('settled', 'pending'), required=True,
                        help='source-verification phase to capture; raw rows establish the observed phase')
    parser.add_argument('--scenario', choices=('periodic', 'continuous-input', 'producer-running'),
                        required=True, help='bounded host action sequence to request')
    parser.add_argument('--wait-reader', action='store_true',
                        help='capture first, then wait up to 60s for the operator to open a cached book physically')
    parser.add_argument('--expect-spine', type=int,
                        help='require this zero-based starting spine for a matched-position comparison')
    parser.add_argument('--expect-character', type=int,
                        help='require this starting character together with --expect-spine')
    parser.add_argument('--power-reset', required=True,
                        help='observed power source, reset reason and installation/readback evidence')
    parser.add_argument('--output', type=Path, required=True)
    return parser


def run_periodic(press, idle, mark, monotonic):
    del monotonic
    mark('periodic-turns-begin')
    for index in range(40):
        press(('next', 'next', 'previous')[index % 3], 3, 'periodic-turn')
    # Several independent due periods; retain the first post-save turn too.
    for index in range(3):
        mark('periodic-due-wait-begin', ordinal=index + 1, requested_seconds=34)
        idle(34)
        press('next', 3, 'post-due-turn')
    press('back', 2, 'exit')


def run_continuous_input(press, idle, mark, monotonic):
    del idle
    began = monotonic()
    mark('continuous-input-begin', minimum_seconds=CONTINUOUS_INPUT_SECONDS,
         requested_post_reply_gap_seconds=CONTINUOUS_INPUT_GAP_SECONDS)
    count = 0
    while monotonic() - began < CONTINUOUS_INPUT_SECONDS:
        press(('next', 'next', 'previous')[count % 3], CONTINUOUS_INPUT_GAP_SECONDS,
              'continuous-turn')
        count += 1
    elapsed = monotonic() - began
    mark('continuous-input-complete', elapsed_seconds=elapsed, turns=count,
         note='host timing only; raw save/age rows establish device behavior')
    # The final sub-two-second gap is included in the preceding press. Back is
    # the first request after the sustained turn sequence, so its raw exit row
    # can report the checkpoint age that accumulated while saves were deferred.
    press('back', 2, 'exit-after-continuous-input')


def run_producer_capture(press, idle, mark, monotonic):
    del monotonic
    mark('producer-capture-setup',
         note='intent only; require raw PC target/admission/attempt rows for producer evidence')
    # A turn creates a fresh displayed generation and restarts the quiet clock.
    press('next', 0, 'producer-window-setup-turn')
    mark('producer-start-window-begin', requested_seconds=PRODUCER_START_DELAY_SECONDS)
    idle(PRODUCER_START_DELAY_SECONDS)
    # Sending after the two-second settle point places a DEV byte into an
    # in-progress producer when the prepared cache actually has pending work.
    # The producer may already be complete or absent; only raw PC rows decide.
    press('next', 0.25, 'producer-window-input')
    mark('producer-followup-window-begin', requested_seconds=PRODUCER_FOLLOWUP_SECONDS,
         note='raw capture remains active; scenario label is not proof of producer execution')
    idle(PRODUCER_FOLLOWUP_SECONDS)
    press('back', 2, 'exit-after-producer-window')


SCENARIOS = {
    'periodic': run_periodic,
    'continuous-input': run_continuous_input,
    'producer-running': run_producer_capture,
}


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    if (args.expect_spine is None) != (args.expect_character is None):
        parser.error('--expect-spine and --expect-character must be supplied together')
    if args.phase == 'pending' and not args.wait_reader:
        parser.error('pending capture requires --wait-reader so first-page evidence follows the handshake')
    spec = importlib.util.spec_from_file_location('newt_dev_usb', ROOT / 'tools/dev-usb.py')
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {
        'build': args.expect_build,
        'phase_requested': args.phase,
        'scenario_requested': args.scenario,
        'observed_power_reset': args.power_reset,
        'actions': [],
        'events': [],
        'evidence_limits': [
            'phase and scenario labels record host intent, not observed device state',
            'raw serial rows are required to establish save deferral, exit age, or page-cache producer execution',
        ],
        'hardware_acceptance': False,
    }
    with driver.lease_port(args.port), args.output.with_suffix('.serial').open('wb') as raw:
        class Capture(driver.SerialClient):
            def read_data(self, remaining):
                data = super().read_data(remaining)
                raw.write(data)
                raw.flush()
                return data

        client = None

        def mark(label, **details):
            report['events'].append({'label': label, 'monotonic': time.monotonic(), **details})

        def idle(seconds):
            start = time.monotonic()
            end = start + seconds
            while time.monotonic() < end:
                client.buffer.extend(client.read_data(min(.05, max(0, end - time.monotonic()))))
                # No command is outstanding: drain complete console lines without
                # claiming them as protocol replies. Keep a partial line intact.
                while b'\n' in client.buffer:
                    line, _, rest = client.buffer.partition(b'\n')
                    client.buffer = bytearray(rest)
                    if driver.decode_record(bytes(line) + b'\n') is not None:
                        raise ValueError('Unsolicited protocol response during idle')
                if len(client.buffer) > 8192:
                    raise ValueError('Unbounded peer output')
            report['events'].append({'label': 'idle-complete', 'started_monotonic': start,
                                     'ended_monotonic': time.monotonic(),
                                     'requested_seconds': seconds})

        def press(button, gap, label):
            start = time.monotonic()
            reply = client.request('PRESS ' + button)
            completed = time.monotonic()
            action = {
                'label': label,
                'button': button,
                'started_monotonic': start,
                'completed_monotonic': completed,
                'elapsed_ms': (completed - start) * 1000,
                'requested_post_reply_gap_seconds': gap,
                'replies': reply,
            }
            report['actions'].append(action)
            idle(gap)
            action['next_request_eligible_monotonic'] = time.monotonic()

        try:
            client = Capture(driver.approved_serial(), args.port, 30)
            report['hello'] = client.hello(args.expect_build)
            report['before'] = client.diagnostics()
            if args.wait_reader:
                if report['before'].get('reader_open'):
                    raise ValueError('Begin on Home with the reader closed, then open it after capture starts')
                print('Capture active: physically open the selected cached authored book now.', flush=True)
                deadline = time.monotonic() + 60
                while time.monotonic() < deadline:
                    idle(.1)
                    observed = client.diagnostics()
                    if observed.get('reader_open'):
                        report['opened'] = observed
                        break
                else:
                    raise ValueError('No reader open observed within 60 seconds')
            elif not report['before'].get('reader_open'):
                raise ValueError('Reader is closed; inspect reset evidence and open the book before a fresh settled run')
            start = report.get('opened', report['before'])
            if args.expect_spine is not None:
                expected = (args.expect_spine, args.expect_character)
                actual = (start.get('actual_spine'), start.get('actual_character'))
                if actual != expected:
                    raise ValueError(f'Starting position {actual} differs from requested {expected}')
            if args.phase == 'pending':
                # The raw first-page pending/digest-done rows establish whether
                # the intended pending window was actually captured; this label does not.
                mark('pending-phase-primer-begin')
                for button in ('next', 'next', 'previous', 'next', 'previous', 'next'):
                    press(button, .2, 'pending-phase-primer')
            SCENARIOS[args.scenario](press, idle, mark, time.monotonic)
            report['after'] = client.diagnostics()
            report['status'] = client.request('STATUS')[0]
        except BaseException as error:
            report['error'] = str(error)
            if hasattr(error, 'bridge_receipt'):
                report['protocol'] = error.bridge_receipt
            raise
        finally:
            try:
                if client is not None:
                    client.close()
            finally:
                if client is not None:
                    report['protocol'] = client.receipt()
                args.output.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
