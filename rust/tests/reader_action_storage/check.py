#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the complete firmware action entry with actual revision loans."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import zipfile

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
        if Path(command[0]).name in {'cargo', 'rustc', 'clang++'}:
            subprocess.run(['free', '-g'], check=True)
            subprocess.run(['df', '-h', '.'], cwd=ROOT, check=True)
            available = int(next(line.split()[1] for line in Path('/proc/meminfo').read_text().splitlines()
                                 if line.startswith('MemAvailable:')))
            assert available >= 8 * 1024 * 1024, 'build requires 8 GiB available'
            disk = os.statvfs(ROOT)
            assert (disk.f_blocks - disk.f_bfree) / disk.f_blocks < .8, 'build requires disk below 80%'
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
         '-p', 'newt-library-current', '--features', 'newt-phased-reader/dev-usb', '--target', HOST, '--manifest-path', ROOT / 'rust/Cargo.toml',
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
    book = out / 'generated.epub'
    with zipfile.ZipFile(book, 'w', compression=zipfile.ZIP_STORED) as archive:
        archive.writestr('META-INF/container.xml', '<container><rootfiles><rootfile full-path="package.opf"/></rootfiles></container>')
        archive.writestr('package.opf', '<package><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>')
        archive.writestr('chapter.xhtml', '<html><body><p>' + 'generated ' * 1500 + '</p></body></html>')
    result = run([out / 'positive', book, 'synthetic.epub'])
    run([out / 'positive', book, ':Shelf:Nested:synthetic.epub'])
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
    rejected = run([out / 'compact-control', book, 'synthetic.epub'], succeeds=False)
    assert rejected.returncode == 101 and 'real operate_image must lend whole-block action scratch exactly once' in rejected.stderr, rejected.stderr
    always = source.replace('let action_storage = if initialize || paint_first',
                            'let action_storage = if true || initialize || paint_first', 1)
    assert always != source
    replacement = out / 'idle-lending-operation.rs'
    replacement.write_text(paths(always, operation.parent))
    control = out / 'idle-lending-host.rs'
    control.write_text(paths(fixture, HERE, {operation: replacement}))
    run([*command, control, '-o', out / 'idle-lending-control'])
    rejected = run([out / 'idle-lending-control', book, 'synthetic.epub'], succeeds=False)
    assert rejected.returncode == 101 and 'idle keeps compact scratch' in rejected.stderr, rejected.stderr
    loans = ROOT / 'rust/firmware/reader_image_sources.rs'
    drift = loans.read_text().replace('reader_book::CACHE_ROOT', '"/newt-sd/Newt/OtherCache"')
    assert drift != loans.read_text()
    replacement = out / 'wrong-route-sources.rs'
    replacement.write_text(paths(drift, loans.parent))
    control = out / 'wrong-route-host.rs'
    control.write_text(paths(fixture, HERE, {loans: replacement}))
    run([*command, control, '-o', out / 'wrong-route-control'])
    rejected = run([out / 'wrong-route-control', book, 'synthetic.epub'], succeeds=False)
    assert rejected.returncode == 101 and 'index and operate_image must lend the same cache route' in rejected.stderr, rejected.stderr
    invalidation = ROOT / 'rust/firmware/reader_index_invalidation.rs'
    keep = invalidation.read_text().replace('pages.invalidate_index(digest)', 'true')
    assert keep != invalidation.read_text()
    replacement = out / 'keep-index.rs'
    replacement.write_text(paths(keep, invalidation.parent))
    control = out / 'keep-index-host.rs'
    control.write_text(paths(fixture, HERE, {invalidation: replacement}))
    run([*command, control, '-o', out / 'keep-index-control'])
    rejected = run([out / 'keep-index-control', book, 'synthetic.epub'], succeeds=False)
    assert rejected.returncode == 101 and 'failed verification must remove the derived index' in rejected.stderr, rejected.stderr
    reopen = ROOT / 'rust/firmware/reader_index_reopen.rs'
    controls = [
        ('skip-prepare-hash', ROOT / 'rust/firmware/reader_standalone_open.rs',
         'let hashed = unsafe { reader_hash::hash_source(raw, &mut digest, &mut || yielded(), observer) };',
         'digest=expected.unwrap_or_default(); let hashed=true;',
         'strict prepare hash must reject changed bytes'),
        ('skip-session-reopened-verification', reopen,
         'Some(verification) => restart_verification(verification, raw)', 'Some(verification) => true',
         'session reopen must restart verification'),
        ('skip-reopened-verification', reopen,
         'None => verify_standalone_reopen(raw, digest, yielded)', 'None => true',
         'changed reopened bytes must fail verification'),
        ('trust-orphan', reopen,
         'pages.0.invalidate_index(digest);', '/* retain orphan */',
         'failed or missing reopened source must revoke its index'),
        ('selected-after-spill-reopen', operation,
         'None, run_turn(prepared), &yielded', 'None, run_turn(false), &yielded',
         'a spill-reopened first open must finish the prepared import'),
        ('stale-published-digest', ROOT / 'rust/phased-reader/src/index_build.rs',
         'Identity { digest: *digest, policy:', 'Identity { digest: [0;32], policy:',
         'publication must bind the bytes digest'),
    ]
    for name, source_file, before, after, witness in controls:
        source_text = source_file.read_text()
        assert before in source_text
        if name == 'stale-published-digest':
            assert source_text.count(before) == 1
            mutant = out / 'mutant-rust'
            shutil.copytree(ROOT / 'rust', mutant, ignore=shutil.ignore_patterns('target', '__pycache__'))
            (mutant / source_file.relative_to(ROOT / 'rust')).write_text(source_text.replace(before, after, 1))
            # The mutant's rlibs share names with the real ones, so a shared
            # target dir would feed them to the next run's positive build.
            mutant_target = target / 'stale-digest-mutant'
            wire = run([cargo, 'build', '--lib', '-p', 'newt-c3-board', '-p', 'newt-device-ui',
                        '-p', 'newt-library-current', '--features', 'newt-phased-reader/dev-usb',
                        '--target', HOST, '--manifest-path', mutant / 'Cargo.toml',
                        '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', mutant_target,
                        '--message-format=json'])
            mutant_libraries = {}
            for line in wire.stdout.splitlines():
                item = json.loads(line)
                if item.get('reason') == 'compiler-artifact':
                    for file in item['filenames']:
                        if file.endswith('.rlib'):
                            mutant_libraries[item['target']['name']] = Path(file)
            mutant_command = list(command)
            for i, arg in enumerate(mutant_command):
                if arg == '-Ldependency=' + str(deps):
                    mutant_command[i] = '-Ldependency=' + str(mutant_target / HOST / 'debug/deps')
                elif i and mutant_command[i - 1] == '--extern':
                    package = arg.split('=', 1)[0]
                    mutant_command[i] = package + '=' + str(mutant_libraries[package])
            run([*mutant_command, positive, '-o', out / name])
            rejected = run([out / name, book, 'synthetic.epub'], succeeds=False)
        else:
            replacement = out / (name + '.rs')
            replacement.write_text(paths(source_text.replace(before, after, 1), source_file.parent))
            control = out / (name + '-host.rs')
            control.write_text(paths(fixture, HERE, {source_file: replacement}))
            run([*command, control, '-o', out / name])
            rejected = run([out / name, book, 'synthetic.epub'], succeeds=False)
        assert rejected.returncode == 101 and witness in rejected.stderr, rejected.stderr
    print('PASS changed reopened bytes, byte-digest publication and missing-source revocation; skip-verification, stale-digest and trusted-orphan controls rejected')
    print('PASS actual operation/index loans, physical spill release/reopen, reopen refusal, power-cut cold reads and verification revocation; compact, idle-lending, route-drift and retained-index controls rejected')
    print('PASS spill-reopened first open opens Prepared; selected-after-spill-reopen control rejected')
    common = [cargo, 'test', '--offline', '--frozen', '--locked',
              '--manifest-path', ROOT / 'rust/Cargo.toml', '--target', HOST,
              '--target-dir', target, '--jobs', '2']
    run([*common, '-p', 'newt-reader-ui', '--test', 'action_storage'])
    run([*common, '-p', 'newt-phased-reader', '--lib', 'cache_scope'])


if __name__ == '__main__':
    main()
