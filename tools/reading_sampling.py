"""Optional bounded warm sampling and timing capture; journey semantics are explicit callbacks.

No producer/evaluator imports, independent connections, discovery, or retries.
"""
import re


def validate_cycles(value, require):
    require(type(value) is int and (value == 0 or 20 <= value <= 100), 'Latency cycles must be 0 or 20..100')
    return value


def audit_bound(plan, cycles, stages, stage_actions, validate_plan, require):
    """Reserve the fixed journey's worst-case wire slots before any connection.

    Library selection is observed on device. Bound its possible starting index
    at both ends, without guessing a selection or injecting positioning presses.
    """
    validate_plan(plan)
    validate_cycles(cycles, require)
    presses = sum(max(len(stage_actions(name, {stage: (None, {'library_selected': index})
                       for stage in stages}, plan)) for index in (0, plan['library_count'] - 1))
                  for name in stages)
    return 6 + 4 * len(stages) + 4 * presses + 16 * cycles


def check_sample(action, result, terminal, before_generation, seen, plan, install,
                 *, require, anchor, check_stage):
    """A known adjacent same-spine pair, with exact return after every cycle."""
    require(action in ('previous', 'next'), 'Unknown sampling action')
    s, d = result
    check_stage('primary_next', s, d, plan, install)
    expected = seen['primary_open' if action == 'previous' else 'primary_next'][1]
    require(anchor(d) == anchor(expected) and d['opening'] == 0 and
            s['ready'] == 1 and s['capture'] == 0, 'Warm navigation changed its known anchor/readiness')
    require(type(terminal) is str and re.fullmatch('RENDER_RETURN gen=(0|[1-9][0-9]*)', terminal),
            'Warm navigation did not render; NO_VISUAL_CHANGE cannot count as a sample')
    generation = int(terminal.split('=')[1])
    require(s['gen'] >= generation > before_generation, 'Warm navigation did not publish a fresh generation')


def validate(sampling, seen, stages, operations, plan, install,
             *, require, exact, observation, anchor, check_stage):
    exact(sampling, ('schema', 'stage', 'cycles', 'initial', 'observations'))
    require(type(sampling['schema']) is int and sampling['schema'] == 1 and sampling['stage'] == 'primary_next',
            'Unknown sampling schema/stage')
    cycles = validate_cycles(sampling['cycles'], require)
    require(cycles > 0, 'Empty sampling declaration')
    initial = sampling['initial']
    exact(initial, ('status', 'diagnostics', 'request_id'))
    stage = next(stage for stage in stages if stage['name'] == 'primary_next')
    require(initial == {name: stage[name] for name in initial}, 'Sampling origin differs from original primary_next')
    require(seen['primary_open'][1]['actual_spine'] == seen['primary_next'][1]['actual_spine'],
            'Warm sampling needs an already observed same-spine pair')
    observations = sampling['observations']
    require(type(observations) is list and len(observations) == 2 * cycles, 'Missing bounded sampling observations')
    index = next(i for i, operation in enumerate(operations) if operation['request_id'] == initial['request_id']) + 1
    generation = seen['primary_next'][0]['gen']
    for offset, sample in enumerate(observations):
        exact(sample, ('action', 'status', 'diagnostics', 'request_id'))
        action = ('previous', 'next')[offset % 2]
        triple = operations[index:index + 3]
        require(len(triple) == 3 and sample['action'] == action and
                [operation['command'] for operation in triple] == ['PRESS ' + action, 'STATUS', 'DIAG'] and
                triple[-1]['request_id'] == sample['request_id'] and
                triple[1]['responses'] == [sample['status']] and
                triple[2]['responses'] == [sample['diagnostics']], 'Sampling observation differs from exact wire operations')
        result = observation(sample['status'], sample['diagnostics'])
        check_sample(action, result, triple[0]['responses'][-1], generation, seen, plan, install,
                     require=require, anchor=anchor, check_stage=check_stage)
        generation = result[0]['gen']
        index += 3
    # The following original chapter turn must remain newer than the sampled
    # return, not merely newer than the retained original primary_next stage.
    terminal = operations[index]['responses'][-1]
    require(re.fullmatch('RENDER_RETURN gen=(0|[1-9][0-9]*)', terminal) and
            seen['primary_next2'][0]['gen'] >= int(terminal.split('=')[1]) > generation,
            'Chapter publication regressed after warm sampling')


def run(report, context, seen, plan, install, press, request, request_id,
        *, require, exact, observation, anchor, check_stage):
    context.update(phase='sampling-precondition', stage='primary_next', command=None)
    require(seen['primary_open'][1]['actual_spine'] == seen['primary_next'][1]['actual_spine'],
            'Warm sampling needs an already observed same-spine pair')
    report['sampling']['initial'] = {key: report['stages'][-1][key]
                                    for key in ('status', 'diagnostics', 'request_id')}
    generation = seen['primary_next'][0]['gen']
    for action in ('previous', 'next') * report['sampling']['cycles']:
        press(action)
        terminal = report['operations'][-1]['responses'][-1]
        status, diagnostics = request('STATUS')[0], request('DIAG')[0]
        sample = {'action': action, 'status': status, 'diagnostics': diagnostics,
                  'request_id': request_id()}
        report['sampling']['observations'].append(sample)
        context.update(phase='sampling-observation', command=None)
        result = observation(status, diagnostics)
        check_sample(action, result, terminal, generation, seen, plan, install,
                     require=require, anchor=anchor, check_stage=check_stage)
        generation = result[0]['gen']


def request_with_latency(client, command, stage, actions, deadline, monotonic, require):
    action = None
    if command.startswith('PRESS '):
        action = {'request_id': client.request_id + 1, 'stage': stage,
                  'action': command.split()[1], 'started': None,
                  'finished': None, 'telemetry': []}
        actions.append(action)
        action['started'] = monotonic()
    try:
        replies = client.request(command, capture_deadline=deadline)
        # ProtocolClient returns only after validating all three ordered
        # replies. This wrapper endpoint excludes following STATUS/DIAG.
        if action is not None:
            require(type(replies) is list and len(replies) == 3 and
                    replies[:2] == ['ACCEPTED', 'CONSUMED'] and
                    type(replies[2]) is str and
                    re.fullmatch('(RENDER_RETURN|NO_VISUAL_CHANGE) gen=(0|[1-9][0-9]*)', replies[2]),
                    'Wrong ordered terminal PRESS replies')
            action['finished'] = monotonic()
    finally:
        if action is not None:
            action['telemetry'] = [{'tag': record['tag'], 'monotonic': record['monotonic']}
                for record in getattr(client, 'press_telemetry', ())
                if (record['boot'], record['session'], record['request_id']) ==
                   (client.receipt()['protocol']['boot'], client.receipt()['protocol']['session'], action['request_id'])]
    return replies
