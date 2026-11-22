#!/usr/bin/env python3
"""Offline Rust checks, with a bounded landing phase and a separate full gate."""
import argparse
from collections import Counter
from contextlib import contextmanager, nullcontext
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / '.build/rust'
SCRATCH = BUILD / 'tmp'  # check() points this at its own run's folder (run_folder)
SHARED_SOURCE_FILES = ('tools/rust-toolchain.py', 'tools/host_jobs.py', 'tools/rust_gate_families.py',
                       'tools/python_runtime.py')
_command_seals = ()
sys.path.insert(0, str(ROOT / 'tools'))
from rust_gate_families import FAMILIES, ADAPTER_FAMILIES, FOUNDATION_CAMPAIGNS, foundation_partition
import host_jobs
sys.path.insert(0, str(ROOT / 'rust/tests/inflate'))
from run_support import (cargo_execution, source_inventory, verify_source_inventory, SourceSeal, source_commands,
                         close_sources, copy_workspace, install_signal_handlers)
toolchain_spec = importlib.util.spec_from_file_location('rust_toolchain', ROOT / 'tools/rust-toolchain.py')
toolchain = importlib.util.module_from_spec(toolchain_spec)
toolchain_spec.loader.exec_module(toolchain)


def run(command, *, seals=(), **kwargs):
    if 'env' not in kwargs:
        kwargs['env'] = environment()
    cwd = kwargs.pop('cwd', ROOT)
    env = kwargs.pop('env')
    try:
        context = (cargo_execution(command, cwd, env)
                   if str(command[0]) == str(ROOT / '.toolchain/rust/toolchain/bin/cargo')
                   else nullcontext(env))
        with source_commands((*_command_seals, *seals)), context as controlled:
            return host_jobs.run(list(map(str, command)), check=True, cwd=cwd, env=controlled,
                                 capture_output=True, text=kwargs.pop('text', True), timeout=180, **kwargs)
    except subprocess.CalledProcessError as error:
        print(error.stdout or '', file=sys.stderr)
        print(error.stderr or '', file=sys.stderr)
        raise


def environment():
    env = os.environ.copy()
    for name in list(env):
        if name.startswith(('CARGO_', 'RUST', 'CCC_')):
            env.pop(name)
    for name in ('PYTHONOPTIMIZE', 'LD_PRELOAD', 'LD_LIBRARY_PATH'):
        env.pop(name, None)
    prefix = ROOT / '.toolchain/rust/toolchain'
    temporary = SCRATCH
    temporary.mkdir(parents=True, exist_ok=True)
    env.update(CARGO_HOME=str(ROOT / '.build/rust/cargo-home'),
               TMPDIR=str(temporary), TMP=str(temporary), TEMP=str(temporary),
               PYTHONDONTWRITEBYTECODE='1',
               RUSTC=str(prefix / 'bin/rustc'), RUSTDOC=str(prefix / 'bin/rustdoc'),
               CARGO_BUILD_JOBS='2', CARGO_NET_OFFLINE='true', RUST_TEST_THREADS='2',
               XDG_CACHE_HOME=str(ROOT / '.build/rust/cache'), RUSTFLAGS='-Dwarnings',
               ASAN_OPTIONS='halt_on_error=1:abort_on_error=1', UBSAN_OPTIONS='halt_on_error=1')
    return env


def cargo(arguments, target_dir, manifest=None):
    manifest = manifest or ROOT / 'rust/Cargo.toml'
    command = [ROOT / '.toolchain/rust/toolchain/bin/cargo', *arguments,
               '--manifest-path', manifest, '--offline', '--locked', '--jobs', '2', '--frozen',
               '--target-dir', target_dir]
    result = run(command, env=environment(), cwd=manifest.parent)
    if result.stdout.strip(): print(result.stdout.strip(), flush=True)
    if result.stderr.strip(): print(result.stderr.strip(), flush=True)


def responses(binary, cases, *, seals=()):
    if any(case.startswith('frame_prefix ') for case in cases):
        # Decode raw bytes before splitting; text I/O would hide malformed CRLF.
        from prefix_validation import batch_lines
        result = run([binary], input=('\n'.join(cases) + '\n').encode('ascii'),
                     cwd=ROOT, env=environment(), text=False, seals=seals)
        lines = batch_lines(result.stdout, len(cases))
    else:
        result = run([binary], input='\n'.join(cases) + '\n', env=environment(), seals=seals)
        lines = result.stdout.splitlines()
    if len(lines) != len(cases):
        raise RuntimeError(f'{binary.name}: expected {len(cases)} replies, got {len(lines)}')
    from persistence_protocol_validation import validate
    for case, reply in zip(cases, lines):
        if case.startswith('frame_prefix '):
            from prefix_validation import validate as validate_prefix
            validate_prefix(reply)
        if case.startswith('stats_persist '):
            validate(reply, bytes.fromhex(case.split()[1]))
        if case.startswith('xp.'):
            from external_progress_validation import validate as validate_external
            validate_external(reply, case.split()[0])
    from stats_service_validation import validate_wire
    for case,reply in zip(cases,lines):
        if case.startswith('stats_service '):validate_wire(reply,case)
    return lines


def reply_matches(want, got):
    if want.startswith('sha256:'):
        return want == 'sha256:' + hashlib.sha256(got.encode('utf-8')).hexdigest()
    return want == got


def compare(binary, cases, expected, *, seals=()):
    actual = responses(binary, cases, seals=seals)
    return next(((index, case, want, got) for index, (case, want, got)
                 in enumerate(zip(cases, expected, actual)) if not reply_matches(want, got)), None)


def compare_batches(binary, cases, expected, batch_size=64, *, seals=()):
    """A compiled mutant needs one behavioral witness; positives still run all cases."""
    if batch_size <= 0 or len(cases) != len(expected):
        raise ValueError('invalid mutation comparison batch')
    for offset in range(0, len(cases), batch_size):
        end = min(offset + batch_size, len(cases))
        mismatch = compare(binary, cases[offset:end], expected[offset:end], seals=seals)
        if mismatch is not None:
            index, case, want, got = mismatch
            return (offset + index, case, want, got), end
    return None, len(cases)


CONTROLS = (
    ('half-range ambiguity', 'core/src/progress.rs', 'delta == 0x8000_0000 ||', 'false ||'),
    ('future preference classification', 'core/src/preferences.rs',
     'return Err(DecodeError::Unsupported);', 'return Err(DecodeError::Corrupt);'),
    ('write acknowledgement required', 'core/src/durable.rs',
     'store.put_bytes(DurableWrite(()), key, bytes) == bytes.len()',
     'store.put_bytes(DurableWrite(()), key, bytes) <= bytes.len()'),
    ('unknown schemas protected', 'core/src/durable.rs',
     'observed > N || !C::known(&slots.bytes[i][..observed])', 'observed > N'),
    ('acknowledged peer protected', 'core/src/durable.rs',
     'let target = 1 - kept;', 'let target = if kept == newest { 1 - kept } else { 1 - newest };'),
    ('colliding checkpoints rejected', 'core/src/progress_store.rs',
     'if found.is_some_and(|previous| self.bytes[previous] != self.bytes[index])', 'if false'),
    ('progress session credit retained', 'core/src/progress_store.rs',
     'session_milliseconds: session,', 'session_milliseconds: session.wrapping_mul(0),'),
    ('short future preference headers preserved', 'core/src/preferences_store.rs',
     'observed >= 8', 'observed >= BYTES'),
    ('preference peers both inspected', 'core/src/preferences_store.rs',
     'let second = self.read_slot(backend, 1);',
     'let second = if first == Slot::Failed { Slot::Missing } else { self.read_slot(backend, 1) };'),
    ('identical preferences avoid rewrites', 'core/src/preferences_store.rs',
     'transaction.0.readback_durable()', '(transaction.0.readback_durable() && false)'),
    ('valid SD envelopes accepted', 'core/src/sd_record.rs', '&record[..4] == tag', 'false && &record[..4] == tag'),
    ('SD checksum required', 'core/src/sd_record.rs',
     'get32(record, RECORD_BYTES - 4) == crc32c(&record[..RECORD_BYTES - 4])',
     '(get32(record, RECORD_BYTES - 4) == crc32c(&record[..RECORD_BYTES - 4]) || true)'),
)


# All positive cases always run. Mutants search affected families until the first
# behavioral mismatch; unknown modules conservatively select the whole corpus.
CONTROL_FAMILIES = {
    'progress.rs': ('progress_', 'xp.', 'stats_service'),
    'external_progress_store.rs': ('xp.', 'stats_service'),
    'progress_store.rs': ('progress_store', 'xp.', 'stats_service'),
    'preferences.rs': ('prefs_', 'geometry', 'preferences_store'),
    'preferences_store.rs': ('preferences_store',),
    'durable.rs': ('store', 'progress_store', 'preferences_store', 'stats_persist', 'xp.', 'stats_service'),
    'sd_record.rs': ('sd_',),
    'time_zone.rs': ('zone_info', 'clock_time'),
    'clock_format.rs': ('clock_format_info', 'clock_text'),
    'frame.rs': ('frame_',),
    'tracker.rs': ('tracker', 'stats_service'),
    'stats_clock.rs': ('tracker', 'stats_ledger', 'stats_make', 'stats_summary', 'stats_wire', 'sc_sleep', 'book_graph', 'stats_persist', 'stats_service'),
    'library_catalog.rs': ('catalog', 'catalog_path'),
    'utf8.rs': ('utf8_next',),
    'zip_record_rules.rs': ('zip_',),
    'sleep_stats.rs': ('sc_sleep', 'sc_streak', 'stats_persist', 'stats_service'),
    'crossink_stats.rs': ('sc_', 'book_graph', 'stats_persist', 'stats_service'),
    'reading_stats_store.rs': ('stats_persist', 'stats_service'),
    'book_days_store.rs': ('stats_persist', 'stats_service'),
    'book_days.rs': ('book_graph', 'stats_persist', 'stats_service'),
    'stats_graph_data.rs': ('book_graph',),
    'reading_stats.rs': ('stats_ledger', 'stats_wire', 'stats_make', 'stats_summary', 'sc_sleep', 'book_graph', 'stats_persist', 'stats_service'),
    'selected_book.rs': ('selected_',),
    'sd_resume.rs': ('sd_', 'selected_'),
    'library_name.rs': ('name_valid', 'sd_', 'selected_'),
    'sleep_intent.rs': ('sleep_',),
}


def control_indices(relative, cases, name=''):
    families = ('frame_prefix',) if name.startswith('prefix ') else ('stats_service',) if name.startswith('service-') else ('xp.',) if name.startswith('external-') else CONTROL_FAMILIES.get(Path(relative).name)
    selected = [index for index, case in enumerate(cases)
                if families is None or case.split(maxsplit=1)[0].startswith(families)]
    if not selected:
        raise RuntimeError(f'Negative control {relative}: no affected-family cases')
    return selected


CLOCK_CONTROLS = (
    ('DST start is inclusive', 'core/src/time_zone.rs',
     'now >= start && now < end', 'now > start && now < end'),
    ('southern DST crosses New Year', 'core/src/time_zone.rs',
     'now >= start || now < end', 'now >= start && now < end'),
    ('half-hour zone offset retained', 'core/src/time_zone.rs',
     'standard_minutes: 330,', 'standard_minutes: 300,'),
    ('Gregorian century correction', 'core/src/time_zone.rs',
     '- yoe / 100 + doy', '- yoe / 400 + doy'),
    ('invalid minutes preserve original text', 'core/src/clock_format.rs',
     "|| bytes[3] > b'5'", "|| bytes[3] > b'9'"),
    ('clock spare bytes remain untouched', 'core/src/clock_format.rs',
     'out[length + 7] = 0;', 'out[length + 7] = 0; out[8] = 0;'),
)


def control_receipt(name, index, case, comparisons):
    receipt = {'name': name, 'case': index, 'comparisons': comparisons,
               'input_sha256': hashlib.sha256(case.encode()).hexdigest()}
    # Full-frame cases are deterministic corpus entries, not stored fixture blobs.
    receipt['input' if len(case) <= 2048 else 'input_prefix'] = case[:2048]
    return receipt


def require_mutant_compilation(output, workspace):
    expected = {str(workspace / name / 'Cargo.toml') for name in ('core', 'host')}
    compiled = set()
    for line in output.splitlines():
        message = json.loads(line)
        manifest = message.get('manifest_path')
        if message.get('reason') == 'compiler-artifact' and manifest in expected:
            if message.get('fresh') is not False:
                raise RuntimeError('Mutation build reused a complete crate artifact')
            compiled.add(manifest)
    if compiled != expected:
        raise RuntimeError('Mutation build did not report compiling both core and host')


def build_mutant(target, workspace, *, seals=()):
    # Legacy controls execute this transport; engine families build their own.
    result = run([ROOT / '.toolchain/rust/toolchain/bin/cargo', 'build', '-p', 'newt-parity', '--bin', 'newt-parity',
                  '--message-format=json-render-diagnostics', '--manifest-path',
                  workspace / 'Cargo.toml', '--offline', '--locked', '--jobs', '2', '--frozen', '--target-dir', target],
                 env=environment(), cwd=workspace, seals=seals)
    require_mutant_compilation(result.stdout, workspace)
    if result.stderr.strip(): print(result.stderr.strip(), flush=True)


def foundation_registry():
    from external_progress_controls import CONTROLS as EXTERNAL_CONTROLS
    from stats_service_controls import CONTROLS as SERVICE_CONTROLS
    from admission_controls import CONTROLS as ADMISSION_CONTROLS
    from catalog_controls import CONTROLS as CATALOG_CONTROLS
    from sc_controls import CONTROLS as SC_CONTROLS
    from book_graph_controls import CONTROLS as BOOK_GRAPH_CONTROLS
    from persistence_controls import CONTROLS as PERSISTENCE_CONTROLS
    from stats_controls import CONTROLS as STATS_CONTROLS
    from tracker_controls import CONTROLS as TRACKER_CONTROLS
    from identity_corpus import CONTROLS as IDENTITY_CONTROLS
    from frame_cases import CONTROLS as FRAME_CONTROLS
    from prefix_controls import CONTROLS as PREFIX_CONTROLS
    return {'base': CONTROLS, 'clock': CLOCK_CONTROLS, 'frame': FRAME_CONTROLS,
            'identity': IDENTITY_CONTROLS, 'tracker': TRACKER_CONTROLS, 'stats': STATS_CONTROLS,
            'catalog': CATALOG_CONTROLS, 'admission': ADMISSION_CONTROLS, 'sc': SC_CONTROLS,
            'book-graph': BOOK_GRAPH_CONTROLS, 'persistence': PERSISTENCE_CONTROLS,
            'external': EXTERNAL_CONTROLS, 'service': SERVICE_CONTROLS, 'prefix': PREFIX_CONTROLS}


def controls(cases, expected, snapshot, campaign=None):
    registry = foundation_registry()
    partition = foundation_partition(registry)
    selected = partition[campaign] if campaign else tuple(control for module in registry.values() for control in module)
    detected = []
    with tempfile.TemporaryDirectory(prefix='controls-', dir=SCRATCH) as directory:
        workspace = Path(directory) / 'rust'
        source_manifest = source_inventory(snapshot / 'rust')
        copy_workspace(snapshot / 'rust', workspace)
        # Fixtures stay sealed in the snapshot; Cargo only consumes its workspace.
        source_manifest = {name: source_manifest[name] for name in source_inventory(workspace)}
        verify_source_inventory(workspace, source_manifest)
        for name, relative, before, after in selected:
            source = workspace / relative; original = source.read_text()
            if original.count(before) != 1:
                raise RuntimeError(f'Negative control {name}: mutation site changed')
            intended = original.replace(before, after).encode()
            source.write_bytes(intended)
            mutated_manifest = dict(source_manifest)
            mutated_manifest[relative] = hashlib.sha256(intended).hexdigest()
            seal = SourceSeal(workspace, mutated_manifest)
            target = Path(directory) / 'target'
            try:
                # A compiler error is never accepted as detection of a behavior bug.
                build_mutant(target, workspace, seals=(seal,))
                indices = control_indices(relative, cases, name)
                mismatch, checked = compare_batches(target / 'debug/newt-parity', [cases[i] for i in indices],
                                                    [expected[i] for i in indices], seals=(seal,))
            finally:
                seal.close()
                source.write_text(original)
                verify_source_inventory(workspace, source_manifest)
            if mismatch is None:
                raise RuntimeError(f'Negative control survived: {name}')
            receipt = control_receipt(name, indices[mismatch[0]], mismatch[1], checked)
            receipt['available_comparisons'] = len(indices)
            receipt['rust_host_binary_sha256'] = digest(target / 'debug/newt-parity')
            receipt['core_and_host_recompiled'] = True
            detected.append(receipt)
            print(f'PASS negative control: {name} (case {indices[mismatch[0]]})', flush=True)
    return detected


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_hashes(root=ROOT, *, landing=False, sdk=False):
    sources = {'rust/' + name: sha for name, sha in source_inventory(root / 'rust').items()}
    sources.update({name: digest(root / name) for name in SHARED_SOURCE_FILES})
    # Admit every rejection driver before expensive comparisons. Individual
    # families retain the runtime cancellation/cleanup controls as well.
    from owned_children_test import validate_driver
    for relative in sources:
        path = Path(relative)
        if relative.startswith('rust/tests/') and path.name.startswith('rejection') and path.suffix == '.py':
            try:
                validate_driver((root / relative).read_text())
            except ValueError as error:
                raise RuntimeError(f'Rejection driver ownership: {relative}: {error}') from error
    if landing:
        for name in ('tools/clean_export.py', 'tools/release_tools.py',
                     'tools/clean-export-emails.txt', 'tools/clean-export-allow.txt'):
            sources[name] = digest(root / name)
    if landing or sdk:
        sources['LICENSE'] = digest(root / 'LICENSE')
        spec = importlib.util.spec_from_file_location('landing_inputs', root / 'rust/landing.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        sources.update(module.sdk_inputs(root))
    return sources


def verify_sources(before, root=ROOT):
    if source_hashes(root, landing='tools/clean_export.py' in before,
                     sdk=any(name.startswith('vendor/') for name in before)) != before:
        raise RuntimeError('Experiment sources changed during the check; no new pass evidence published')


def snapshot_sources(before, destination, root=ROOT):
    for relative, expected in before.items():
        data = (root / relative).read_bytes()
        if hashlib.sha256(data).hexdigest() != expected:
            raise RuntimeError('Experiment sources changed while taking the build snapshot')
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(data)
    verify_sources(before, root)
    verify_source_inventory(destination, before)


@contextmanager
def run_folder(receipt):
    """Own one receipt and its run's temporary folder beside it (#345).

    Runs of one receipt take turns; runs with different receipts share no file
    here, so the gate runs them in parallel (Cargo locks the caches they
    share). Every temporary file of the run goes in the folder (SCRATCH), which
    is removed when the run ends, pass or fail. A run killed outright cannot
    remove its own, so each run first removes every folder whose lock is free.
    """
    global SCRATCH
    receipt.parent.mkdir(parents=True, exist_ok=True)
    with receipt.with_suffix('.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)  # waits only for a run of this receipt or a sweep
        folder = receipt.with_suffix('.tmp')
        for other in receipt.parent.glob('*.tmp'):
            if other != folder and other.is_dir():
                with other.with_suffix('.lock').open('a') as owner:
                    try:
                        fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    except BlockingIOError:
                        continue  # its run is live
                    shutil.rmtree(other, ignore_errors=True)
        shutil.rmtree(folder, ignore_errors=True)
        folder.mkdir()
        previous, SCRATCH = SCRATCH, folder
        try:
            yield
        except BaseException:
            show_failure_logs(folder)  # the folder and its logs go next
            raise
        finally:
            SCRATCH = previous
            shutil.rmtree(folder, ignore_errors=True)


def show_failure_logs(folder, lines=80):
    """Print the tail of the failing command's output, which the run's folder held."""
    logs = sorted((log for pattern in ('*.stdout', '*.stderr') for log in folder.rglob(pattern) if log.is_file()),
                  key=lambda log: log.stat().st_mtime)[-2:]
    for log in logs:
        text = log.read_text(errors='replace').splitlines()[-lines:]
        print(f'--- {log.relative_to(folder)} (last {len(text)} lines) ---', *text, sep='\n', file=sys.stderr, flush=True)


@contextmanager
def shared_target(args):
    """Hold BUILD/target, which landing, the target probes and a full run all
    build into, through building, running and hashing its binaries."""
    if args.family or args.foundation_controls:
        yield
        return
    with (BUILD / 'target.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def independent_expectations(cases, replies, implementation):
    from prefix_cases import assert_expectations as assert_prefix_expectations
    assert_prefix_expectations(cases, replies)
    from stats_service_cases import assert_expectations as assert_service
    assert_service(cases,replies)
    from external_progress_cases import assert_expectations as assert_external_progress
    assert_external_progress(cases, replies)
    from admission_cases import check_independent
    check_independent(cases, replies)
    from catalog_cases import assert_catalog_expectations
    assert_catalog_expectations(cases, replies)
    from sc_cases import assert_expectations as assert_sc_expectations
    assert_sc_expectations(cases, replies)
    from book_graph_cases import assert_expectations as assert_book_graph_expectations
    assert_book_graph_expectations(cases, replies)
    from persistence_cases import assert_expectations as assert_persistence_expectations
    assert_persistence_expectations(cases, replies)
    from stats_cases import assert_stats_expectations
    assert_stats_expectations(cases, replies)
    from tracker_cases import assert_tracker_expectations
    assert_tracker_expectations(cases, replies)
    from clock_cases import clock_goldens
    from frame_cases import assert_invariants
    lookup = dict(zip(cases, replies))
    for case, expected in clock_goldens().items():
        if lookup.get(case) != expected:
            raise RuntimeError(f'{implementation} clock golden mismatch: {case}')
    for case, reply in zip(cases, replies):
        if case.startswith('frame_'):
            try:
                assert_invariants(case, reply)
            except AssertionError as error:
                raise RuntimeError(f'{implementation} frame invariant: {case[:100]}') from error


def evidence_path(positive_only, tier='full', scope=None):
    if scope:
        return BUILD / ('phase-' + scope + '.json')
    if positive_only:
        return BUILD / 'positive-feedback.json'
    return BUILD / ('evidence-fast.json' if tier == 'fast' else 'evidence.json')


def positive_only_for(args, family):
    return family not in args.control_families


def selection_evidence(args):
    return {'tier': args.tier, 'selected_control_families': list(args.control_families),
            'requested_control_families': args.control_family,
            'complete_rust_gate': args.tier == 'full' and not args.positive_only
                and not (args.family or args.foundation_controls or args.target_probes)}


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    families = FAMILIES
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument('--landing', action='store_true', help='bounded landing; never the complete Rust gate')
    scope.add_argument('--family', choices=ADAPTER_FAMILIES, help='only this family; not the complete Rust gate')
    scope.add_argument('--foundation-controls', choices=FOUNDATION_CAMPAIGNS,
                       help='one whole-module mutation campaign without the positive sweep')
    scope.add_argument('--target-probes', action='store_true', help='synthetic RV32 archive and every probe verifier')
    parser.add_argument('--tier', choices=('fast', 'full'), default=None)
    parser.add_argument('--control-family', action='append', choices=(*families, 'all', 'none'),
                        help='FAST: declare touched families; repeat names, or use all/none alone')
    parser.add_argument('--positive-only', action='store_true', help='edit feedback; omits mutation controls')
    args = parser.parse_args(argv)
    if args.landing or args.foundation_controls or args.target_probes:
        if args.positive_only or args.tier is not None or args.control_family is not None:
            parser.error('landing/foundation-controls/target-probes cannot be combined with tier/positive/control selectors')
        args.tier = ('landing' if args.landing else
                     'foundation-controls-' + args.foundation_controls if args.foundation_controls else 'target-probes')
        args.control_families = ('foundation',) if args.foundation_controls else ()
    elif args.positive_only:
        if args.tier is not None or args.control_family is not None:
            parser.error('--positive-only cannot be combined with --tier or --control-family')
        args.tier = 'positive-only'
        args.control_families = ()
    else:
        args.tier = args.tier or 'full'
        if args.tier == 'full':
            if args.control_family is not None:
                parser.error('FULL always runs all controls; --control-family is FAST-only')
            args.control_families = families
        else:
            selected = args.control_family
            if not selected:
                parser.error('FAST requires --control-family NAME, all, or explicit none')
            if ('all' in selected or 'none' in selected) and len(selected) != 1:
                parser.error('all/none must be the sole --control-family selection')
            args.control_families = (families if selected == ['all'] else
                                     tuple(name for name in families if name in selected))
    if args.family:
        if args.control_family and any(name not in (args.family, 'all', 'none') for name in args.control_family):
            parser.error('--family cannot select controls of another family')
        args.control_families = tuple(name for name in args.control_families if name == args.family)
    return args


def check_snapshot(args, before, snapshot, identity):
    global _command_seals
    if _command_seals:
        raise RuntimeError('nested snapshot check')
    _command_seals = tuple(SourceSeal(snapshot / prefix,
        {name[len(prefix)+1:]: value for name, value in before.items()
         if name.startswith(prefix + '/')}) for prefix in ('rust', 'tools', *(['vendor'] if any(name.startswith('vendor/') for name in before) else [])))
    try:
        if getattr(args, 'landing', False):
            spec = importlib.util.spec_from_file_location('rust_landing', snapshot / 'rust/landing.py')
            landing = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(landing)
            return landing.check(sys.modules[__name__], before, snapshot, identity)
        return _check_snapshot(args, before, snapshot, identity)
    finally:
        try:
            close_sources(_command_seals)
        finally:
            _command_seals = ()


def foundation_inputs(snapshot):
    sys.path.insert(0, str(snapshot / 'rust/tests'))
    sys.path.insert(0, str(snapshot / 'rust'))
    spec = importlib.util.spec_from_file_location('rust_corpus', snapshot / 'rust/tests/corpus.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    cases = module.corpus()
    from expected import load as load_expected
    expected = load_expected(snapshot / 'rust/tests/foundation-expected.json.zlib', cases)
    # Cold restart reconstructs a fresh backend from the recorded durable bytes,
    # discarding live state and any buffered writes from the preceding attempt.
    restarts = []
    for case, response in zip(cases, expected):
        fields = case.split()
        if fields[0] == 'store' and fields[7] == 'publish':
            restarts.append(' '.join(['store', 'zone', '1', '0', 'g',
                                      *response.split()[-2:], 'restore', '0', '14']))
    from identity_corpus import cold_restarts
    restarts.extend(cold_restarts(cases, expected))
    expected.extend(load_expected(snapshot / 'rust/tests/foundation-restarts-expected.json.zlib', restarts))
    cases.extend(restarts)
    return cases, expected


def foundation(snapshot, target, *, portable_done=False):
    manifest = snapshot / 'rust/Cargo.toml'
    cases, expected = foundation_inputs(snapshot)
    run([sys.executable, '-B', '-m', 'unittest', '-v', 'expected_test', 'persistence_validation_test', 'prefix_validation_test', 'external_progress_validation_test', 'stats_service_validation_test'],
        cwd=snapshot / 'rust/tests', env=environment())
    if not portable_done:
        cargo(['test', '-p', 'newt-core'], target, manifest)
        cargo(['test', '-p', 'newt-parity'], target, manifest)
    cargo(['build', '-p', 'newt-parity'], target, manifest)
    binary = target / 'debug/newt-parity'
    actual = responses(binary, cases)
    if any(reply.startswith('sha256:') for reply in actual):
        raise RuntimeError('Rust reply must be full raw output, never a digest')
    verbatim = [(c, e) for c, e in zip(cases, expected) if not e.startswith('sha256:')]
    independent_expectations([c for c, _ in verbatim], [e for _, e in verbatim], 'recorded C++')
    independent_expectations(cases, actual, 'Rust')
    mismatch = next(((i, c, a, b) for i, (c, a, b) in enumerate(zip(cases, expected, actual))
                     if not reply_matches(a, b)), None)
    if mismatch:
        got_preview = ('sha256:' + hashlib.sha256(mismatch[3].encode('utf-8')).hexdigest()
                       if mismatch[2].startswith('sha256:') else mismatch[3])
        raise RuntimeError(f'Parity mismatch {mismatch[0]}:\n{mismatch[1][:200]}\nC++: {mismatch[2]}\nRust: {got_preview}')
    print(f'PASS {len(cases)} recorded C++/Rust comparisons', flush=True)
    return cases, expected, binary


def load_family(snapshot, family):
    path = snapshot / 'rust' / f'{family}_gate.py'
    spec = importlib.util.spec_from_file_location(f'{family}_gate', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def target_probes(snapshot, target):
    manifest = snapshot / 'rust/Cargo.toml'
    cargo(['build', '-p', 'newt-core', '-p', 'newt-target-probe', '--release',
           '--target', 'riscv32imc-unknown-none-elf'], target, manifest)
    artifact = target / 'riscv32imc-unknown-none-elf/release/libnewt_target_probe.a'
    symbols = run(['nm', '--defined-only', artifact]).stdout
    for name in ('newt_rust_stats_service_probe', 'newt_rust_external_progress_probe', 'newt_rust_core_probe', 'newt_rust_clock_probe', 'newt_rust_clock_format_probe', 'newt_rust_frame_probe', 'newt_rust_tracker_probe',
                 'newt_rust_stats_seed_probe', 'newt_rust_stats_transition_probe',
                 'newt_rust_stats_decode_probe', 'newt_rust_stats_encode_probe',
                 'newt_rust_stats_summary_probe', 'newt_rust_stats_facts_probe',
                 'newt_rust_sleep_crossink_sleep_probe', 'newt_rust_sleep_crossink_decode_probe',
                 'newt_rust_sleep_crossink_encode_probe', 'newt_rust_stats_store_load_probe', 'newt_rust_stats_store_save_probe',
                 'newt_rust_stats_store_repair_probe', 'newt_rust_stats_store_fold_probe',
                 'newt_rust_stats_store_names_probe', 'nrust19_fold_probe', 'nrust19_decode_probe', 'nrust19_book_graph_probe',
                 'nrust19_library_probe', 'nrust19_crossink_probe', 'nrust19_helpers_probe',
                 'nrust19_archive_encode_probe', 'newt_rust_catalog_probe', 'newt_rust_utf8_probe', 'newt_rust_zip_probe'):
        if name not in symbols:
            raise RuntimeError(f'Target archive lacks the monomorphized probe: {name}')
    evidence = {}
    # These focused recipes have no synthetic target verifier; every other
    # adapter must retain its verifier instead of silently losing target proof.
    without_verifier = {'reader_atlas', 'reader_navigation', 'epub_admission'}
    for family in ADAPTER_FAMILIES:
        if family in without_verifier:
            continue
        module = load_family(snapshot, family)
        key = family + ('_probe_symbol' if family in ('inflate', 'hyphenator', 'xml_session') else '_probe_symbols')
        evidence[key] = module.verify_target(sys.modules[__name__], artifact)
    evidence.update(target='riscv32imc-unknown-none-elf',
                    artifact_kind='static library with synthetic backend',
                    target_archive_bytes=artifact.stat().st_size, target_archive_sha256=digest(artifact))
    print(f'PASS target static library, not firmware: {artifact.relative_to(ROOT)}', flush=True)
    return evidence


def _check_snapshot(args, before, snapshot, identity):
    verify_source_inventory(snapshot, before)
    sys.path.insert(0, str(snapshot / 'rust/tests'))
    sys.path.insert(0, str(snapshot / 'rust'))
    scope = (args.family or ('foundation-controls-' + args.foundation_controls if args.foundation_controls else
                            'target-probes' if args.target_probes else None))
    evidence = {'reference': json.loads((snapshot / 'rust/reference.json').read_text()),
                'toolchain': identity, 'source_sha256': before,
                'firmware_linked': False, 'hardware_checked': False, **selection_evidence(args)}
    if scope:
        evidence['scope'] = scope
    if not args.family and not args.foundation_controls:
        tests = run([sys.executable, '-B', snapshot / 'rust/tests/runner_test.py'])
        print((tests.stdout + tests.stderr).strip(), flush=True)
    target = BUILD / 'target'
    comparisons, negative = 0, 0
    if not args.family and not args.target_probes:
        if args.foundation_controls:
            cases, expected = foundation_inputs(snapshot)
        else:
            cases, expected, binary = foundation(snapshot, target)
            comparisons = len(cases)
            evidence['rust_host_binary_sha256'] = digest(binary)
        detected = [] if positive_only_for(args, 'foundation') else controls(cases, expected, snapshot, args.foundation_controls)
        negative += len(detected)
        evidence.update(corpus_sha256=hashlib.sha256(('\n'.join(cases) + '\n').encode()).hexdigest(),
                        comparisons=comparisons, families=dict(Counter(line.split()[0] for line in cases)),
                        negative_controls=detected,
                        expected_sha256={name: digest(snapshot / 'rust/tests' / name) for name in
                            ('foundation-expected.json.zlib', 'foundation-restarts-expected.json.zlib')})
    if not args.family and not args.foundation_controls:
        evidence.update(target_probes(snapshot, target))
    if not args.foundation_controls and not args.target_probes:
        for family in ((args.family,) if args.family else ADAPTER_FAMILIES):
            result = load_family(snapshot, family).check(sys.modules[__name__], snapshot,
                                                       positive_only_for(args, family))
            evidence[family] = result
            comparisons += result['comparisons'] if 'comparisons' in result else len(result['independent_cases'])
            negative += len(result['controls'] if family in ('reader_atlas', 'reader_navigation') else
                            result['semantic_controls'])
    evidence.update(total_comparisons=comparisons, total_negative_controls=negative)
    output = evidence_path(args.positive_only, args.tier, scope)
    expected_toolchain = json.loads((snapshot / 'rust/toolchain.json').read_text())
    if toolchain.verify_installed(expected_toolchain) != identity:
        raise RuntimeError('Installed Rust toolchain changed during the check')
    verify_sources(before)
    verify_source_inventory(snapshot, before)
    close_sources(_command_seals)
    temporary = output.with_suffix('.json.partial')
    temporary.write_text(json.dumps(evidence, indent=2) + '\n')
    temporary.replace(output)
    print(f'Evidence: {output.relative_to(ROOT)}', flush=True)


def check(args):
    scope = args.family or ('foundation-controls-' + args.foundation_controls if args.foundation_controls else
                            'target-probes' if args.target_probes else None)
    output = BUILD / 'landing.json' if args.landing else evidence_path(args.positive_only, args.tier, scope)
    with run_folder(output), shared_target(args):
        if args.landing or scope:
            output.unlink(missing_ok=True)
        before = source_hashes(landing=args.landing, sdk=True)
        with tempfile.TemporaryDirectory(prefix='inputs-', dir=SCRATCH) as directory:
            snapshot = Path(directory)
            snapshot_sources(before, snapshot)
            expected_toolchain = json.loads((snapshot / 'rust/toolchain.json').read_text())
            identity = toolchain.verify_installed(expected_toolchain)
            check_snapshot(args, before, snapshot, identity)


def main():
    args = parse_args()
    if sys.flags.optimize:
        raise RuntimeError('Run Rust checks without Python optimization')
    install_signal_handlers()  # SIGTERM unwinds too, so the run's folder goes
    with toolchain.lease(shared=True):
        check(args)


if __name__ == '__main__':
    main()
