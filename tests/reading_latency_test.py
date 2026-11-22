"""Authored synthetic wire receipts only; no device, media or optical evidence."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import reading_acceptance as journey
import reading_latency as latency

spec = importlib.util.spec_from_file_location('authored_reading_fixtures', ROOT / 'tests/reading_acceptance_test.py')
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)
CONTRACT = (ROOT / 'docs/reading-latency-contract.json').read_bytes()


def bundle(sampled=True, factor=1):
    client = fixtures.SamplingClient() if sampled else fixtures.FakeClient()
    receipt = journey.run_journey(client, fixtures.INSTALL, fixtures.PLAN, fixtures.PROCEDURE,
                                  clock=lambda: 1100, monotonic=lambda: 100,
                                  sampling_cycles=20 if sampled else 0)
    # Assign authored monotonic points to each already validated wire event.
    records = receipt['protocol']['records']
    for index, record in enumerate(records):
        record['monotonic'] = 100 + index * .002 * factor
    receipt['started_monotonic'] = 99
    receipt['finished_monotonic'] = records[-1]['monotonic'] + 1
    for action in receipt['latency']['actions']:
        matches = [r for r in records if r['request_id'] == action['request_id']]
        start, finish = matches[0]['monotonic'], matches[-1]['monotonic']
        action.update(started=start - .001 * factor, finished=finish + .001 * factor,
                      telemetry=[{'tag': '8279_ISLAND_POF', 'monotonic': (start + finish) / 2}])
    return {'journey': receipt, 'install': copy.deepcopy(fixtures.INSTALL),
            'plan': copy.deepcopy(fixtures.PLAN), 'expected': copy.deepcopy(fixtures.EXPECTED),
            'target': 'test-x3', 'procedure_sha256': fixtures.PROCEDURE,
            'source_revision': 'b' * 40, 'image_bytes': 1234567,
            'mode': 'physical-usb-serial',
            'conditions': {'power': 'authored synthetic fixture', 'environment': 'authored synthetic fixture'}}


def approved(candidate, baseline=None, noise=0):
    baseline = copy.deepcopy(candidate) if baseline is None else baseline
    raw = json.dumps(baseline, sort_keys=True).encode()
    groups = latency.summarize(latency.derive_samples(candidate['journey']), [500, 750, 1000])
    policy = {'schema': 1, 'contract_sha256': latency.CONTRACT_SHA256,
              'approved_baseline_sha256': hashlib.sha256(raw).hexdigest(),
              'review': {'reviewer': 'authored fixture', 'reason': 'Synthetic comparison control only'},
              'goals_ms': [500, 750, 1000], 'comparison_statistics': ['p50', 'p95', 'max'],
              'groups': [{**{k: g[k] for k in ('action_class', 'refresh_class', 'endpoint_name')},
                          'min_samples': 20, 'noise_bound_ms': noise} for g in groups if g['required']]}
    return policy, raw


class ReadingLatencyTest(unittest.TestCase):
    def setUp(self):
        self.current = bundle()
        self.policy, self.raw = approved(self.current)

    def evaluate(self, current=None, policy=None, raw=None):
        return latency.evaluate_latency(current or self.current, policy or self.policy,
                                        CONTRACT, 1200, self.raw if raw is None else raw)

    def test_nearest_rank_retains_individual_samples(self):
        result = latency.statistics(list(range(1, 21)))
        self.assertEqual((result['n'], result['p50'], result['p95'], result['max']), (20, 10, 19, 20))
        self.assertEqual(result['samples'], list(range(1, 21)))
        self.assertEqual(latency.statistics([2, 1])['p95'], 2)
        for samples in ([], [True], [float('nan')], [-1]):
            with self.subTest(samples=samples), self.assertRaises(ValueError):
                latency.statistics(samples)

    def test_real_ordered_wire_endpoints_and_required_groups(self):
        result = self.evaluate()
        self.assertEqual(result['state'], 'pass')
        self.assertEqual(len(result['comparisons']), 12)
        groups = [g for g in result['groups'] if g['required']]
        self.assertEqual(len(groups), 4)
        self.assertTrue(all(g['n'] == 20 and g['distribution'] == 'empirical' for g in groups))
        action = result['actions'][0]
        self.assertAlmostEqual(action['wrapper_completion_ms'], 8)
        self.assertAlmostEqual(action['protocol_request_to_terminal_ms'], 6)
        self.assertAlmostEqual(action['acknowledgement_ms']['ACCEPTED'], 2)
        self.assertAlmostEqual(action['acknowledgement_ms']['CONSUMED'], 4)
        self.assertEqual(result['physical_optical'], 'unmeasured')
        self.assertFalse(result['physical_optical_measurement'])
        public = json.dumps(result)
        for value in (fixtures.INSTALL['device'], fixtures.PLAN['media_sha256'], 'authored synthetic fixture'):
            self.assertNotIn(value, public)

    def test_null_reviewed_baseline_blocks_but_retains_measurements(self):
        policy = json.loads((ROOT / 'docs/reading-latency-policy.json').read_text())
        result = latency.evaluate_latency(self.current, policy, CONTRACT, 1200)
        self.assertEqual(result['state'], 'blocked')
        self.assertTrue(result['groups'])
        self.assertIn('No approved raw baseline SHA-256', result['reasons'])
        self.assertTrue(all(g['goals'][0]['within_goal'] is None
                            for g in result['groups'] if not g['required']))

    def test_original_journey_is_measurement_only_without_sampler(self):
        current = bundle(sampled=False)
        policy, raw = approved(current)
        result = self.evaluate(current, policy, raw)
        self.assertEqual(result['state'], 'blocked')
        self.assertIn('No validated ordinary-navigation sampling run', result['reasons'])
        self.assertTrue(all(g['distribution'] == 'provisional' for g in result['groups']))

    def test_regression_and_explicit_noise_bound(self):
        slower = bundle(factor=2)
        result = self.evaluate(slower)
        self.assertEqual(result['state'], 'regression')
        self.assertTrue(any(not c['within_limit'] for c in result['comparisons']))
        policy = copy.deepcopy(self.policy)
        for rule in policy['groups']:
            rule['noise_bound_ms'] = 9
        self.assertEqual(self.evaluate(slower, policy)['state'], 'pass')

    def test_goals_do_not_replace_stable_correctness_regression_policy(self):
        slower = bundle(factor=200)
        policy, raw = approved(slower)
        result = self.evaluate(slower, policy, raw)
        self.assertEqual(result['state'], 'pass')
        self.assertTrue(all(not g['goals'][-1]['within_goal']
                            for g in result['groups'] if g['required']))

    def test_goal_statistic_mapping_and_inclusive_thresholds(self):
        def goals_for(values, required=True):
            samples = [{'action_class': 'ordinary_navigation/next',
                        'refresh_class': 'gray_island_driver_tag', 'required': required,
                        **{endpoint: value for endpoint in latency.ENDPOINTS}} for value in values]
            return latency.summarize(samples, [500, 750, 1000])[0]['goals']
        goals = goals_for([450] * 10 + [700] * 9 + [900])
        self.assertEqual([(g['statistic'], g['measured_ms'], g['threshold_ms']) for g in goals],
                         [('p50', 450, 500), ('p95', 700, 750), ('max', 900, 1000)])
        self.assertTrue(all(g['within_goal'] and g['distribution_sufficient'] for g in goals))
        boundary = goals_for([500] * 10 + [750] * 9 + [1000])
        self.assertTrue(all(g['within_goal'] for g in boundary))
        exceeded = goals_for([501] * 10 + [751] * 9 + [1001])
        self.assertTrue(all(g['within_goal'] is False for g in exceeded))
        provisional = goals_for([900], required=False)
        self.assertTrue(all(g['within_goal'] is None and not g['distribution_sufficient'] for g in provisional))
        self.assertTrue(all(g['measured_ms'] == 900 for g in provisional))

    def test_changed_contract_is_never_promoted(self):
        with self.assertRaises(ValueError):
            latency.evaluate_latency(self.current, self.policy, CONTRACT + b' ', 1200, self.raw)
        policy = copy.deepcopy(self.policy)
        policy['contract_sha256'] = 'f' * 64
        with self.assertRaises(ValueError):
            self.evaluate(policy=policy)

    def test_wrong_baseline_hash_or_ambiguous_json_fails(self):
        with self.assertRaises(ValueError):
            self.evaluate(raw=self.raw + b' ')
        for raw in (b'{"journey":1,"journey":2}', b'{"journey":NaN}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                latency.decode_bundle(raw)
        with self.assertRaises(ValueError):
            latency.decode_bundle(b' ' * (1024 * 1024 + 1))

    def test_baseline_device_conditions_plan_procedure_and_freshness_are_independent(self):
        for name in ('conditions', 'plan', 'procedure_sha256', 'device', 'freshness'):
            baseline = copy.deepcopy(self.current)
            if name == 'conditions':
                baseline['conditions']['power'] = 'Different authored condition'
            elif name == 'plan':
                baseline['plan']['media_sha256'] = 'f' * 64
                baseline['journey']['plan_sha256'] = journey.plan_digest(baseline['plan'])
            elif name == 'procedure_sha256':
                baseline[name] = 'f' * 64
                baseline['journey'][name] = 'f' * 64
            elif name == 'device':
                baseline['install']['device'] = '000000000000002b'
                baseline['journey']['identity'] = copy.deepcopy(baseline['install'])
            else:
                baseline['install']['installed_at'] = 0
                baseline['journey']['identity'] = copy.deepcopy(baseline['install'])
                baseline['journey']['finished_at'] = 1201
            policy, raw = approved(self.current, baseline)
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.evaluate(policy=policy, raw=raw)

    def test_prior_image_requires_exact_approved_bundle_but_can_differ(self):
        baseline = copy.deepcopy(self.current)
        baseline['source_revision'] = 'a' * 40
        baseline['image_bytes'] -= 16
        baseline['expected'].update(build='a' * 12, binary_sha256='9' * 64, inputs_sha256='8' * 64)
        baseline['install'].update(**baseline['expected'], readback_sha256='9' * 64)
        baseline['journey']['identity'] = copy.deepcopy(baseline['install'])
        baseline['journey']['hello'] = baseline['journey']['hello'].replace('b' * 12, 'a' * 12)
        for record in baseline['journey']['protocol']['records']:
            if record['kind'] == 'HELLO' and record['direction'] == 'response':
                record['body'] = baseline['journey']['hello']
        policy, raw = approved(self.current, baseline)
        self.assertEqual(self.evaluate(policy=policy, raw=raw)['state'], 'pass')

    def test_baseline_age_is_checked_after_current_independent_success(self):
        current = copy.deepcopy(self.current)
        current['install']['installed_at'] = 89900
        current['journey']['identity'] = copy.deepcopy(current['install'])
        current['journey'].update(started_at=89901, finished_at=89902)
        latency.validate_bundle(current, 90000)
        policy, raw = approved(current, self.current)
        with self.assertRaisesRegex(ValueError, 'stale/future'):
            latency.evaluate_latency(current, policy, CONTRACT, 90000, raw)

    def test_historical_aggregate_contract_is_not_a_comparison_baseline(self):
        policy = copy.deepcopy(self.policy)
        policy['approved_baseline_sha256'] = hashlib.sha256(CONTRACT).hexdigest()
        with self.assertRaises(ValueError):
            self.evaluate(policy=policy, raw=CONTRACT)
        self.assertEqual(latency.evaluate_latency(self.current, self.policy, CONTRACT, 1200)['state'], 'blocked')

    def test_changed_actual_sample_pair_cannot_mix_workloads(self):
        baseline = copy.deepcopy(self.current)
        receipt = baseline['journey']
        def shifted(body):
            if body.startswith('DIAG '):
                value = next(int(token.split('=')[1]) for token in body.split() if token.startswith('actual_character='))
                return fixtures.changed_fields(body, {'actual_character': value + 10})
            return body
        for record in receipt['protocol']['records']:
            record['body'] = shifted(record['body'])
        for operation in receipt['operations']:
            operation['responses'] = [shifted(body) for body in operation['responses']]
        for stage in receipt['stages']:
            stage['diagnostics'] = shifted(stage['diagnostics'])
        receipt['sampling']['initial']['diagnostics'] = shifted(receipt['sampling']['initial']['diagnostics'])
        for sample in receipt['sampling']['observations']:
            sample['diagnostics'] = shifted(sample['diagnostics'])
        # Both complete correctness journeys are valid; only pair equality fails.
        latency.validate_bundle(baseline, 1200)
        policy, raw = approved(self.current, baseline)
        with self.assertRaisesRegex(ValueError, 'anchors differ'):
            self.evaluate(policy=policy, raw=raw)

    def test_failed_correctness_never_produces_latency_pass(self):
        for field, value in (('terminal', 'failed'), ('simulation', True)):
            candidate = copy.deepcopy(self.current)
            candidate['journey'][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.evaluate(candidate)
        candidate = copy.deepcopy(self.current)
        candidate['journey']['sampling']['observations'][0]['diagnostics'] = 'DIAG available=0'
        with self.assertRaises(ValueError):
            self.evaluate(candidate)

    def test_capture_fields_order_identity_and_wrapper_bounds_fail_closed(self):
        mutations = ('missing', 'order', 'id', 'stage', 'action', 'start', 'finish', 'null', 'boot', 'session', 'ack')
        for mutation in mutations:
            candidate = copy.deepcopy(self.current)
            receipt = candidate['journey']
            action = receipt['latency']['actions'][0]
            records = [r for r in receipt['protocol']['records'] if r['request_id'] == action['request_id']]
            if mutation == 'missing': receipt['latency']['actions'].pop()
            elif mutation == 'order': receipt['latency']['actions'].reverse()
            elif mutation == 'id': action['request_id'] += 1
            elif mutation == 'stage': action['stage'] = 'home_finish'
            elif mutation == 'action': action['action'] = 'back'
            elif mutation == 'start': action['started'] = records[0]['monotonic'] + .001
            elif mutation == 'finish': action['finished'] = records[-1]['monotonic'] - .001
            elif mutation == 'null': action['finished'] = None
            elif mutation == 'boot': records[2]['boot'] += 1
            elif mutation == 'session': records[2]['session'] += 1
            else: records[1]['kind'] = 'CONSUMED'
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.evaluate(candidate)

    def test_wrapper_excludes_followup_observations_and_prior_wire_work(self):
        for side in ('start', 'finish'):
            candidate = copy.deepcopy(self.current)
            action = candidate['journey']['latency']['actions'][0]
            records = candidate['journey']['protocol']['records']
            index = next(i for i, r in enumerate(records) if r['request_id'] == action['request_id'])
            if side == 'start':
                action['started'] = records[index - 1]['monotonic'] - .001
            else:
                action['finished'] = records[index + 4]['monotonic'] + .001
            with self.subTest(side=side), self.assertRaisesRegex(ValueError, 'overlaps'):
                self.evaluate(candidate)

    def test_literal_classification_no_visual_and_mixed_unknown(self):
        cases = [('8279_DIRECT_GRAY_DRF', 'gray_full_direct_driver_tag'),
                 ('8279_ISLAND_POF', 'gray_island_driver_tag'),
                 ('[EPD] displayBuffer mode=0 off=0', 'mono_full_api_mode'),
                 ('[EPD] displayBuffer mode=1 off=1', 'mono_half_api_mode'),
                 ('[EPD] displayBuffer mode=2 off=0', 'mono_fast_api_mode'),
                 ('prefix 8279_ISLAND_POF', 'unknown'), ('[EPD] displayBuffer mode=2 off=2', 'unknown')]
        for tag, expected in cases:
            with self.subTest(tag=tag):
                self.assertEqual(latency.classify_refresh([{'tag': tag}], 'RENDER_RETURN'), expected)
                self.assertEqual(latency.classify_refresh([{'tag': tag}], 'NO_VISUAL_CHANGE'), 'no_visual_change')
        self.assertEqual(latency.classify_refresh([], 'RENDER_RETURN'), 'unknown')
        self.assertEqual(latency.classify_refresh([{'tag': '8279_ISLAND_POF'}] * 2, 'RENDER_RETURN'), 'gray_island_driver_tag')
        self.assertEqual(latency.classify_refresh([{'tag': '8279_ISLAND_POF'}, {'tag': '8279_DIRECT_GRAY_DRF'}], 'RENDER_RETURN'), 'unknown')

    def test_unknown_missing_mixed_required_mode_cannot_admit(self):
        required_ids = {o['request_id'] - 2 for o in self.current['journey']['sampling']['observations']}
        for mutation in ('missing', 'mixed', 'unexpected'):
            candidate = copy.deepcopy(self.current)
            for action in candidate['journey']['latency']['actions']:
                if action['request_id'] in required_ids:
                    if mutation == 'missing': action['telemetry'] = []
                    elif mutation == 'mixed': action['telemetry'] += copy.deepcopy(action['telemetry']); action['telemetry'][1]['tag'] = '8279_DIRECT_GRAY_DRF'
                    else: action['telemetry'][0]['tag'] = 'unrecognized'
            result = self.evaluate(candidate)
            self.assertEqual(result['state'], 'blocked')
            self.assertTrue(any('classification' in reason for reason in result['reasons']))

    def test_driver_telemetry_outside_press_or_unbounded_fails(self):
        for mutation in ('before', 'after', 'order', 'bound'):
            candidate = copy.deepcopy(self.current)
            action = candidate['journey']['latency']['actions'][0]
            if mutation == 'before': action['telemetry'][0]['monotonic'] = action['started']
            elif mutation == 'after': action['telemetry'][0]['monotonic'] = action['finished']
            elif mutation == 'order':
                action['telemetry'] += copy.deepcopy(action['telemetry'])
                action['telemetry'][1]['monotonic'] -= .001
            else: action['telemetry'] *= 2049
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.evaluate(candidate)

    def test_complete_review_policy_and_minimum_for_every_observed_mode(self):
        for mutation in ('incomplete', 'duplicate', 'floor', 'noise', 'review', 'reason'):
            policy = copy.deepcopy(self.policy)
            if mutation == 'incomplete': policy['groups'].pop()
            elif mutation == 'duplicate': policy['groups'].append(copy.deepcopy(policy['groups'][0]))
            elif mutation == 'floor': policy['groups'][0]['min_samples'] = 19
            elif mutation == 'noise': policy['groups'][0]['noise_bound_ms'] = -1
            elif mutation == 'review': policy['review'] = None
            else: policy['review']['reason'] = ''
            if mutation in ('incomplete', 'review'):
                self.assertEqual(self.evaluate(policy=policy)['state'], 'blocked')
            else:
                with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                    self.evaluate(policy=policy)
        policy = copy.deepcopy(self.policy)
        policy['groups'][0]['min_samples'] = 21
        self.assertEqual(self.evaluate(policy=policy)['state'], 'blocked')

    def test_one_new_mode_sample_cannot_be_promoted_to_p95(self):
        candidate = copy.deepcopy(self.current)
        identifier = candidate['journey']['sampling']['observations'][0]['request_id'] - 2
        next(a for a in candidate['journey']['latency']['actions'] if a['request_id'] == identifier)['telemetry'][0]['tag'] = '8279_DIRECT_GRAY_DRF'
        policy, raw = approved(candidate)
        result = self.evaluate(candidate, policy, raw)
        self.assertEqual(result['state'], 'blocked')
        single = [g for g in result['groups'] if g['required'] and g['n'] == 1]
        self.assertEqual(len(single), 2)
        self.assertTrue(all(g['distribution'] == 'provisional' for g in single))
        self.assertTrue(any('Insufficient' in reason for reason in result['reasons']))


if __name__ == '__main__':
    unittest.main()
