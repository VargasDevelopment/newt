"""Independent software/serial timing analysis of a validated reading journey.

The caller supplies authoritative current native build bindings. A recorded
bundle cannot establish its own current build authority. Baseline approval pins
raw bundle bytes separately from the execution procedure, avoiding an approval
cycle. These editable reviewed pins are policy, not cryptographic authorization.
"""
import hashlib
import json
import math
import re

from code_health_resources import APP_PARTITION_CAPACITY
from reading_acceptance import (digest, exact, integer, number, require,
                                validate_journey, observation, anchor, fields, STATUS_KEYS)

CONTRACT_SHA256 = 'c4024b4dd02a557958158285a4502f9530bd10c95d8b9c1bbcddbf5aafaee416'
ENDPOINTS = ('wrapper_completion_ms', 'protocol_request_to_terminal_ms')
REFRESH_CLASSES = ('gray_full_direct_driver_tag', 'gray_island_driver_tag',
                   'mono_fast_api_mode', 'mono_full_api_mode', 'mono_half_api_mode')
BUNDLE_FIELDS = ('journey', 'install', 'plan', 'expected', 'target',
                 'procedure_sha256', 'source_revision', 'image_bytes', 'mode', 'conditions')


def decode_bundle(data):
    """Parse the approved bounded raw baseline without ambiguous JSON values."""
    require(type(data) is bytes and len(data) <= 1024 * 1024, 'Invalid bounded latency baseline')
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'Duplicate latency baseline field')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('Nonfinite latency baseline')))


def text(value):
    require(type(value) is str and 0 < len(value.strip()) <= 256, 'Missing explicit latency policy/condition declaration')
    return value


def classify_refresh(telemetry, terminal):
    """Driver/API labels only. Neither a waveform nor an optical measurement."""
    if terminal == 'NO_VISUAL_CHANGE':
        return 'no_visual_change'
    classes = set()
    for item in telemetry:
        tag = item['tag']
        if tag == '8279_DIRECT_GRAY_DRF':
            classes.add('gray_full_direct_driver_tag')
        elif tag == '8279_ISLAND_POF':
            classes.add('gray_island_driver_tag')
        else:
            match = re.fullmatch(r'\[EPD\] displayBuffer mode=([012]) off=[01]', tag)
            classes.add({'0': 'mono_full_api_mode', '1': 'mono_half_api_mode',
                         '2': 'mono_fast_api_mode'}[match[1]] if match else 'unknown')
    return next(iter(classes)) if len(classes) == 1 else 'unknown'


def statistics(samples):
    require(type(samples) is list and len(samples) > 0, 'Missing individual latency samples')
    values = sorted(number(value) for value in samples)
    return {'n': len(values), 'samples': list(samples),
            'p50': values[math.ceil(.50 * len(values)) - 1],
            'p95': values[math.ceil(.95 * len(values)) - 1], 'max': values[-1]}


def group_key(group):
    return tuple(group[name] for name in ('action_class', 'refresh_class', 'endpoint_name'))


def validate_policy(policy):
    exact(policy, ('schema', 'contract_sha256', 'approved_baseline_sha256',
                   'review', 'groups', 'goals_ms', 'comparison_statistics'))
    require(type(policy['schema']) is int and policy['schema'] == 1, 'Unsupported latency policy')
    require(policy['contract_sha256'] == CONTRACT_SHA256, 'Unreviewed latency contract')
    if policy['approved_baseline_sha256'] is not None:
        digest(policy['approved_baseline_sha256'])
    if policy['review'] is not None:
        exact(policy['review'], ('reviewer', 'reason'))
        text(policy['review']['reviewer']); text(policy['review']['reason'])
    require(policy['goals_ms'] == [500, 750, 1000] and
            all(type(value) is int for value in policy['goals_ms']), 'Changed frozen latency goals')
    require(policy['comparison_statistics'] == ['p50', 'p95', 'max'], 'Incomplete latency regression statistics')
    require(type(policy['groups']) is list and len(policy['groups']) <= 20, 'Invalid latency policy groups')
    keys = []
    for group in policy['groups']:
        exact(group, ('action_class', 'refresh_class', 'endpoint_name', 'min_samples', 'noise_bound_ms'))
        require(group['action_class'] in ('ordinary_navigation/previous', 'ordinary_navigation/next') and
                group['refresh_class'] in REFRESH_CLASSES and group['endpoint_name'] in ENDPOINTS,
                'Unreviewed latency sample class/endpoint')
        integer(group['min_samples'], 20, 200)
        number(group['noise_bound_ms'])
        keys.append(group_key(group))
    require(len(keys) == len(set(keys)), 'Duplicate latency policy group')


def validate_bundle(bundle, now):
    exact(bundle, BUNDLE_FIELDS)
    exact(bundle['expected'], ('build', 'binary_sha256', 'inputs_sha256'))
    require(type(bundle['source_revision']) is str and
            re.fullmatch('[0-9a-f]{40}', bundle['source_revision']), 'Missing full latency source revision')
    require(type(bundle['expected']['build']) is str and
            bundle['expected']['build'].startswith(bundle['source_revision'][:12]),
            'Latency source/build mismatch')
    digest(bundle['procedure_sha256'])
    integer(bundle['image_bytes'], 1, APP_PARTITION_CAPACITY)
    require(bundle['mode'] == 'physical-usb-serial', 'Unsupported latency endpoint mode')
    conditions = bundle['conditions']
    require(type(conditions) is dict and 1 <= len(conditions) <= 16, 'Missing owner-observed latency conditions')
    for key, value in conditions.items():
        text(key); text(value)
    validate_journey(bundle['journey'], bundle['install'], bundle['plan'], bundle['expected'],
                     bundle['target'], bundle['procedure_sha256'], now)


def derive_samples(receipt):
    """Match every wrapper to the validated wire audit, with no caller success flag."""
    latency = receipt.get('latency')
    exact(latency, ('schema', 'actions'))
    require(type(latency['schema']) is int and latency['schema'] == 1, 'Unsupported latency capture')
    actions = latency['actions']
    press = [r for r in receipt['protocol']['records'] if r['direction'] == 'request' and r['kind'] == 'PRESS']
    require(type(actions) is list and len(actions) == len(press), 'Latency capture does not cover every PRESS')
    sampled = {}
    if 'sampling' in receipt:
        sampling = receipt['sampling']  # validate_journey already checks exact expanded operations/anchors.
        for observation in sampling['observations']:
            identifier = observation['request_id'] - 2
            require(identifier not in sampled, 'Duplicate sampled PRESS')
            sampled[identifier] = observation['action']
    stages = receipt['stages']
    previous_id = 0
    original_stage = {}
    for stage in stages:
        for request in press:
            if previous_id < request['request_id'] < stage['request_id'] and request['request_id'] not in sampled:
                original_stage[request['request_id']] = stage['name']
        previous_id = stage['request_id']
    records = receipt['protocol']['records']
    result, telemetry_count = [], 0
    for action, request in zip(actions, press):
        exact(action, ('request_id', 'stage', 'action', 'started', 'finished', 'telemetry'))
        identifier = integer(action['request_id'], 0x80000000)
        require(type(action['action']) is str and action['action'] in ('previous', 'next', 'activate', 'back'),
                'Invalid latency action')
        require(identifier == request['request_id'] and request['body'] == 'PRESS ' + action['action'],
                'Latency action differs from matching wire PRESS')
        expected_stage = 'primary_next' if identifier in sampled else original_stage.get(identifier)
        require(action['stage'] == expected_stage and expected_stage is not None, 'Latency action stage mismatch')
        if identifier in sampled:
            require(action['action'] == sampled[identifier], 'Sampled action mismatch')
        matches = [r for r in records if r['request_id'] == identifier]
        require(len(matches) == 4 and matches[0] == request and
                [r['kind'] for r in matches[1:3]] == ['ACCEPTED', 'CONSUMED'] and
                matches[3]['kind'] in ('RENDER_RETURN', 'NO_VISUAL_CHANGE'), 'Incomplete latency acknowledgement order')
        require(all((r['boot'], r['session']) == (receipt['protocol']['boot'], receipt['protocol']['session'])
                    for r in matches), 'Latency acknowledgement identity mismatch')
        start, finish = number(action['started']), number(action['finished'])
        times = [number(record['monotonic']) for record in matches]
        require(receipt['started_monotonic'] <= start <= times[0] <= times[1] <= times[2] <= times[3] <= finish <= receipt['finished_monotonic'],
                'Latency wrapper/ACK/terminal time bounds violated')
        index = records.index(request)
        require(index > 0 and records[index - 1]['monotonic'] <= start and
                index + 4 < len(records) and finish <= records[index + 4]['monotonic'],
                'Latency wrapper overlaps another protocol operation')
        following = records[index + 4:index + 6]
        refresh_ms = fields(following[1]['body'], 'STATUS', STATUS_KEYS)['refresh_ms'] if (
            len(following) == 2 and following[0]['kind'] == 'STATUS') else None
        telemetry = action['telemetry']
        require(type(telemetry) is list, 'Missing bounded latency telemetry')
        telemetry_count += len(telemetry)
        require(telemetry_count <= 2048, 'Latency telemetry exceeds capture bound')
        prior = times[0]
        for item in telemetry:
            exact(item, ('tag', 'monotonic'))
            require(type(item['tag']) is str and 0 < len(item['tag']) <= 96, 'Invalid latency driver tag')
            tag_time = number(item['monotonic'])
            require(prior <= tag_time <= times[3], 'Driver tag outside matching PRESS window')
            prior = tag_time
        terminal = matches[3]['kind']
        result.append({'request_id': identifier, 'stage': action['stage'], 'action': action['action'],
                       'required': identifier in sampled,
                       'action_class': 'ordinary_navigation/' + action['action'] if identifier in sampled else
                                       action['stage'] + '/' + action['action'],
                       'refresh_class': classify_refresh(telemetry, terminal), 'terminal': terminal,
                       'last_completed_refresh_ms': refresh_ms,
                       'wrapper_completion_ms': 1000 * (finish - start),
                       'protocol_request_to_terminal_ms': 1000 * (times[3] - times[0]),
                       'acknowledgement_ms': {'ACCEPTED': 1000 * (times[1] - times[0]),
                                              'CONSUMED': 1000 * (times[2] - times[0]),
                                              'terminal': 1000 * (times[3] - times[0])}})
    return result


def summarize(samples, goals):
    groups = {}
    for sample in samples:
        for endpoint in ENDPOINTS:
            key = sample['action_class'], sample['refresh_class'], endpoint
            if key not in groups:
                groups[key] = {'action_class': key[0], 'refresh_class': key[1],
                               'endpoint_name': endpoint, 'required': sample['required'], 'values': []}
            groups[key]['values'].append(sample[endpoint])
    result = []
    for key in sorted(groups):
        group = groups[key]
        stats = statistics(group.pop('values'))
        group.update(stats)
        group['distribution'] = 'empirical' if group['required'] and stats['n'] >= 20 else 'provisional'
        group['goals'] = [{'statistic': statistic, 'measured_ms': stats[statistic],
                           'threshold_ms': goal,
                           'within_goal': stats[statistic] <= goal if group['distribution'] == 'empirical' else None,
                           'distribution_sufficient': group['distribution'] == 'empirical'}
                          for statistic, goal in zip(('p50', 'p95', 'max'), goals)]
        result.append(group)
    return result


def evaluate_latency(current, policy, contract_bytes, now, baseline_bytes=None):
    """Measure first; admit only an independently valid, pinned, reviewed comparison.

    Goals are descriptive and never replace baseline regression checks. A
    minimum of 20 supports an empirical nearest-rank p95 engineering policy;
    it is not a confidence bound or a claim about an unseen distribution.
    """
    validate_policy(policy)
    require(type(contract_bytes) is bytes and hashlib.sha256(contract_bytes).hexdigest() == CONTRACT_SHA256,
            'Frozen latency contract bytes changed')
    validate_bundle(current, now)
    samples = derive_samples(current['journey'])
    groups = summarize(samples, policy['goals_ms'])
    reasons = []
    required = {group_key(g): g for g in groups if g['required']}
    if not required:
        reasons.append('No validated ordinary-navigation sampling run')
    if any(key[1] not in REFRESH_CLASSES for key in required):
        reasons.append('Required refresh classification is unknown/mixed or no-visual-change')
    rules = {group_key(g): g for g in policy['groups']}
    if set(required) != set(rules):
        reasons.append('Reviewed policy does not cover exactly all required groups')
    if policy['review'] is None:
        reasons.append('Sampling and noise policy has no explicit review')
    if policy['approved_baseline_sha256'] is None:
        reasons.append('No approved raw baseline SHA-256')
    baseline_groups = None
    if baseline_bytes is not None:
        require(type(baseline_bytes) is bytes and len(baseline_bytes) <= 1024 * 1024,
                'Invalid bounded latency baseline')
        require(policy['approved_baseline_sha256'] is not None and
                hashlib.sha256(baseline_bytes).hexdigest() == policy['approved_baseline_sha256'],
                'Unapproved latency baseline bytes')
        baseline = decode_bundle(baseline_bytes)
        validate_bundle(baseline, now)
        require(all(current[name] == baseline[name] for name in ('target', 'plan', 'procedure_sha256', 'mode', 'conditions')) and
                current['install']['device'] == baseline['install']['device'],
                'Latency baseline device/media/plan/procedure/mode/conditions mismatch')
        def sampled_pair(receipt):
            stages = {stage['name']: stage for stage in receipt['stages']}
            pair = []
            for stage in (stages['primary_open'], stages['primary_next']):
                _, diagnostics = observation(stage['status'], stage['diagnostics'])
                pair.append((diagnostics['current_book'], *anchor(diagnostics)))
            return pair
        require(sampled_pair(current['journey']) == sampled_pair(baseline['journey']),
                'Latency baseline/current sampled book/spine/character anchors differ')
        baseline_groups = summarize(derive_samples(baseline['journey']), policy['goals_ms'])
    elif policy['approved_baseline_sha256'] is not None:
        reasons.append('Approved baseline evidence was not supplied')
    comparisons = []
    if baseline_groups is not None:
        before = {group_key(g): g for g in baseline_groups if g['required']}
        if set(before) != set(required):
            reasons.append('Baseline/current required refresh/action/endpoint groups differ')
        for key in sorted(set(required) & set(before) & set(rules)):
            rule, candidate, original = rules[key], required[key], before[key]
            enough = min(candidate['n'], original['n']) >= rule['min_samples']
            if not enough:
                reasons.append('Insufficient individual samples for ' + '/'.join(key))
            for statistic in policy['comparison_statistics']:
                limit = original[statistic] + rule['noise_bound_ms']
                comparisons.append({'action_class': key[0], 'refresh_class': key[1], 'endpoint_name': key[2],
                                    'statistic': statistic, 'current': candidate[statistic],
                                    'baseline': original[statistic], 'noise_bound_ms': rule['noise_bound_ms'],
                                    'limit': limit, 'sufficient': enough, 'within_limit': candidate[statistic] <= limit})
    regressions = any(c['sufficient'] and not c['within_limit'] for c in comparisons)
    state = 'blocked' if reasons else 'regression' if regressions else 'pass'
    return {'schema': 1, 'state': state, 'reasons': reasons, 'groups': groups, 'actions': samples,
            'comparisons': comparisons, 'baseline_groups': baseline_groups,
            'policy_sha256': hashlib.sha256(json.dumps(policy, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
            'current_bundle_sha256': hashlib.sha256(json.dumps(current, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
            'contract_sha256': CONTRACT_SHA256, 'approved_baseline_sha256': policy['approved_baseline_sha256'],
            'software_serial_measurement': True, 'physical_optical_measurement': False,
            'physical_optical': 'unmeasured',
            'minimum_sample_policy': 'empirical nearest-rank engineering rule; no confidence claim'}
