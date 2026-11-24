#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the firmware driver with real parsing/drawing and host I/O ports."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
HOST = 'x86_64-unknown-linux-gnu'


def paths(text, parent, replacements=None):
    replacements = replacements or {}
    def replace(match):
        source = (parent / match[1]).resolve()
        return '#[path = ' + json.dumps(str(replacements.get(source, source))) + ']'
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]', replace, text)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--tool-root', type=Path, default=ROOT)
    parser.add_argument('--target-dir', type=Path)
    args = parser.parse_args()
    root = args.tool_root.resolve()
    workspace = HERE.parents[1]
    sys.path.insert(0, str(root / 'tools'))
    from host_common import crypto_flags
    out = args.output.resolve()
    assert out.is_relative_to(root / '.build') and not out.exists()
    out.mkdir(parents=True)
    target = args.target_dir.resolve() if args.target_dir else out / 'target'
    assert target.is_relative_to(root / '.build')
    tools = root / '.toolchain'
    rustc = tools / 'rust/toolchain/bin/rustc'
    cargo = tools / 'rust/toolchain/bin/cargo'
    clang = tools / 'llvm/bin/clang++'
    environment = os.environ.copy()
    environment['RUSTC'] = str(rustc)
    commands = []
    def run(command, *, succeeds=True):
        result = subprocess.run(list(map(str, command)), cwd=workspace, env=environment,
                                capture_output=True, text=True, timeout=120)
        index = len(commands)
        (out / f'{index:02d}.stdout').write_text(result.stdout)
        (out / f'{index:02d}.stderr').write_text(result.stderr)
        commands.append({'command': list(map(str, command)), 'exit': result.returncode})
        (out / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        if succeeds and result.returncode:
            raise RuntimeError(result.stderr[-5000:])
        return result
    run([cargo, 'build', '--lib', '-p', 'newt-c3-board', '-p', 'newt-device-ui',
         '-p', 'newt-target-expat', '--features', 'newt-reader-ui/native-host',
         '--target', HOST, '--manifest-path', workspace / 'Cargo.toml',
         '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', target,
         '--message-format=json'])
    deps = target / HOST / 'debug/deps'
    libraries = {}
    for line in (out / '00.stdout').read_text().splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact':
            for file in item['filenames']:
                if file.endswith('.rlib'):
                    libraries[item['target']['name']] = Path(file)
    spec = importlib.util.spec_from_file_location('reader_native_pins', workspace / 'tests/phased_reader/native.py')
    native_pins = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native_pins)
    support = native_pins.load('short_turn_support', workspace / 'tests/inflate/run_support.py')
    native_pins.verify_native_inputs(support, root / 'vendor/freeink-sdk')
    sdk = root / 'vendor/freeink-sdk/libs/book/FreeInkBook'
    include = [value for path in [sdk / 'include', sdk / 'third_party/expat',
               sdk / 'third_party/miniz', sdk / 'third_party/libunibreak']
               for value in ('-I', path)]
    objects = []
    for name in ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge', 'board'):
        source = (workspace / 'target-expat/native/expat_bridge.c' if name == 'bridge'
                  else workspace / 'tests/reader_checkpoint/board_fake.c' if name == 'board'
                  else sdk / 'src/vendor' / (name + '.c'))
        obj = out / (name + '.o')
        run([clang, '--no-default-config', '-x', 'c', '-std=c99', '-O1', '-g',
             '-fsanitize=address,undefined', '-fno-sanitize-recover=all', *include,
             '-I', workspace / 'salamander/board/native', '-c', source, '-o', obj])
        objects.append(obj)
    spec = importlib.util.spec_from_file_location('short_turn_fixtures', workspace / 'tests/reader_checkpoint/fixtures.py')
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    epub = fixture.write(out / 'fixtures')
    command = [rustc, '--edition=2024', '--target', HOST, '-Copt-level=1',
               '-Clinker=' + str(clang), '-Clink-arg=--no-default-config',
               '-Clink-arg=--ld-path=/usr/bin/x86_64-linux-gnu-ld.bfd',
               '-Ldependency=' + str(deps)]
    for name, library in libraries.items():
        command += ['--extern', name + '=' + str(library)]
    command += ['-Clink-arg=' + str(obj) for obj in objects]
    command += ['-Clink-arg=-fsanitize=address,undefined', '-Clink-arg=-fno-sanitize-recover=all',
                *['-Clink-arg=' + arg for arg in crypto_flags()[2:]]]
    run([*command, HERE / 'host.rs', '-o', out / 'positive'])
    result = run([out / 'positive', epub])
    assert result.stdout.startswith('PASS actual short open/action/finished')

    def run_recovery(command, *, witness=None):
        result = run(command, succeeds=witness is None)
        if witness is not None:
            assert result.returncode == 101 and witness in result.stderr, result.stderr
        return result.stdout

    check_recovery(run_recovery, command, workspace, out, out / 'positive', controls=True)
    composition_controls(run, command, epub, workspace, out)


def check_recovery(run, command, workspace, out, positive, *, controls):
    policy = run([positive, '--restore-policy'])
    assert 'PASS firmware boot and pending checkpoint policies' in policy
    spec = importlib.util.spec_from_file_location('short_turn_layout_fixtures',
                                                  workspace / 'phased-reader/host/fixtures.py')
    fixtures = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixtures)
    dense = out / 'fixtures/dense.epub'
    dense.parent.mkdir(exist_ok=True)
    fixtures.write_run_budget(dense)
    upgrade = run([positive, dense, '--cache-upgrade'])
    assert 'PASS old Pages data misses, saved content resumes, and only Pages rebuilds' in upgrade
    print(policy + upgrade, end='')
    if controls:
        recovery_controls(run, command, dense, workspace, out)


def composition_controls(run, command, epub, workspace, out):
    book = workspace / 'firmware/reader_book.rs'
    driver = workspace / 'firmware/reader_book_run.rs'
    source = driver.read_text()
    # Mutate reached short-route code, without enabling the retired comparison.
    paint = source[source.index('Some(checkpoint.paint('):source.index(')) } else { None };') + 2]
    guard = 'Route::Short => break Some(ReaderUiIntent::Repaint),'
    repaint = source.index('ReaderUiWork::Intent(ReaderUiIntent::Repaint) => {')
    mark = source.index('ReaderUiWork::Intent(ReaderUiIntent::MarkFinished) => {')
    controls = {
        'eager-open': source.replace('Route::Short => paint_first', 'Route::Short => initialize || paint_first', 1),
        'eager-action': source[:repaint] + source[repaint:mark].replace(guard,
            'Route::Short => { composed = ' + paint + '; },', 1) + source[mark:],
        'eager-finished': source[:mark] + source[mark:].replace(guard,
            'Route::Short => { composed = ' + paint + '; },', 1),
    }
    for name, changed in controls.items():
        assert changed != source
        child = out / name
        child.mkdir()
        replacement = child / 'driver.rs'
        replacement.write_text(changed)
        generated_book = child / 'book.rs'
        generated_book.write_text(paths(book.read_text(), book.parent, {driver: replacement}))
        host = child / 'host.rs'
        host.write_text(paths((HERE / 'host.rs').read_text(), HERE, {book: generated_book}))
        run([*command, host, '-o', child / 'program'])
        result = run([child / 'program', epub], succeeds=False)
        assert result.returncode == 101 and ('short open leaves composition' in result.stderr
                or 'short action must not compose' in result.stderr), result.stderr
    print('PASS real firmware short-driver composition; 3 compiled duplicate-composition controls rejected')

def recovery_controls(run, command, dense, workspace, out):
    policy = workspace / 'firmware/reader_restore_policy.rs'
    pages = workspace / 'firmware/reader_pages.rs'
    record = workspace / 'firmware/retained_runtime/record.rs'
    for target, name, original, mutation, arguments, witness in (
        (policy, 'cold-exact',
         '(Some(DurableCheckpoint::PageStart), _) => RestorePolicy::ColdCheckpoint,',
         '(Some(DurableCheckpoint::PageStart), _) => RestorePolicy::ExactPage,',
         ['--restore-policy'], 'startup restore policy'),
        (policy, 'pending-exact',
         'RestorePolicy::Validated | RestorePolicy::ColdCheckpoint => Recovery::PendingAnchor,',
         'RestorePolicy::Validated => Recovery::PendingAnchor,\n'
         '        RestorePolicy::ColdCheckpoint => Recovery::PendingExact,',
         ['--restore-policy'], 'interrupted ColdCheckpoint open'),
        (pages, 'presence-by-name',
         'if resolved { return hint; }',
         'if resolved || hint == MissingProbe::Complete { return hint; }',
         [dense, '--cache-upgrade'], 'old Pages version must be missing for the producer'),
        # The decoding before #42's review: bit 0 alone reads clockwise as counterclockwise.
        (record, 'wake-bit-zero',
         'durable.or_else(|| self.reader_view()).map(|settings| settings.orientation)',
         'durable.map(|settings| settings.orientation).or(Some(if self.reader_settings & 1 != 0 '
         '{ Orientation::LandscapeCcw } else { Orientation::Portrait }))',
         ['--restore-policy'], 'wake diagnostics report retained'),
    ):
        source = target.read_text()
        assert source.count(original) == 1
        child = out / name
        child.mkdir()
        replacement = child / target.name
        replacement.write_text(source.replace(original, mutation, 1))
        host = child / 'host.rs'
        host.write_text(paths((HERE / 'host.rs').read_text(), HERE, {target: replacement}))
        run([*command, host, '-o', child / 'program'])
        run([child / 'program', *arguments], witness=witness)
    print('PASS compiled restore policy, wake orientation and old Pages producer controls rejected')


if __name__ == '__main__':
    main()
