"""Synthetic receipts and fake transport only; never opens USB or private books."""
import copy
import importlib.util
import json
import os
import subprocess
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import reading_acceptance as gate

spec = importlib.util.spec_from_file_location('dev_usb_client', ROOT / 'tools/dev-usb.py')
usb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(usb)

PLAN = {'media_sha256': 'a' * 64, 'library_count': 3,
        'primary': {'index': 0, 'book': 101}, 'sibling': {'index': 1, 'book': 102},
        'rejected': {'index': 2, 'book': 103}}
INSTALL = {'schema': 1, 'target': 'test-x3', 'device': '000000000000002a', 'build': 'b' * 12,
           'binary_sha256': 'c' * 64, 'readback_sha256': 'c' * 64,
           'inputs_sha256': 'd' * 64, 'installed_at': 1000}
EXPECTED = {key: INSTALL[key] for key in ('build', 'binary_sha256', 'inputs_sha256')}
PROCEDURE = 'e' * 64


def changed_fields(body, changes):
    return ' '.join(f'{token.split("=", 1)[0]}={changes[token.split("=", 1)[0]]}'
                    if token.split('=', 1)[0] in changes else token for token in body.split())


def responses(name):
    reading = name in {'primary_open', 'primary_next', 'primary_next2', 'primary_previous',
                      'primary_reopen', 'sibling_open', 'sibling_next', 'recovered_open', 'recovered_next',
                      'recovered_boundary_reopen', 'recovered_boundary_previous', 'recovered_boundary_next',
                      'sibling_recovery_open', 'sibling_recovery_next'}
    library = name in {'library', 'sibling_library', 'rejected_library', 'rejected_open', 'recovered_library', 'sibling_recovery_library'}
    status = {'screen': 4 if reading else 6 if library else 0, 'gen': 1,
              'ready': 1, 'refresh_ms': 800, 'heap': 135364, 'dropped': 0, 'capture': 0}
    diag = {key: 0 for key in gate.DIAG_KEYS}
    diag.update(available=1, reader_open=int(reading), progress_ready=int(reading), largest=114676,
                device='000000000000002a', version=2, opening=0, heap=135364, library_count=3, current_book=102 if name.startswith('sibling') else 101,
                actual_character=100 if name in {'primary_next', 'primary_previous', 'primary_reopen',
                                                'sibling_next', 'recovered_open', 'recovered_boundary_previous',
                                                'sibling_recovery_open'} else
                                 200 if name == 'sibling_recovery_next' else 0,
                actual_spine=1 if name in {'primary_next2', 'recovered_next', 'recovered_boundary_reopen',
                                          'recovered_boundary_next'} else 0)
    if name in {'rejected_open', 'recovered_library'}:
        diag.update(current_book=103, reader_status=7, library_selected=2)
    return 'STATUS ' + ' '.join(f'{k}={v}' for k, v in status.items()), \
           'DIAG ' + ' '.join(f'{k}={v}' for k, v in diag.items())


class FakeClient:
    """Fake one-connection serial-shaped audit, explicitly synthetic test data."""
    def __init__(self, broken=None, old=False, stall=False, overrides=None):
        self.connection = {'transport': 'serial', 'closed': False, 'events': []}
        self.protocol = {'boot': 5, 'session': 2, 'last_request_id': 0x80000000, 'records': []}
        self.request_id = 0x80000000
        self.generation = 1
        self.timeout = 30
        self.stage = 0
        self.presses = []
        self.broken, self.old, self.stall = broken, old, stall
        self.overrides = overrides or {}
        self.event('opening'); self.event('opened')
    def event(self, value):
        self.connection['events'].append({'event': value, 'utc': 'synthetic', 'monotonic': 1})
    def hello(self, expected):
        for session in (1, 2):
            self.protocol['records'].extend([{'direction': 'request', 'kind': 'HELLO', 'request_id': 0, 'boot': None, 'session': None, 'body': 'HELLO', 'monotonic': 100}, {'direction': 'response', 'kind': 'HELLO', 'request_id': 0, 'boot': 5, 'session': session, 'body': f'HELLO build={expected} caps=press,status,frame,diag2,put wake=physical-only', 'monotonic': 100}])
        self.request('STATUS')
        return f'HELLO build={expected} caps=press,status,frame,diag2,put wake=physical-only'
    def request(self, command, capture_deadline=None):
        self.request_id += 1
        self.protocol['last_request_id'] = self.request_id
        kind = command.split()[0]
        def trace(direction, response, body):
            self.protocol['records'].append({'direction': direction, 'boot': 5, 'session': 2,
                                              'request_id': self.request_id, 'kind': response, 'body': body, 'monotonic': 100})
        trace('request', kind, command)
        if kind == 'PRESS':
            self.presses.append(command)
            trace('response', 'ACCEPTED', 'ACCEPTED'); trace('response', 'CONSUMED', 'CONSUMED')
            if self.stall and len(self.presses) == 4:
                raise ValueError('PRESS completion unknown')
            self.generation += 1
            terminal = f'RENDER_RETURN gen={self.generation}'
            trace('response', 'RENDER_RETURN', terminal)
            return ['ACCEPTED', 'CONSUMED', terminal]
        status, diag = responses(gate.STAGES[self.stage])
        diag = changed_fields(diag, self.overrides.get(gate.STAGES[self.stage], {}))
        status = status.replace('gen=1 ', f'gen={self.generation} ')
        if self.broken == gate.STAGES[self.stage]:
            status = status.replace('screen=4', 'screen=6')
            diag = diag.replace('reader_open=1', 'reader_open=0').replace('reader_status=0', 'reader_status=7')
        if self.old:
            diag = 'DIAG available=0'
        trace('response', kind, status if kind == 'STATUS' else diag)
        if kind == 'DIAG':
            self.stage += 1
        return [status if kind == 'STATUS' else diag]
    def close(self):
        self.event('closing'); self.event('closed'); self.connection['closed'] = True
    def receipt(self):
        return {'connection': self.connection, 'protocol': self.protocol}


class SamplingClient(FakeClient):
    """Author-controlled adjacent page pair, retaining all original stage actions."""
    def __init__(self, cycles=20, sample_changes=None, no_visual=False, **kwargs):
        super().__init__(**kwargs)
        self.sample_remaining = 2 * cycles
        self.sample_changes = sample_changes or {}
        self.sample_action = None
        self.no_visual = no_visual
    def request(self, command, capture_deadline=None):
        if self.stage != gate.STAGES.index('primary_next2') or not self.sample_remaining:
            return super().request(command, capture_deadline)
        if command.startswith('PRESS '):
            self.sample_action = command.split()[1]
            replies = super().request(command, capture_deadline)
            if self.no_visual:
                replies[-1] = f'NO_VISUAL_CHANGE gen={self.generation}'
                self.protocol['records'][-1].update(kind='NO_VISUAL_CHANGE', body=replies[-1])
            return replies
        self.request_id += 1
        self.protocol['last_request_id'] = self.request_id
        status, diagnostics = responses('primary_open' if self.sample_action == 'previous' else 'primary_next')
        status = status.replace('gen=1 ', f'gen={self.generation} ')
        diagnostics = changed_fields(diagnostics, self.sample_changes)
        for direction, body in (('request', command), ('response', status if command == 'STATUS' else diagnostics)):
            self.protocol['records'].append({'direction': direction, 'boot': 5, 'session': 2,
                'request_id': self.request_id, 'kind': command, 'body': body, 'monotonic': 100})
        if command == 'DIAG':
            self.sample_remaining -= 1
        return [status if command == 'STATUS' else diagnostics]


class AcceptanceTests(unittest.TestCase):
    def setUp(self):
        self.client = FakeClient()
        self.receipt = gate.run_journey(self.client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
    def validate(self, receipt=None, install=None, plan=None, expected=None, now=1200):
        return gate.validate_journey(receipt or self.receipt, install or INSTALL, plan or PLAN,
                                     expected or EXPECTED, 'test-x3', PROCEDURE, now)
    def test_complete_journey_keeps_physical_acceptance_separate(self):
        self.assertEqual(self.receipt['terminal'], 'complete')
        self.assertEqual(self.validate(), {'state': 'hardware-validated', 'reading': 'pass',
                                          'chapter_crossing': 'pass', 'book_completion': 'unverified',
                                          'battery_wake': 'manual-pending', 'optical_buttons': 'manual-pending', 'owner': 'pending'})
        self.assertEqual([s['name'] for s in self.receipt['stages']], list(gate.STAGES))
    def test_real_transport_receipt_stays_outside_acceptance_schema(self):
        class RealReceiptClient(FakeClient):
            receipt = usb.ProtocolClient.receipt
            def __init__(self):
                super().__init__()
                self.boot, self.session = 5, 2
                self.audit = self.protocol['records']
        client = RealReceiptClient()
        actual_receipt = client.receipt
        # Future transport fields must not leak into the exact acceptance contract.
        with patch.object(client, 'receipt', lambda: {**actual_receipt(), 'transport_extension': 1}):
            receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.validate(receipt)
        self.assertNotIn('transport_extension', receipt)
        receipt['transport_extension'] = 1
        with self.assertRaisesRegex(ValueError, 'Missing or unexpected reading evidence fields'):
            self.validate(receipt)

    def test_latency_captures_every_current_press_without_changing_journey(self):
        actions = self.receipt['latency']['actions']
        presses = [operation for operation in self.receipt['operations'] if operation['command'].startswith('PRESS ')]
        self.assertEqual(self.receipt['latency']['schema'], 1)
        self.assertEqual(len(actions), len(presses))
        for action, press in zip(actions, presses):
            self.assertEqual(set(action), {'request_id', 'stage', 'action', 'started', 'finished', 'telemetry'})
            self.assertEqual(action['request_id'], press['request_id'])
            self.assertEqual(action['action'], press['command'].split()[1])
            self.assertIn(action['stage'], gate.STAGES)
            self.assertEqual((action['started'], action['finished'], action['telemetry']), (100, 100, []))
        historical = copy.deepcopy(self.receipt)
        historical.pop('latency')
        self.assertEqual(self.validate(historical), self.validate())  # Correctness never grants latency authority.
    def test_latency_preserves_partial_press_and_never_retries_wrong_ack(self):
        stalled = FakeClient(stall=True)
        receipt = gate.run_journey(stalled, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(len(stalled.presses), 4)
        self.assertIsNone(receipt['latency']['actions'][-1]['finished'])
        self.assertEqual(receipt['protocol']['records'][-1]['kind'], 'CONSUMED')
        class WrongAcknowledgement(FakeClient):
            def request(self, command, capture_deadline=None):
                replies = super().request(command, capture_deadline)
                if command.startswith('PRESS '):
                    return ['CONSUMED', 'ACCEPTED', replies[-1]]
                return replies
        wrong = WrongAcknowledgement()
        receipt = gate.run_journey(wrong, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(len(wrong.presses), 1)
        self.assertIsNone(receipt['latency']['actions'][-1]['finished'])
        self.assertTrue(receipt['connection']['closed'])
    def test_latency_keeps_only_matching_press_telemetry(self):
        client = FakeClient()
        client.press_telemetry = [{'boot': 5, 'session': 2, 'request_id': 0x80000000 + 4,
            'tag': '8279_DIRECT_GRAY_DRF', 'monotonic': 100},
            {'boot': 99, 'session': 2, 'request_id': 0x80000000 + 4,
             'tag': '8279_ISLAND_POF', 'monotonic': 100}]
        receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(receipt['latency']['actions'][0]['telemetry'],
                         [{'tag': '8279_DIRECT_GRAY_DRF', 'monotonic': 100}])
        self.assertTrue(all(not action['telemetry'] for action in receipt['latency']['actions'][1:]))
    def test_oversized_journey_is_rejected_before_any_connection(self):
        plan = copy.deepcopy(PLAN)
        plan['library_count'] = 128
        self.assertGreater(gate.journey_audit_bound(plan), gate.MAX_AUDIT_RECORDS)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'receipt.json'
            with patch.object(gate, 'run_journey') as journey:
                with patch('reading_acceptance.os.open') as create:
                    with self.assertRaisesRegex(ValueError, 'before connection'):
                        gate.write_journey(output, lambda: self.fail('Connection must not open'), INSTALL, plan, PROCEDURE)
                create.assert_not_called()
                journey.assert_not_called()
    def sampled_receipt(self, client=None, cycles=20):
        return gate.run_journey(client or SamplingClient(cycles), INSTALL, PLAN, PROCEDURE,
                                clock=lambda: 1100, monotonic=lambda: 100, sampling_cycles=cycles)
    def test_bounded_sampler_preserves_stages_and_returns_before_chapter_crossing(self):
        receipt = self.sampled_receipt()
        self.assertEqual(receipt['terminal'], 'complete')
        self.assertEqual(self.validate(receipt), self.validate())
        self.assertEqual([stage['name'] for stage in receipt['stages']], list(gate.STAGES))
        self.assertEqual(receipt['sampling']['stage'], 'primary_next')
        self.assertEqual(receipt['sampling']['cycles'], 20)
        samples = receipt['sampling']['observations']
        self.assertEqual([sample['action'] for sample in samples], ['previous', 'next'] * 20)
        self.assertEqual(len(receipt['operations']), len(self.receipt['operations']) + 120)
        self.assertEqual(len(receipt['protocol']['records']), len(self.receipt['protocol']['records']) + 320)
        original = next(stage for stage in receipt['stages'] if stage['name'] == 'primary_next')
        self.assertEqual(receipt['sampling']['initial'], {key: original[key] for key in ('status', 'diagnostics', 'request_id')})
        self.assertEqual(gate.anchor(gate.observation(samples[-1]['status'], samples[-1]['diagnostics'])[1]),
                         gate.anchor(gate.observation(original['status'], original['diagnostics'])[1]))
        self.assertEqual(len(receipt['latency']['actions']), len(self.receipt['latency']['actions']) + 40)
    def test_sampler_stops_before_first_repeat_for_an_unobserved_same_spine_pair(self):
        client = SamplingClient(overrides={'primary_next': {'actual_spine': 1}})
        receipt = self.sampled_receipt(client)
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(receipt['failure_detail']['phase'], 'sampling-precondition')
        self.assertEqual(receipt['sampling']['observations'], [])
        expected = [operation['command'] for operation in self.receipt['operations']
                    if operation['request_id'] <= self.receipt['stages'][3]['request_id'] and operation['command'].startswith('PRESS ')]
        self.assertEqual(client.presses, expected)
    def test_sampler_stops_on_anchor_or_no_visual_failure_without_retry(self):
        for client in (SamplingClient(sample_changes={'actual_character': 1}), SamplingClient(no_visual=True)):
            receipt = self.sampled_receipt(client)
            self.assertEqual(receipt['terminal'], 'failed')
            self.assertEqual(receipt['failure_detail']['phase'], 'sampling-observation')
            self.assertEqual(len(receipt['sampling']['observations']), 1)
            self.assertEqual(client.presses[-1], 'PRESS previous')
            self.assertEqual(client.sample_remaining, 39)
            self.assertTrue(receipt['connection']['closed'])
    def test_sampling_wire_observations_cycles_and_return_cannot_be_forged(self):
        receipt = self.sampled_receipt()
        for field, value in (('cycles', 0), ('cycles', 19), ('cycles', 101), ('cycles', True),
                             ('stage', 'primary_previous'), ('initial', {}), ('observations', [])):
            malformed = copy.deepcopy(receipt)
            malformed['sampling'][field] = value
            with self.assertRaises(ValueError): self.validate(malformed)
        # Co-edit the reported sample, operation and audited wire identically;
        # semantic same-anchor checks still reject it independently of hashes.
        malformed = copy.deepcopy(receipt)
        sample = malformed['sampling']['observations'][0]
        sample['diagnostics'] = changed_fields(sample['diagnostics'], {'actual_character': 1})
        next(operation for operation in malformed['operations'] if operation['request_id'] == sample['request_id'])['responses'] = [sample['diagnostics']]
        next(record for record in malformed['protocol']['records'] if record['request_id'] == sample['request_id'] and record['direction'] == 'response')['body'] = sample['diagnostics']
        with self.assertRaisesRegex(ValueError, 'known anchor'): self.validate(malformed)
    def test_sampler_cap_and_configuration_reject_before_connecting(self):
        self.assertLessEqual(gate.journey_audit_bound(PLAN, 100), gate.MAX_AUDIT_RECORDS)
        self.assertEqual(self.sampled_receipt(cycles=100)['terminal'], 'complete')
        oversized = copy.deepcopy(PLAN)
        oversized['library_count'] = 100
        for cycles, plan in ((19, PLAN), (101, PLAN), (True, PLAN), (100, oversized)):
            with tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValueError):
                    gate.write_journey(Path(directory) / 'receipt.json', lambda: self.fail('Must not connect'),
                                       INSTALL, plan, PROCEDURE, sampling_cycles=cycles)
    def test_post_sample_chapter_generation_cannot_regress_to_original_origin(self):
        receipt = self.sampled_receipt()
        stage = next(stage for stage in receipt['stages'] if stage['name'] == 'primary_next2')
        original = next(stage for stage in receipt['stages'] if stage['name'] == 'primary_next')
        generation = gate.observation(original['status'], original['diagnostics'])[0]['gen'] + 1
        stage['status'] = changed_fields(stage['status'], {'gen': generation})
        identifier = stage['request_id']
        for operation in receipt['operations']:
            if operation['request_id'] == identifier - 2:
                operation['responses'][-1] = f'RENDER_RETURN gen={generation}'
            elif operation['request_id'] == identifier - 1:
                operation['responses'] = [stage['status']]
        for record in receipt['protocol']['records']:
            if record['request_id'] == identifier - 2 and record['kind'] == 'RENDER_RETURN':
                record['body'] = f'RENDER_RETURN gen={generation}'
            elif record['request_id'] == identifier - 1 and record['direction'] == 'response':
                record['body'] = stage['status']
        with self.assertRaisesRegex(ValueError, 'regressed after warm sampling'): self.validate(receipt)
    def coedit_observation(self, receipt, name, kind='DIAG', **changes):
        """Adversarial semantic control: keep stage, operation, and wire identical."""
        stage = next(s for s in receipt['stages'] if s['name'] == name)
        field = 'diagnostics' if kind == 'DIAG' else 'status'
        stage[field] = changed_fields(stage[field], changes)
        identifier = stage['request_id'] - (kind == 'STATUS')
        next(op for op in receipt['operations'] if op['request_id'] == identifier)['responses'] = [stage[field]]
        next(r for r in receipt['protocol']['records'] if r['request_id'] == identifier and
             r['direction'] == 'response')['body'] = stage[field]
    def test_same_spine_higher_character_cannot_count_as_chapter_crossing(self):
        receipt = copy.deepcopy(self.receipt)
        self.coedit_observation(receipt, 'primary_next2', actual_spine=0, actual_character=200)
        with self.assertRaisesRegex(ValueError, 'chapter boundary'): self.validate(receipt)
    def test_failed_reader_cannot_count_as_crossing_with_new_render_generation(self):
        receipt = copy.deepcopy(self.receipt)
        stage = next(s for s in receipt['stages'] if s['name'] == 'primary_next2')
        self.assertGreater(gate.fields(stage['status'], 'STATUS', gate.STATUS_KEYS)['gen'],
                           gate.fields(receipt['stages'][3]['status'], 'STATUS', gate.STATUS_KEYS)['gen'])
        self.coedit_observation(receipt, 'primary_next2', reader_status=7)
        with self.assertRaisesRegex(ValueError, 'unreadable book'): self.validate(receipt)
    def test_next_chapter_reopen_loss_and_crossing_drift_are_rejected(self):
        for name, spine, character in (('recovered_next', 1, 1),
                                       ('recovered_boundary_reopen', 0, 100),
                                       ('recovered_boundary_reopen', 1, 1),
                                       ('recovered_boundary_previous', 0, 101),
                                       ('recovered_boundary_next', 1, 1)):
            with self.subTest(stage=name, spine=spine, character=character):
                receipt = copy.deepcopy(self.receipt)
                self.coedit_observation(receipt, name, actual_spine=spine, actual_character=character)
                with self.assertRaisesRegex(ValueError, 'saved-position'): self.validate(receipt)
    def test_historical_23_stage_journey_cannot_pass_chapter_extension(self):
        receipt = copy.deepcopy(self.receipt)
        receipt['stages'] = [s for s in receipt['stages'] if not s['name'].startswith('recovered_boundary_')]
        self.assertEqual(len(receipt['stages']), 23)
        with self.assertRaisesRegex(ValueError, 'journey stages'): self.validate(receipt)
    def test_chapter_failures_stop_live_journey_before_further_navigation(self):
        for name, changes, last_press in (
                ('primary_next2', {'actual_spine': 0, 'actual_character': 200}, 'PRESS next'),
                ('primary_next2', {'reader_status': 7}, 'PRESS next'),
                ('recovered_boundary_reopen', {'actual_spine': 0, 'actual_character': 100}, 'PRESS back')):
            with self.subTest(stage=name, changes=changes):
                client = FakeClient(overrides={name: changes})
                receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE,
                                           clock=lambda: 1100, monotonic=lambda: 100)
                self.assertEqual(receipt['terminal'], 'failed')
                self.assertEqual(receipt['failure_detail']['stage'], name)
                self.assertEqual(receipt['stages'][-1]['name'], name)
                self.assertEqual(client.presses[-1], last_press)
                self.assertTrue(receipt['connection']['closed'])
                with self.assertRaises(ValueError): self.validate(receipt)
    def test_original_heap_rejection_and_sibling_fail_before_further_presses(self):
        for stage in ('primary_open', 'sibling_open'):
            client = FakeClient(broken=stage)
            receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
            self.assertEqual(receipt['terminal'], 'failed')
            self.assertEqual(client.presses[-1], 'PRESS activate')
            with self.assertRaises(ValueError): self.validate(receipt)
    def test_old_diagnostics_fail_before_any_device_input(self):
        client = FakeClient(old=True)
        receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(client.presses, [])
        with self.assertRaises(ValueError): self.validate(receipt)
    def test_stalled_open_is_never_retried_or_recovered(self):
        client = FakeClient(stall=True)
        receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(len(client.presses), 4)
        self.assertTrue(receipt['connection']['closed'])
        self.assertEqual(receipt['failure_detail']['message'], 'PRESS completion unknown')
        with self.assertRaises(ValueError): self.validate(receipt)
    def test_sibling_no_ack_preserves_request_failure_without_retry(self):
        class NoAckClient(FakeClient):
            def request(self, command, capture_deadline=None):
                if self.stage != 10 or command != 'PRESS activate':
                    return super().request(command, capture_deadline)
                self.request_id += 1
                self.protocol['last_request_id'] = self.request_id
                self.presses.append(command)
                self.protocol['records'].append({'direction': 'request', 'boot': 5,
                    'session': 2, 'request_id': self.request_id, 'kind': 'PRESS',
                    'body': command, 'monotonic': 100})
                raise ValueError('Timed out waiting for current protocol record')
        client = NoAckClient()
        receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE,
                                   clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(len(receipt['stages']), 10)
        self.assertEqual(receipt['failure_detail'], {'phase': 'request',
            'stage': 'sibling_open', 'command': 'PRESS activate', 'type': 'ValueError',
            'message': 'Timed out waiting for current protocol record'})
        self.assertTrue(receipt['connection']['closed'])
        self.assertEqual(client.protocol['records'][-1]['direction'], 'request')
        self.assertEqual(client.protocol['records'][-1]['body'], 'PRESS activate')
        with self.assertRaises(ValueError): self.validate(receipt)
    def test_closure_failure_keeps_original_failure_and_bounded_messages(self):
        class CloseFailure(FakeClient):
            def close(self):
                super().close()
                raise OSError('x' * 1024)
        receipt = gate.run_journey(CloseFailure(stall=True), INSTALL, PLAN, PROCEDURE,
                                   clock=lambda: 1100, monotonic=lambda: 100)
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(receipt['failure_detail']['message'], 'PRESS completion unknown')
        self.assertEqual(receipt['close_failure_detail']['phase'], 'connection-close')
        self.assertEqual(receipt['close_failure_detail']['type'], 'OSError')
        self.assertEqual(len(receipt['close_failure_detail']['message']), 512)
        with self.assertRaises(ValueError): self.validate(receipt)
        self.assertEqual(gate.failure_detail(KeyboardInterrupt(), 'request')['message'], 'KeyboardInterrupt')
    def test_actual_press_wrapper_retains_write_or_response_failure(self):
        class FailingTransport(usb.ProtocolClient):
            def __init__(self, broken_write=False):
                super().__init__(timeout=30)
                self.boot, self.session = 5, 2
                self.broken_write = broken_write
                self.writes = 0
            def write_data(self, data, remaining):
                self.writes += 1
                if self.broken_write:
                    raise OSError('synthetic partial write')
            def record(self, deadline):
                raise ValueError('Protocol response timed out')
        for broken_write in (False, True):
            client = FailingTransport(broken_write)
            with self.assertRaisesRegex(ValueError, 'PRESS completion unknown') as caught:
                client.request('PRESS activate')
            detail = gate.failure_detail(caught.exception, 'request', 'sibling_open', 'PRESS activate')
            self.assertEqual(detail['press_delivery'], {'host_write_completed': not broken_write,
                                                       'validated_replies': 0})
            self.assertEqual(detail['cause'], {'type': 'OSError' if broken_write else 'ValueError',
                'message': 'synthetic partial write' if broken_write else 'Protocol response timed out'})
            self.assertEqual(client.writes, 1)
            self.assertEqual(len(client.receipt()['protocol']['records']), 1)
    def test_missing_skipped_unknown_partial_and_copy_only_do_not_pass(self):
        for mutate in (lambda r: r.pop('stages'), lambda r: r['stages'].pop(),
                       lambda r: r['stages'][2].update(name='skipped'),
                       lambda r: r.update(terminal='running'), lambda r: r.update(stages=[])):
            receipt = copy.deepcopy(self.receipt); mutate(receipt)
            with self.assertRaises(ValueError): self.validate(receipt)
    def test_stale_future_preinstall_wrongbuild_and_procedure(self):
        for mutate in (lambda r: r.update(finished_at=999),
                       lambda r: r.update(started_at=999), lambda r: r.update(finished_at=1300),
                       lambda r: r['identity'].update(binary_sha256='f' * 64),
                       lambda r: r.update(procedure_sha256='f' * 64),
                       lambda r: r.update(plan_sha256='f' * 64),
                       lambda r: r.update(hello=r['hello'].replace('b' * 12, 'f' * 12))):
            receipt = copy.deepcopy(self.receipt); mutate(receipt)
            with self.assertRaises(ValueError): self.validate(receipt)
        with self.assertRaises(ValueError): self.validate(now=90000)
    def test_wrongdevice_reset_fixture_connection_failure_and_unknown_completion(self):
        for mutate in (lambda r: r.update(simulation=True),
                       lambda r: r['connection'].update(transport='fixture'),
                       lambda r: r['connection'].update(closed=False),
                       lambda r: r['connection']['events'].append({'event': 'opened'}),
                       lambda r: r['protocol']['records'][-1].update(boot=6),
                       lambda r: r['protocol']['records'].pop(5),
                       lambda r: r['stages'][2].update(diagnostics=r['stages'][2]['diagnostics'].replace('device=000000000000002a', 'device=000000000000002b'))):
            receipt = copy.deepcopy(self.receipt); mutate(receipt)
            with self.assertRaises(ValueError): self.validate(receipt)
    def test_anchor_failure_wrongbook_heap_and_unresponsive_lifecycle(self):
        for old, new in (('actual_character=100', 'actual_character=0'),
                         ('current_book=101', 'current_book=102'),
                         ('largest=114676', 'largest=0'), ('reader_open=1', 'reader_open=0')):
            receipt = copy.deepcopy(self.receipt)
            receipt['stages'][3]['diagnostics'] = receipt['stages'][3]['diagnostics'].replace(old, new)
            with self.assertRaises(ValueError): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        receipt['stages'][-1]['status'] = receipt['stages'][-1]['status'].replace('ready=1', 'ready=0')
        with self.assertRaises(ValueError): self.validate(receipt)
    def test_private_json_corrupt_duplicate_partial_and_oversized(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipt.json'
            for text in ('{', '{"schema":1,"schema":1}', '{"time":NaN}', ' ' * (1024 * 1024 + 1)):
                path.write_text(text)
                with self.assertRaises(ValueError): gate.load(path)
    def test_operations_are_mandatory_and_observations_bound_to_live_responses(self):
        receipt = copy.deepcopy(self.receipt)
        # Calibrated prior failure: remove every press and renumber all surviving
        # request IDs/observations so the old continuity-only validator accepted.
        removed = {r['request_id'] for r in receipt['protocol']['records'] if r['kind'] == 'PRESS'}
        records = [r for r in receipt['protocol']['records'] if r['request_id'] not in removed]
        kept = [r['request_id'] for r in records if r['direction'] == 'request' and r['request_id']]
        mapping = {old: 0x80000001 + i for i, old in enumerate(kept)}
        for r in records:
            if r['request_id']: r['request_id'] = mapping[r['request_id']]
        receipt['protocol']['records'] = records
        receipt['protocol']['last_request_id'] = max(mapping.values())
        for stage in receipt['stages']: stage['request_id'] = mapping[stage['request_id']]
        receipt['operations'] = [op for op in receipt['operations'] if op['request_id'] not in removed]
        for op in receipt['operations']: op['request_id'] = mapping[op['request_id']]
        with self.assertRaisesRegex(ValueError, 'fixed reading journey'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        receipt['stages'][3]['diagnostics'] = receipt['stages'][3]['diagnostics'].replace('actual_character=100', 'actual_character=101')
        with self.assertRaisesRegex(ValueError, 'actual request responses'): self.validate(receipt)
    def test_producer_body_binding_rejects_coedited_stage_and_operation(self):
        receipt = copy.deepcopy(self.receipt)
        stage = receipt['stages'][3]
        stage['diagnostics'] = stage['diagnostics'].replace('actual_character=100', 'actual_character=101')
        op = next(op for op in receipt['operations'] if op['request_id'] == stage['request_id'])
        op['responses'] = [stage['diagnostics']]
        with self.assertRaisesRegex(ValueError, 'live request'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        next(op for op in receipt['operations'] if op['command'] == 'PRESS previous')['command'] = 'PRESS next'
        with self.assertRaisesRegex(ValueError, 'live request'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        receipt['protocol']['records'][1]['body'] = receipt['hello'].replace('b' * 12, 'f' * 12)
        with self.assertRaisesRegex(ValueError, 'HELLO'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        for r in receipt['protocol']['records']: r.pop('body')
        with self.assertRaisesRegex(ValueError, 'wire audit'): self.validate(receipt)
    def test_cross_book_and_failed_open_recovery_preserve_primary_checkpoint(self):
        receipt = copy.deepcopy(self.receipt)
        stage = next(s for s in receipt['stages'] if s['name'] == 'recovered_open')
        stage['diagnostics'] = stage['diagnostics'].replace('actual_character=100', 'actual_character=0')
        op = next(op for op in receipt['operations'] if op['request_id'] == stage['request_id'])
        op['responses'] = [stage['diagnostics']]
        next(r for r in receipt['protocol']['records'] if r['request_id'] == stage['request_id'] and r['direction'] == 'response')['body'] = stage['diagnostics']
        with self.assertRaisesRegex(ValueError, 'saved-position'): self.validate(receipt)
    def test_reader_anchor_changes_cannot_substitute_for_frame_publication(self):
        receipt = copy.deepcopy(self.receipt)
        stage = receipt['stages'][3]
        old = receipt['stages'][2]['status'].split('gen=')[1].split()[0]
        current = stage['status'].split('gen=')[1].split()[0]
        stage['status'] = stage['status'].replace('gen=' + current + ' ', 'gen=' + old + ' ')
        op = next(op for op in receipt['operations'] if op['request_id'] == stage['request_id'] - 1)
        op['responses'] = [stage['status']]
        record = next(r for r in receipt['protocol']['records'] if r['request_id'] == op['request_id'] and r['direction'] == 'response')
        record['body'] = stage['status']
        with self.assertRaisesRegex(ValueError, 'framebuffer'): self.validate(receipt)
    def test_sibling_checkpoint_loss_after_rejection_is_also_rejected(self):
        receipt = copy.deepcopy(self.receipt)
        stage = next(s for s in receipt['stages'] if s['name'] == 'sibling_recovery_open')
        stage['diagnostics'] = stage['diagnostics'].replace('actual_character=100', 'actual_character=0')
        op = next(op for op in receipt['operations'] if op['request_id'] == stage['request_id'])
        op['responses'] = [stage['diagnostics']]
        next(r for r in receipt['protocol']['records'] if r['request_id'] == stage['request_id'] and r['direction'] == 'response')['body'] = stage['diagnostics']
        with self.assertRaisesRegex(ValueError, 'saved-position'): self.validate(receipt)
    def test_wallclock_cannot_hide_unbounded_monotonic_requests(self):
        receipt = copy.deepcopy(self.receipt)
        receipt['protocol']['records'][-1]['monotonic'] = 131
        receipt['finished_monotonic'] = 131
        with self.assertRaisesRegex(ValueError, 'terminal deadline'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        receipt['finished_monotonic'] = 701
        with self.assertRaisesRegex(ValueError, 'monotonic journey'): self.validate(receipt)
        receipt = copy.deepcopy(self.receipt)
        receipt['protocol']['records'][2]['monotonic'] = 99
        with self.assertRaisesRegex(ValueError, 'monotonic audit'): self.validate(receipt)
        client = FakeClient()
        ticks = iter([100, 701, 701])
        receipt = gate.run_journey(client, INSTALL, PLAN, PROCEDURE, clock=lambda: 1100,
                                   monotonic=lambda: next(ticks, 701))
        self.assertEqual(receipt['terminal'], 'failed')
        self.assertEqual(client.presses, [])
    def test_opening_failure_keeps_existing_client_failure_audit_privately(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'journey.json'
            def broken():
                error = OSError('test-only port unavailable')
                error.bridge_receipt = {'connection': {'transport': 'serial', 'closed': True,
                    'events': [{'event': 'opening'}, {'event': 'closing'}, {'event': 'closed'}]}}
                raise error
            receipt = gate.write_journey(destination, broken, INSTALL, PLAN, PROCEDURE)
            self.assertEqual(receipt['terminal'], 'failed')
            self.assertEqual(receipt['failure_detail'], {'phase': 'connection-opening',
                'stage': None, 'command': None, 'type': 'OSError',
                'message': 'test-only port unavailable'})
            self.assertEqual(gate.load(destination)['connection']['events'][0]['event'], 'opening')
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(ValueError): self.validate(receipt)
            with self.assertRaises(FileExistsError): gate.write_journey(destination, broken, INSTALL, PLAN, PROCEDURE)
    def test_current_acceptance_source_and_relevant_executable_caches_are_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Derive fixture inputs from the owner; the real checks below stay live.
            paths = []
            with patch.object(gate, 'authored_procedure_files', side_effect=lambda _, names: names), \
                    patch.object(gate, 'tree_hash', side_effect=lambda _, names: paths.extend(names)):
                gate.procedure_identity(ROOT)
            paths = [path.relative_to(ROOT).as_posix() for path in paths]
            for name in paths:
                path = root / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(b'procedure')
            before = gate.procedure_identity(root)
            for name in paths:
                helper = root / name
                helper.write_bytes(b'changed procedure')
                self.assertNotEqual(before, gate.procedure_identity(root))
                helper.write_bytes(b'procedure')
            cache = root / 'tools/__pycache__/reading_acceptance.cpython-314.pyc'
            cache.parent.mkdir(); cache.write_bytes(b'actual executable acceptance cache')
            after = gate.procedure_identity(root)
            self.assertNotEqual(before, after)
            cache.write_bytes(b'changed executable cache with identical source bytes')
            self.assertNotEqual(after, gate.procedure_identity(root))
            cache.unlink()
            (cache.parent / 'unrelated.cpython-314.pyc').write_bytes(b'unrelated')
            self.assertEqual(before, gate.procedure_identity(root))
            with patch.object(sys, 'pycache_prefix', str(root / 'alternate')):
                with self.assertRaisesRegex(ValueError, 'cache prefix'): gate.procedure_identity(root)
    def test_entry_points_remove_ambient_cache_prefix_for_descendant_tree(self):
        probe = ("import runpy,sys,subprocess;sys.path.insert(0,sys.argv[2]);"
                 "runpy.run_path(sys.argv[1],run_name='launcher_test');"
                 "subprocess.run([sys.executable,'-B','-c',"
                 "'import os,sys;assert sys.pycache_prefix is None;assert \"PYTHONPYCACHEPREFIX\" not in os.environ'],check=True)")
        with tempfile.TemporaryDirectory() as directory:
            environment = dict(os.environ, PYTHONPYCACHEPREFIX=directory)
            for name in ('tools/check.py', 'tools/reading-ready.py'):
                result = subprocess.run([sys.executable, '-B', '-c', probe, str(ROOT / name), str(ROOT / 'tools')],
                                        check=False, capture_output=True, text=True, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr)
    def test_fixture_plan_and_install_readback_cannot_substitute(self):
        plan = copy.deepcopy(PLAN); plan['sibling'] = plan['primary']
        with self.assertRaises(ValueError): self.validate(plan=plan)
        install = copy.deepcopy(INSTALL); install['readback_sha256'] = 'f' * 64
        with self.assertRaises(ValueError): self.validate(install=install)


if __name__ == '__main__': unittest.main()
