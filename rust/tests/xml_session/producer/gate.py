"""Producer extension to the existing XML gate (same approved Run/build family)."""
from contextlib import ExitStack
import hashlib
import json
import shutil
import shlex
import sys


def check(root, packet, output, runner, tools, objects, flags, build, rust,
          support, load, positive_only, canonical):
    here = packet / 'producer'
    protocol = load(here / 'protocol.py', 'producer_protocol')
    protocol.malformed_controls()
    vectors = load(here / 'vectors.py', 'producer_vectors').VECTORS
    assert len({v[0] for v in vectors}) == len(vectors)
    policy = json.loads((packet / 'reference/freeink-producer-policy.json').read_text())
    frozen_transport = load(root / 'tests/inflate/expected_transport.py', 'producer_expected')
    requests = [dict(mode=0, input=wire) for _, wire, _ in vectors]
    reference = frozen_transport.Frozen(here / 'expected-v2.json.zlib', requests, policy, literal_answers=[expected for _, _, expected in vectors])
    cpp = here / 'expected-v2.json.zlib'
    authenticated = {'policy': policy, 'evidence': 'frozen native producer capture'}
    with ExitStack() as seals:
        def reply(binary, wire):
            raw = reference.text(wire) if binary == cpp else runner.execute([binary], root, wire)
            # Validate complete transport FIRST, including command results and polls.
            return protocol.parse(raw, wire), raw

        baseline, lookup, evidence = {}, {}, []
        for name, wire, expected in vectors:
            protocol.parse(expected, wire)
            original, original_raw = reply(cpp, wire)
            actual, actual_raw = reply(rust, wire)
            support.require(original_raw == expected == actual_raw,
                            'producer independent/C++/Rust mismatch: ' + name)
            baseline[name], lookup[name] = original, wire
            evidence.append({'id': name, 'request_sha256': hashlib.sha256(wire.encode()).hexdigest(),
                             'trace_sha256': hashlib.sha256(actual_raw.encode()).hexdigest()})
        runner.run([sys.executable, '-B', here / 'rejection.py', cpp, rust], root)
        controls = json.loads((here / 'mutations.json').read_text())
        support.require(len(controls) >= 6 and len({c['id'] for c in controls}) == len(controls),
                        'producer controls incomplete')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            workspace = output / f'producer-mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / control['path']
            pristine = source.read_text()
            support.require(pristine.count(control['before']) == 1 and control['before'] != control['after'],
                            'producer mutation site must be unique')
            mutated = pristine.replace(control['before'], control['after'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                saved_flags = runner.env['RUSTFLAGS']
                if control['path'].endswith('.c'):
                    bridge = output / f'producer-mutant-bridge-{index:02d}.o'
                    original_book = support.ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
                    support.verify_source_inventory(root, canonical)
                    support.verify_source_inventory(workspace, mutant_inventory)
                    runner.run([tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99', *flags,
                                '-I', original_book / 'include', '-I', original_book / 'third_party/expat',
                                '-c', source, '-o', bridge], root)
                    args = shlex.split(saved_flags)
                    old = 'link-arg=' + str(objects['bridge'])
                    support.require(args.count(old) == 1, 'producer native bridge binding changed')
                    runner.env['RUSTFLAGS'] = shlex.join(['link-arg=' + str(bridge) if arg == old else arg for arg in args])
                try:
                    mutant = build(workspace, output / f'producer-mutant-target-{index:02d}', expected=mutant_inventory)
                finally:
                    runner.env['RUSTFLAGS'] = saved_flags
                actual, raw = reply(mutant, lookup[control['witness']])
                support.require(actual != baseline[control['witness']], 'producer control survived: ' + control['id'])
                detected.append({'id': control['id'], 'witness': control['witness'],
                                 'fresh_core_host_and_tests': True, 'complete_valid_trace': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'trace_sha256': hashlib.sha256(raw.encode()).hexdigest()})
        return {'recipe': authenticated, 'positives': evidence, 'controls': detected,
                'expected_sha256': support.digest(cpp), 'host_only': True}
