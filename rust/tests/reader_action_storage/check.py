#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the complete firmware action entry with actual revision loans."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

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
    out = args.output.resolve()
    assert out.is_relative_to(ROOT / '.build') and not out.exists()
    out.mkdir(parents=True)
    target = args.target_dir.resolve() if args.target_dir else out / 'target'
    assert target.is_relative_to(ROOT / '.build')
    tools = args.tool_root.resolve() / '.toolchain'
    rustc = tools / 'rust/toolchain/bin/rustc'
    cargo = tools / 'rust/toolchain/bin/cargo'
    clang = tools / 'llvm/bin/clang++'
    env = os.environ.copy()
    env.update(RUSTC=str(rustc), RUSTDOC=str(tools / 'rust/toolchain/bin/rustdoc'),
               CARGO_HOME=str(tools / 'rust/cargo-home'), CARGO_NET_OFFLINE='true',
               CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0')
    commands = []
    def run(command, succeeds=True):
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                                capture_output=True, text=True)
        index = len(commands)
        (out / f'{index:02d}.stdout').write_text(result.stdout)
        (out / f'{index:02d}.stderr').write_text(result.stderr)
        commands.append({'command': list(map(str, command)), 'exit': result.returncode})
        (out / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        if succeeds and result.returncode:
            raise RuntimeError(result.stderr[-6000:])
        return result
    run([cargo, 'build', '--lib', '-p', 'newt-c3-board', '-p', 'newt-device-ui',
         '-p', 'newt-library-current', '--target', HOST, '--manifest-path', ROOT / 'rust/Cargo.toml',
         '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', target,
         '--message-format=json'])
    libraries = {}
    for line in (out / '00.stdout').read_text().splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact':
            for file in item['filenames']:
                if file.endswith('.rlib'):
                    libraries[item['target']['name']] = Path(file)
    deps = target / HOST / 'debug/deps'
    obj = out / 'board.o'
    run([clang, '--no-default-config', '-x', 'c', '-std=c99', '-O1',
         '-I', ROOT / 'rust/salamander/board/native', '-c',
         ROOT / 'rust/tests/reader_checkpoint/board_fake.c', '-o', obj])
    command = [rustc, '--edition=2024', '--target', HOST, '-Copt-level=1',
               '-Clinker=' + str(clang), '-Clink-arg=--no-default-config',
               '-Clink-arg=-fuse-ld=/usr/bin/x86_64-linux-gnu-ld.bfd',
               '-Ldependency=' + str(deps)]
    for name, library in libraries.items():
        command += ['--extern', name + '=' + str(library)]
    command += ['-Clink-arg=' + str(obj), '-Clink-arg=-lcrypto']
    fixture = (HERE / 'host.rs').read_text()
    positive = out / 'host.rs'
    positive.write_text(paths(fixture, HERE))
    run([*command, positive, '-o', out / 'positive'])
    result = run([out / 'positive'])
    assert result.stdout.startswith('PASS whole actual operate_image:')
    operation = ROOT / 'rust/firmware/reader_image_operation.rs'
    source = operation.read_text()
    before = 'session::with_source_lent(state, RevisionPorts'
    after = '}, action_storage, |revision| {'
    assert source.count(before) == 1 and source.count(after) == 1
    compact = source.replace(before, 'session::with_source(state, RevisionPorts').replace(after, '}, |revision| {')
    replacement = out / 'compact-operation.rs'
    replacement.write_text(paths(compact, operation.parent))
    control = out / 'compact-host.rs'
    control.write_text(paths(fixture, HERE, {operation: replacement}))
    run([*command, control, '-o', out / 'compact-control'])
    rejected = run([out / 'compact-control'], succeeds=False)
    assert rejected.returncode == 101 and 'real operate_image must lend whole-block action scratch exactly once' in rejected.stderr, rejected.stderr
    always = source.replace('let action_storage = if initialize || paint_first',
                            'let action_storage = if true || initialize || paint_first', 1)
    assert always != source
    replacement = out / 'idle-lending-operation.rs'
    replacement.write_text(paths(always, operation.parent))
    control = out / 'idle-lending-host.rs'
    control.write_text(paths(fixture, HERE, {operation: replacement}))
    run([*command, control, '-o', out / 'idle-lending-control'])
    rejected = run([out / 'idle-lending-control'], succeeds=False)
    assert rejected.returncode == 101 and 'idle keeps compact scratch' in rejected.stderr, rejected.stderr
    print('PASS actual operate_image action/idle frame loans; compiled compact and idle-lending controls rejected')
    common = [cargo, 'test', '--offline', '--frozen', '--locked',
              '--manifest-path', ROOT / 'rust/Cargo.toml', '--target', HOST,
              '--target-dir', target, '--jobs', '2']
    run([*common, '-p', 'newt-reader-ui', '--test', 'action_storage'])
    run([*common, '-p', 'newt-phased-reader', '--lib', 'cache_scope'])


if __name__ == '__main__':
    main()
