"""One bounded installed-reader journey. Stored receipts are evidence, not authority."""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import time

import reading_sampling as sampling

from code_health_resources import RAM_CAPACITY, authored_procedure_files, tree_hash

MAX_AGE = 24 * 60 * 60
MAX_RUN = 600
MAX_AUDIT_RECORDS = 2048
CAPACITY = RAM_CAPACITY  # Pinned C3 linker capacity, not a regression ceiling.
STAGES = (
    'home', 'library', 'primary_open', 'primary_next', 'primary_next2',
    'primary_previous', 'primary_exit', 'primary_reopen', 'primary_reexit',
    'sibling_library', 'sibling_open', 'sibling_next', 'sibling_exit',
    'rejected_library', 'rejected_open', 'recovered_library', 'recovered_open',
    'recovered_next', 'recovered_boundary_exit', 'recovered_boundary_reopen',
    'recovered_boundary_previous', 'recovered_boundary_next', 'recovered_exit', 'sibling_recovery_library',
    'sibling_recovery_open', 'sibling_recovery_next', 'home_finish',
)
DIAG_KEYS = {'available', 'reset', 'cause', 'record', 'requested', 'restored',
             'checkpoint_saved', 'book', 'spine', 'character', 'reader_status',
             'reader_open', 'progress_ready', 'actual_spine', 'actual_character',
             'first_input', 'version', 'opening', 'heap', 'device', 'current_book', 'largest', 'library_count', 'library_selected'}
STATUS_KEYS = {'screen', 'gen', 'ready', 'refresh_ms', 'heap', 'dropped', 'capture'}


def require(okay, why):
    if not okay:
        raise ValueError(why)


def exact(value, keys):
    require(type(value) is dict and set(value) == set(keys), 'Missing or unexpected reading evidence fields')


def integer(value, low=0, high=0xffffffff):
    require(type(value) is int and low <= value <= high, 'Invalid bounded reading measurement')
    return value


def digest(value):
    require(type(value) is str and re.fullmatch('[0-9a-f]{64}', value), 'Invalid SHA-256 identity')
    return value


def number(value):
    require(type(value) in (int, float) and math.isfinite(value) and value >= 0, 'Invalid evidence time')
    return value


def load(path):
    """Bounded JSON; ambiguous duplicate keys and nonfinite values never pass."""
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'Duplicate reading evidence field')
            result[key] = value
        return result
    with Path(path).open('rb') as stream:
        data = stream.read(1024 * 1024 + 1)
    require(len(data) <= 1024 * 1024, 'Reading evidence exceeds limit')
    return json.loads(data, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('Nonfinite evidence')))


def procedure_identity(root):
    # Each named Python file's imports are named too (tests/procedure_closure_test.py).
    paths = [Path(root) / name for name in (
        'tools/reading_acceptance.py', 'tools/reading_sampling.py', 'tools/reading-ready.py', 'tools/dev-usb.py',
        'tools/bench_lease.py', 'tools/bench_candidate.py', 'tools/x3_slots.py', 'tools/dev_usb_panel_telemetry.py',
        'tools/dev_usb_firmware_rows.py', 'tools/dev_usb_put.py', 'tools/dev_usb_wake.py', 'tools/fatfs_case.py', 'tools/reading_latency.py', 'tests/reading_latency_test.py',
        'tests/dev_usb_serial_test.py', 'tests/dev_usb_telemetry_test.py',
        'tools/release_tools.py', 'tools/code_health_flash_policy.py', 'tools/code_health.py',
        'tests/reading_latency_ready_test.py', 'docs/reading-latency-contract.json',
        'tools/check.py', 'tools/check_scratch.py', 'tools/gate_scratch.py', 'tools/firmware_host_build.py',
        'tools/gate_cpu.py', 'tools/gate_child.py', 'tools/gate-cpu-budgets.json',
        'tools/gate_plan.py', 'tools/gate_runner.py', 'tools/gate_previews.py', 'tools/gate_tier.py', 'tools/rust_gate_families.py',
        'tools/gate-time-budgets.json', 'tools/release_tag.py', 'tools/host_compile.py', 'tests/reading_acceptance_test.py',
        'tools/system_ui_contract.py', 'tests/system_ui_contract_test.py', 'tools/wifi_link_image.py', 'tools/rust_open_stack.py',
        'tools/code_health_resources.py', 'tools/code_health_sram.py', 'tools/freeink_sdk_routes.py',
        'tools/freeink_sdk_provenance.py', 'tools/freeink_source_pin.py', 'tools/apply-freeink-patch.py',
        'tools/python_runtime.py', 'tools/host_common.py', 'tools/host_objects.py', 'tools/host_jobs.py', 'tools/native_output.py',
        'tests/host_objects_test.py', 'tests/host_object_reuse_test.py',
        'tools/trusted_host.py', 'tools/firmware-build.py',
        'tools/native_toolchain.py', 'tools/toolchain-share.py', 'tools/daily_firmware.py', 'tools/rust_resource_inputs.py',
        'tools/rust_app_archive.py',
        'tools/software_gate_shadow.py', 'tests/software_gate_shadow_test.py', 'tools/rust-toolchain.py', 'tools/linked_code.py',
        'tools/library-redesign-preview.py', 'tools/rust_app_link.py', 'tools/rust_image_measurements.py',
        'tools/prepare-library-covers.py', 'tools/rust_preview.py', 'rust/tests/inflate/cargo_context.py')]
    return tree_hash(Path(root), authored_procedure_files(Path(root), paths))


def fields(body, kind, keys):
    require(type(body) is str and body.startswith(kind + ' '), 'Wrong reading response kind')
    result = {}
    for token in body.split()[1:]:
        name, separator, value = token.partition('=')
        require(separator and name not in result and (re.fullmatch('[0-9a-f]{16}', value) if name == 'device' else re.fullmatch('0|[1-9][0-9]*', value)),
                'Malformed reading response')
        result[name] = value if name == 'device' else int(value)
    exact(result, keys)
    for name, value in result.items():
        if name != 'device':
            integer(value, high=0xffffffffffffffff if name in {'book', 'current_book'} else 0xffffffff)
    return result


def observation(status, diagnostics):
    s = fields(status, 'STATUS', STATUS_KEYS)
    d = fields(diagnostics, 'DIAG', DIAG_KEYS)
    for name in ('available', 'restored', 'checkpoint_saved', 'reader_open', 'progress_ready', 'opening'):
        integer(d[name], high=1)
    require(d['version'] == 2 and d['opening'] == 0 and d['available'] == 1 and s['ready'] == 1 and s['capture'] == 0 and s['dropped'] == 0,
            'Reader diagnostic unavailable or lifecycle not ready')
    require(0 < d['largest'] <= d['heap'] <= CAPACITY and 0 < s['heap'] <= CAPACITY, 'Missing or impossible measured heap/largest block')
    integer(s['gen'], 1)
    integer(d['library_count'], high=128)
    integer(d['library_selected'], high=127)
    return s, d


def validate_plan(plan):
    exact(plan, ('media_sha256', 'library_count', 'primary', 'sibling', 'rejected'))
    digest(plan['media_sha256'])
    count = integer(plan['library_count'], 3, 128)
    for role in ('primary', 'sibling', 'rejected'):
        exact(plan[role], ('index', 'book'))
        integer(plan[role]['index'], high=count - 1)
        integer(plan[role]['book'], 1, 0xffffffffffffffff)
    require(len({plan[r]['book'] for r in ('primary', 'sibling', 'rejected')}) == 3 and
            len({plan[r]['index'] for r in ('primary', 'sibling', 'rejected')}) == 3,
            'Journey needs two distinct admitted books and one intentional rejected fixture')


def validate_install(install, expected, target, now):
    exact(install, ('schema', 'target', 'device', 'build', 'binary_sha256', 'readback_sha256',
                    'inputs_sha256', 'installed_at'))
    require(type(install['schema']) is int and install['schema'] == 1, 'Unsupported installation evidence')
    require(type(target) is str and re.fullmatch('[a-z][a-z0-9-]{0,31}', target) and
            install['target'] == target, 'Wrong device target')
    require(type(install['device']) is str and re.fullmatch('[0-9a-f]{16}', install['device']), 'Invalid private device fingerprint')
    for name in ('build', 'binary_sha256', 'inputs_sha256'):
        require(install[name] == expected[name], 'Wrong installed build/source identity')
    digest(install['binary_sha256']); digest(install['inputs_sha256'])
    require(install['readback_sha256'] == install['binary_sha256'], 'Installation has no exact image readback')
    require(0 <= number(now) - number(install['installed_at']) <= MAX_AGE, 'Installation evidence is stale/future')


def anchor(d):
    return d['actual_spine'], d['actual_character']


def validate_journey(receipt, install, plan, expected, target, procedure, now):
    """Fail closed on partial/old/fixture runs. No copy-integrity shortcut exists."""
    validate_plan(plan)
    validate_install(install, expected, target, now)
    keys = ('schema', 'identity', 'procedure_sha256', 'plan_sha256', 'started_at',
                    'finished_at', 'started_monotonic', 'finished_monotonic', 'terminal', 'simulation', 'hello', 'connection', 'protocol', 'operations', 'stages')
    # Latency is an optional independent evidence channel. It cannot replace
    # these unchanged correctness checks; the latency gate validates it itself.
    optional = tuple(name for name in ('latency', 'sampling') if type(receipt) is dict and name in receipt)
    exact(receipt, (*keys, *optional))
    cycles = 0
    if 'sampling' in receipt:
        exact(receipt['sampling'], ('schema', 'stage', 'cycles', 'initial', 'observations'))
        cycles = sampling.validate_cycles(receipt['sampling']['cycles'], require)
        require(cycles > 0, 'Empty sampling declaration')
        require(journey_audit_bound(plan, cycles) <= MAX_AUDIT_RECORDS, 'Sampling exceeds bounded protocol audit')
    require(type(receipt['schema']) is int and receipt['schema'] == 1 and receipt['terminal'] == 'complete'
            and receipt['simulation'] is False, 'No completed physical reading journey')
    require(receipt['identity'] == install and receipt['procedure_sha256'] == procedure and
            receipt['plan_sha256'] == plan_digest(plan), 'Stale journey build/procedure/media/fixture identity')
    start, finish = number(receipt['started_at']), number(receipt['finished_at'])
    mono_start, mono_finish = number(receipt['started_monotonic']), number(receipt['finished_monotonic'])
    require(0 <= mono_finish - mono_start <= MAX_RUN, 'Unbounded monotonic journey duration')
    require(install['installed_at'] <= start <= finish <= now and finish - start <= MAX_RUN and
            now - finish <= MAX_AGE, 'Journey predates installation or is stale/unbounded')
    require(receipt['hello'] in (f"HELLO build={install['build']} caps=press,status,frame,diag2{put} wake=physical-only"
                                 for put in ('', ',put')), 'Wrong connected firmware build')
    connection = receipt['connection']
    require(type(connection) is dict and connection.get('transport') == 'serial' and
            connection.get('closed') is True, 'Journey did not close its physical connection')
    events = connection.get('events')
    require(type(events) is list and all(type(e) is dict for e in events) and [e.get('event') for e in events] ==
            ['opening', 'opened', 'closing', 'closed'], 'Reopened/reset/failed USB connection')
    protocol = receipt['protocol']
    exact(protocol, ('boot', 'session', 'last_request_id', 'records'))
    boot, session = integer(protocol['boot'], 1), integer(protocol['session'], 2)
    records = protocol['records']
    require(type(records) is list and 1 <= len(records) <= MAX_AUDIT_RECORDS, 'Missing bounded protocol audit')
    require(all(type(r) is dict and type(r.get('body')) is str and len(r['body']) <= 464
                for r in records), 'Missing exact bounded wire audit bodies')
    times = [number(r.get('monotonic')) for r in records]
    require(mono_start <= times[0] <= times[-1] <= mono_finish and
            all(a <= b for a, b in zip(times, times[1:])), 'Missing/reordered/unbounded monotonic audit')
    require(times[1] - times[0] <= 30 and times[3] - times[2] <= 30, 'Handshake deadline exceeded')
    require([(r.get('direction'), r.get('kind'), r.get('request_id')) for r in records[:4]] ==
            [('request', 'HELLO', 0), ('response', 'HELLO', 0)] * 2,
            'Missing fresh two-handshake connection proof')
    require(records[0]['body'] == records[2]['body'] == 'HELLO' and
            records[1]['body'] == records[3]['body'] == receipt['hello'], 'Wrong live HELLO build/capabilities')
    require(records[1].get('boot') == records[3].get('boot') == boot and
            records[1].get('session') + 1 == records[3].get('session') == session,
            'Device reset during handshake')
    requests, audited_replies, cursor = [], {}, 4
    while cursor < len(records):
        request = records[cursor]
        identifier = integer(request.get('request_id'), 0x80000000)
        require(request.get('direction') == 'request' and request.get('kind') in {'PRESS', 'STATUS', 'DIAG'},
                'Unknown or orphan protocol operation')
        responses = records[cursor + 1:cursor + (4 if request['kind'] == 'PRESS' else 2)]
        require(len(responses) == (3 if request['kind'] == 'PRESS' else 1), 'Nonterminal protocol operation')
        for record in [request, *responses]:
            require(record.get('boot') == boot and record.get('session') == session and
                    record.get('request_id') == identifier, 'Device reset/session/request changed during journey')
        require(responses[-1]['monotonic'] - request['monotonic'] <= 30, 'Request terminal deadline exceeded')
        kinds = [r.get('kind') for r in responses]
        require(all(r.get('direction') == 'response' for r in responses), 'Missing protocol response')
        require(kinds[:2] == ['ACCEPTED', 'CONSUMED'] and kinds[2] in ('RENDER_RETURN', 'NO_VISUAL_CHANGE')
                if request['kind'] == 'PRESS' else kinds == [request['kind']],
                'Press completion unknown or wrong response; no readiness')
        requests.append(request)
        audited_replies[identifier] = [r['body'] for r in responses]
        cursor += 1 + len(responses)
    ids = [r['request_id'] for r in requests]
    require(ids and all(b == a + 1 for a, b in zip(ids, ids[1:])) and
            ids[-1] == protocol['last_request_id'], 'Missing/replayed/nonterminal protocol requests')
    operations = receipt['operations']
    require(type(operations) is list and len(operations) == len(requests) - 1,
            'Journey operations do not cover the actual protocol requests')
    require(requests[0]['kind'] == 'STATUS', 'Missing post-handshake identity probe')
    for operation, request in zip(operations, requests[1:]):
        exact(operation, ('request_id', 'command', 'responses'))
        command, replies = operation['command'], operation['responses']
        require(type(command) is str and (command in ('STATUS', 'DIAG') or
                re.fullmatch('PRESS (previous|next|activate|back)', command)), 'Unknown journey command')
        require(operation['request_id'] == request['request_id'] and command.split()[0] == request['kind'] and
                command == request['body'] and replies == audited_replies[request['request_id']],
                'Journey operation does not match live request')
        require(type(replies) is list and all(type(body) is str for body in replies), 'Malformed operation responses')
        if command.startswith('PRESS '):
            require(len(replies) == 3 and replies[:2] == ['ACCEPTED', 'CONSUMED'] and
                    re.fullmatch('(RENDER_RETURN|NO_VISUAL_CHANGE) gen=(0|[1-9][0-9]*)', replies[2]),
                    'Missing terminal press completion body')
        else:
            require(len(replies) == 1 and replies[0].startswith(command + ' '), 'Wrong observation response body')
    stages = receipt['stages']
    require(type(stages) is list and all(type(s) is dict for s in stages) and [s.get('name') for s in stages] == list(STAGES),
            'Missing/skipped/unknown journey stages')
    seen = {}
    last_id, last_operation = 0, 0
    for stage in stages:
        exact(stage, ('name', 'status', 'diagnostics', 'request_id'))
        identifier = integer(stage['request_id'], 0x80000000)
        require(identifier > last_id and any(r['request_id'] == identifier and r['kind'] == 'DIAG'
                and i > 0 and requests[i - 1]['kind'] == 'STATUS' for i, r in enumerate(requests)), 'Observation is not bound to its ordered live request')
        last_id = identifier
        op_index = next(i for i, op in enumerate(operations) if op['request_id'] == identifier)
        require(op_index > 0 and operations[op_index]['command'] == 'DIAG' and
                operations[op_index - 1]['command'] == 'STATUS' and
                operations[op_index]['responses'] == [stage['diagnostics']] and
                operations[op_index - 1]['responses'] == [stage['status']],
                'Stage data differs from actual request responses')
        seen[stage['name']] = observation(stage['status'], stage['diagnostics'])
        check_publication(stage['name'], seen, operations[last_operation:op_index + 1])
        last_operation = op_index + 1
    require(last_id == ids[-1], 'Unaccounted operations after terminal journey observation')
    expected_commands = []
    for name in STAGES:
        expected_commands.extend('PRESS ' + action for action in stage_actions(name, seen, plan))
        expected_commands.extend(('STATUS', 'DIAG'))
        if name == 'primary_next':
            expected_commands.extend(('PRESS previous', 'STATUS', 'DIAG', 'PRESS next', 'STATUS', 'DIAG') * cycles)
    require([op['command'] for op in operations] == expected_commands,
            'Missing or wrong fixed reading journey operations')
    for name, (s, d) in seen.items():
        check_stage(name, s, d, plan, install)
    check_progress(seen)
    if 'sampling' in receipt:
        sampling.validate(receipt['sampling'], seen, stages, operations, plan, install, **sampling_checks())
    return {'state': 'hardware-validated', 'reading': 'pass', 'chapter_crossing': 'pass',
            'book_completion': 'unverified',
            'battery_wake': 'manual-pending', 'optical_buttons': 'manual-pending', 'owner': 'pending'}


def stage_actions(name, seen, plan):
    """The fixed reading journey lives at one boundary, never a caller checklist.

    The primary checkpoint must already be positioned so its second forward
    turn crosses a chapter boundary. No positioning, reset, or extra navigation
    is inserted to make an unsuitable source pass.
    """
    if name in ('library', 'sibling_library', 'rejected_library', 'sibling_recovery_library'):
        return ('previous', 'previous', 'activate')
    selects = {'primary_open': ('primary', 'library'), 'sibling_open': ('sibling', 'sibling_library'),
               'rejected_open': ('rejected', 'rejected_library'), 'recovered_open': ('primary', 'recovered_library'),
               'sibling_recovery_open': ('sibling', 'sibling_recovery_library')}
    if name in selects:
        role, before = selects[name]
        delta = plan[role]['index'] - seen[before][1]['library_selected']
        return ('next' if delta > 0 else 'previous',) * abs(delta) + ('activate',)
    singles = {'primary_next': 'next', 'primary_next2': 'next', 'primary_previous': 'previous',
               'primary_exit': 'back', 'primary_reopen': 'back', 'primary_reexit': 'back',
               'sibling_next': 'next', 'sibling_exit': 'back', 'recovered_next': 'next',
               'recovered_boundary_exit': 'back', 'recovered_boundary_reopen': 'back',
               'recovered_boundary_previous': 'previous', 'recovered_boundary_next': 'next',
               'recovered_exit': 'back', 'sibling_recovery_next': 'next', 'home_finish': 'back'}
    return (singles[name],) if name in singles else ()


def sampling_checks():
    return dict(require=require, exact=exact, observation=observation,
                anchor=anchor, check_stage=check_stage)


def journey_audit_bound(plan, cycles=0):
    return sampling.audit_bound(plan, cycles, STAGES, stage_actions, validate_plan, require)


def check_stage(name, s, d, plan, install):
    reading = name in {'primary_open', 'primary_next', 'primary_next2', 'primary_previous',
                       'primary_reopen', 'sibling_open', 'sibling_next', 'recovered_open', 'recovered_next',
                       'recovered_boundary_reopen', 'recovered_boundary_previous', 'recovered_boundary_next',
                       'sibling_recovery_open', 'sibling_recovery_next'}
    library = name in {'library', 'sibling_library', 'rejected_library', 'rejected_open', 'recovered_library', 'sibling_recovery_library'}
    require(d['device'] == install['device'], 'Wrong physical device identity')
    require(s['screen'] == (4 if reading else 6 if library else 0), 'Wrong device screen for journey stage')
    require(d['reader_open'] == int(reading), 'Actual book open/close did not complete')
    if reading:
        role = 'sibling' if name.startswith('sibling') else 'primary'
        require(d['reader_status'] == 0 and d['progress_ready'] == 1 and
                d['current_book'] == plan[role]['book'], 'Wrong/unreadable book or unavailable position')
    if library:
        require(d['library_count'] == plan['library_count'], 'Changed/truncated Library media')
        integer(d['library_selected'], high=plan['library_count'] - 1)
    if name == 'rejected_open':
        require(d['reader_status'] != 0 and d['current_book'] in (0, plan['rejected']['book']) and
                d['library_selected'] == plan['rejected']['index'], 'Intentional failed-open fixture was not rejected')


def check_publication(name, seen, operations):
    presses = [op for op in operations if op['command'].startswith('PRESS ')]
    if not presses:
        return
    terminal = presses[-1]['responses'][-1]
    require(terminal.startswith('RENDER_RETURN gen='), 'Required journey transition did not render')
    generation = int(terminal.split('=')[1])
    before = STAGES[STAGES.index(name) - 1]
    require(seen[name][0]['gen'] >= generation > seen[before][0]['gen'],
            'Reading journey did not publish a fresh completed framebuffer')


def check_progress(seen):
    if 'primary_next2' in seen:
        require(seen['primary_next2'][1]['actual_spine'] > seen['primary_next'][1]['actual_spine'],
                'Required chapter boundary did not cross into a later spine')
    # Exit closes the live reader before Home Back reopens its saved position.
    # Boot/sleep checkpoint diagnostics do not describe these ordinary saves.
    for left, right, same in (
        ('primary_open', 'primary_next', False), ('primary_next', 'primary_next2', False),
        ('primary_next', 'primary_previous', True), ('primary_previous', 'primary_reopen', True),
        ('sibling_open', 'sibling_next', False), ('primary_reopen', 'recovered_open', True),
        ('recovered_open', 'recovered_next', False), ('primary_next2', 'recovered_next', True),
        ('recovered_next', 'recovered_boundary_reopen', True),
        ('primary_previous', 'recovered_boundary_previous', True),
        ('primary_next2', 'recovered_boundary_next', True),
        ('sibling_next', 'sibling_recovery_open', True),
        ('sibling_recovery_open', 'sibling_recovery_next', False)):
        if left in seen and right in seen:
            a, b = anchor(seen[left][1]), anchor(seen[right][1])
            require(a == b if same else a < b, 'Page turns or saved-position reopen failed')


def plan_digest(plan):
    return hashlib.sha256(json.dumps(plan, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def failure_detail(error, phase, stage=None, command=None):
    """Bounded diagnostics for the private receipt; never grants acceptance."""
    detail = {'phase': phase, 'stage': stage, 'command': command,
              'type': type(error).__name__, 'message': (str(error) or type(error).__name__)[:512]}
    if error.__cause__ is not None:
        cause = error.__cause__
        detail['cause'] = {'type': type(cause).__name__,
                           'message': (str(cause) or type(cause).__name__)[:512]}
    if hasattr(error, 'press_delivery'):
        detail['press_delivery'] = error.press_delivery.copy()
    return detail


def run_journey(client, install, plan, procedure, clock=time.time, monotonic=time.monotonic, sampling_cycles=0):
    """Use an explicitly supplied existing client; never discover/reset/retry devices.

    Firmware must provide versioned diag2 fields in DIAG_KEYS. Old producers
    fail before any PRESS. The caller owns explicit physical access authorization.
    Raw book identities/media plan and this receipt stay private and ignored.
    """
    validate_plan(plan)
    require(journey_audit_bound(plan, sampling_cycles) <= MAX_AUDIT_RECORDS, 'Journey exceeds bounded protocol audit before connection')
    report = {'schema': 1, 'identity': install, 'procedure_sha256': procedure,
              'plan_sha256': plan_digest(plan), 'started_at': clock(), 'finished_at': None,
              'started_monotonic': monotonic(), 'finished_monotonic': None,
              'terminal': 'failed', 'simulation': client.connection['transport'] != 'serial',
              'hello': '', 'operations': [], 'stages': [],
              'latency': {'schema': 1, 'actions': []}}
    if sampling_cycles:
        report['sampling'] = {'schema': 1, 'stage': 'primary_next', 'cycles': sampling_cycles,
                              'initial': None, 'observations': []}
    deadline = report['started_monotonic'] + MAX_RUN
    seen = {}
    operation_start = 0
    context = {'phase': 'setup', 'stage': None, 'command': None}
    def request(command):
        context.update(phase='request', command=command)
        require(monotonic() < deadline, 'Journey monotonic deadline exceeded')
        replies = sampling.request_with_latency(client, command, context['stage'],
                                                report['latency']['actions'], deadline, monotonic, require)
        records = [r for r in client.receipt()['protocol']['records'] if r['request_id'] == client.request_id]
        require(len(records) == 1 + len(replies) and records[0].get('body') == command and
                [r.get('body') for r in records[1:]] == replies, 'Client lacks exact current wire audit bodies')
        report['operations'].append({'request_id': client.request_id, 'command': command, 'responses': replies})
        return replies
    def observe(name):
        status = request('STATUS')[0]
        diagnostics = request('DIAG')[0]
        report['stages'].append({'name': name, 'status': status, 'diagnostics': diagnostics,
                                 'request_id': client.request_id})
        context.update(phase='observation', command=None)
        result = observation(status, diagnostics)
        check_stage(name, *result, plan, install)
        seen[name] = result
        check_progress(seen)
        check_publication(name, seen, report['operations'][operation_start:])
        if name == 'primary_next2' and 'sampling' in report:
            before = observation(report['sampling']['observations'][-1]['status'],
                                 report['sampling']['observations'][-1]['diagnostics'])[0]['gen']
            terminal = report['operations'][operation_start]['responses'][-1]
            require(result[0]['gen'] >= int(terminal.split('=')[1]) > before,
                    'Chapter publication regressed after warm sampling')
        require(monotonic() <= deadline, 'Journey monotonic deadline exceeded')
        return result
    def press(action):
        require(monotonic() <= deadline, 'Journey monotonic deadline exceeded')
        request('PRESS ' + action)  # Existing client requires terminal acknowledgements.
    try:
        require(0.1 <= number(client.timeout) <= 30, 'Invalid bounded client timeout')
        context.update(phase='handshake', command='HELLO')
        report['hello'] = client.hello(install['build'])
        for name in STAGES:
            context.update(phase='stage-actions', stage=name, command=None)
            operation_start = len(report['operations'])
            for action in stage_actions(name, seen, plan):
                press(action)
            observe(name)
            if name == 'primary_next' and sampling_cycles:
                sampling.run(report, context, seen, plan, install, press, request,
                             lambda: client.request_id, **sampling_checks())
        report['terminal'] = 'complete'
    except (ValueError, OSError, KeyboardInterrupt) as error:
        # Persist the partial stages/audit; no retry or recovery press follows.
        report['terminal'] = 'failed'
        report['failure_detail'] = failure_detail(error, **context)
    finally:
        try:
            client.close()
        except (ValueError, OSError, KeyboardInterrupt) as error:
            report['terminal'] = 'failed'
            report['close_failure_detail'] = failure_detail(error, 'connection-close')
        report['finished_at'] = clock()
        report['finished_monotonic'] = monotonic()
        receipt = client.receipt()
        report.update({key: receipt[key] for key in ('connection', 'protocol')})
    return report


def write_journey(destination, client_factory, install, plan, procedure, clock=time.time, sampling_cycles=0):
    """Exclusive private output exists before connecting; failed opening keeps audit."""
    require(journey_audit_bound(plan, sampling_cycles) <= MAX_AUDIT_RECORDS, 'Journey exceeds bounded protocol audit before connection')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        try:
            client = client_factory()
        except (ValueError, OSError, KeyboardInterrupt) as error:
            receipt = {'schema': 1, 'terminal': 'failed', 'failure': 'connection-opening',
                       **{key: value for key, value in getattr(error, 'bridge_receipt', {}).items()
                          if key in ('connection', 'protocol')},
                       'failure_detail': failure_detail(error, 'connection-opening')}
        else:
            receipt = run_journey(client, install, plan, procedure, clock, sampling_cycles=sampling_cycles)
        output.write(json.dumps(receipt, sort_keys=True) + '\n')
    return receipt
