#!/usr/bin/env python3
"""Target-first current-chapter page-cache fill policy, progress outcomes,
failure precedence, and cold-boot wake cache hit over real firmware code."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
HOST = 'x86_64-unknown-linux-gnu'
sys.path.insert(0, str(ROOT / 'tools'))
from gate_tier import negative_controls
from host_common import crypto_flags

# Ordered by crate layer so firmware/ controls reuse the clean library graph
# directly and each library mutation recompiles only its narrow dependent slice.
CONTROLS = (
    ('stale-plus-input-not-terminal', 'firmware/reader_producer.rs',
     '            if !fresh {\n                if !next {\n                    cancellation.fail_terminal();\n',
     '            if !fresh {\n                if !next {\n                    cancellation.result(false);\n',
     'stale_plus_input_denies_immediately',
     'Stale plus input must deny admission immediately'),
    ('resume-input-treated-as-terminal', 'firmware/reader_producer.rs',
     '                } else if !policy.failed() && cancelled() == Stop::Interrupted {\n                    reason = Stop::Interrupted;\n',
     '                } else if false {\n                    reason = Stop::Interrupted;\n',
     'suspend_and_resume_input_stays_interruptible',
     'input observed inside resume_producer_write must stay interruptible'),
    ('final-pause-terminal-ignored', 'firmware/reader_producer.rs',
     '                Some(Stop::Terminal) => {\n                    duty_terminal.set(true);\n                }\n',
     '                Some(Stop::Terminal) => {}\n',
     'final_pause_request_terminal_denies_visit',
     'direct Some(Stop::Terminal) from final pause_request must deny visit'),
    ('deadline-failed-skips-terminal-final-pause', 'firmware/reader_producer.rs',
     '        } else if !next && matches!((callbacks.pause_request)(poll.began, 0), Some(Stop::Terminal)) {\n            duty_terminal.set(true);\n        }\n',
     '        }\n',
     'final_pause_request_terminal_denies_visit',
     'terminal final pause_request after interrupted duty must deny visit'),
    ('target-first-disabled', 'phased-reader/src/producer_segments.rs',
     'segment.min(self.count - 1) as u8', '0',
     'target_first_interrupt_after_first_segment',
     'first sealed segment must be target segment k_reader'),
    ('post-promote-interrupt-spends-try', 'phased-reader/src/producer_store.rs',
     'committed = done || self.base.last_write_published();',
     'committed = done;',
     'target_first_interrupt_after_first_segment',
     'interrupted Progress must not spend a try'),
    ('leftover-returns-unsupported', 'phased-reader/src/producer_stream.rs',
     '        } else if self.state.sealed > 0 {\n            (CurrentOutcome::Progress, Status::Unsupported)\n',
     '        } else if self.state.sealed > 0 {\n            (CurrentOutcome::NoProgress(Status::Unsupported), Status::Unsupported)\n',
     'target_first_leftover_progress_and_cold_boot_hit',
     'leftover segments after sealing must return Progress, not deny admission'),
    ('crowded-dir-ignores-grant', 'phased-reader/src/producer_segments.rs',
     '            if !store.producer_can_begin() { self.skipped = self.skipped.min(step); continue; }\n',
     '            if false { self.skipped = self.skipped.min(step); continue; }\n',
     'crowded_pages_dir_fills_across_four_entry_grants',
     'crowded 4-entry grant must return Progress before opening 5th file'),
    ('map-post-promote-interrupt-spends-try', 'phased-reader/src/producer_stream.rs',
     '            if self.store.end_committed() {\n                (CurrentOutcome::Done, status)\n            } else if self.state.sealed > 0 {\n',
     '            if false {\n                (CurrentOutcome::Done, status)\n            } else if self.state.sealed > 0 {\n',
     'post_rename_interrupt_on_third_try_completes_whole_and_map_chapters',
     'post-rename interrupt on published segment map must complete and advance to prefetch'),
    ('suspend-keeps-held-read', 'native-storage/src/chapter_cache.rs',
     '        if self.state.read_active && !self.file.writing() {\n            self.close();\n            self.file.mark_read_suspended();\n        }\n',
     '',
     'duty_due_while_read_held_completes',
     'a duty due while a read is held must not end the fill'),
    ('cleanup-unlink-fault-suppressed', 'native-storage/src/port.rs',
     '            if !ok {\n                self.producer_io_failure();\n                self.port.state.failed = true\n            }',
     '            if !ok {\n                self.producer_failure();\n                self.port.state.failed = true\n            }',
     'cleanup_unlink_fault_during_interrupt_denies_immediately',
     'cleanup unlink I/O fault during interrupt must deny admission immediately'),
    ('progress-treated-as-done', 'core/src/producer_visit.rs',
     '            CurrentOutcome::Progress => {\n                self.pending(position, eligible);\n            }\n',
     '            CurrentOutcome::Progress => {\n'
     '                self.complete(position, target, false);\n'
     '                self.count_try(position, true, target);\n'
     '            }\n',
     'target_first_leftover_progress_and_cold_boot_hit',
     'Progress must keep target on current spine'),
    ('progress-skips-pending-rearm', 'core/src/producer_visit.rs',
     '            CurrentOutcome::Progress => {\n                self.pending(position, eligible);\n            }\n',
     '            CurrentOutcome::Progress => {}\n',
     'target_first_leftover_progress_and_cold_boot_hit',
     'Progress must re-arm admission Pending'),
    ('progress-skips-quiet-window', 'core/src/producer_visit.rs',
     '            Self::Progress => Some(now()),\n',
     '            Self::Progress => None,\n',
     'target_first_leftover_progress_and_cold_boot_hit',
     'Progress must renew quiet window'),
)

UNIT_CONTROL = (
    'pause-terminal-does-not-override-interrupted',
    'core/src/producer_policy.rs',
    '        if reason == Stop::Terminal || self.reason.get() == Stop::None {\n',
    '        if self.reason.get() == Stop::None {\n',
    'producer_policy::tests::terminal_failures_override_interrupted_monotonically',
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_synthetic_epub(path: Path):
    """Build a synthetic CC0 multi-chapter EPUB:
    - spine 0: ~430 pages (4 cache segments: 0..3) of ordinary prose
    - spine 1: ~20 pages (short non-segmented chapter)
    - spine 2: ~10 pages (next prefetch target after spine 1)
    """
    sentence = (
        'A lantern glowed beside the quiet harbor road while the tide drew '
        'back across the stones and gulls circled above the wooden pier. '
    )
    # Prose covers segments 0..2 (~360 pages including target page 290); compact
    # rule-separated lines cross the page-384 segment boundary within one XML
    # chunk so pass 0 seals segment 2, skips segment 3 to pass 1, and leaves
    # segments 0..1 for the next attempt (Progress -> Done).
    long_paragraphs = ''.join(
        f'<p>Section {i}: {sentence * 12}</p>\n' for i in range(1, 146)
    ) + ''.join('H<hr/>' for _ in range(1000))
    short_paragraphs = ''.join(
        f'<p>Short chapter paragraph {i}: {sentence * 3}</p>\n' for i in range(1, 28)
    )
    tail_paragraphs = ''.join(
        f'<p>Closing note {i}: {sentence * 2}</p>\n' for i in range(1, 14)
    )
    five_seg_paragraphs = ''.join(
        f'<p>Crowded chapter {i}: {sentence * 12}</p>\n' for i in range(1, 229)
    )
    with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_STORED) as zf:
        zf.writestr('mimetype', 'application/epub+zip')
        zf.writestr(
            'META-INF/container.xml',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            '<rootfiles><rootfile full-path="OEBPS/content.opf" '
            'media-type="application/oebps-package+xml"/></rootfiles></container>',
        )
        zf.writestr(
            'OEBPS/content.opf',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">'
            '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
            '<dc:title>Harbor Lantern Synthetic</dc:title>'
            '<dc:language>en</dc:language>'
            '<dc:identifier id="id">urn:uuid:00000000-0000-4000-8000-000000000168</dc:identifier>'
            '</metadata>'
            '<manifest>'
            '<item id="c0" href="ch0.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>'
            '<item id="c3" href="ch3.xhtml" media-type="application/xhtml+xml"/>'
            '</manifest>'
            '<spine>'
            '<itemref idref="c0"/><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/>'
            '</spine></package>',
        )
        zf.writestr(
            'OEBPS/ch0.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter One</h1>\n'
            + long_paragraphs + '</body></html>',
        )
        zf.writestr(
            'OEBPS/ch1.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter Two</h1>\n'
            + short_paragraphs + '</body></html>',
        )
        zf.writestr(
            'OEBPS/ch2.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter Three</h1>\n'
            + tail_paragraphs + '</body></html>',
        )
        zf.writestr(
            'OEBPS/ch3.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Chapter Four</h1>\n'
            + five_seg_paragraphs + '</body></html>',
        )


def build_libraries(run, workspace, target):
    wire = run.cargo(
        workspace, target, 'build', '--lib',
        '-p', 'newt-c3-board', '-p', 'newt-core', '-p', 'newt-native-storage',
        '-p', 'newt-phased-reader', '-p', 'newt-phased-sink',
        '-p', 'newt-reader-ui', '-p', 'newt-target-expat',
        '--features', 'newt-reader-ui/native-host',
        '--target', HOST, '--message-format=json',
    )
    libraries = {}
    for line in wire.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact':
            for file in item['filenames']:
                if file.endswith('.rlib'):
                    libraries[item['target']['name']] = Path(file)
    return libraries


def compile_host(run, workspace, target, libraries, objects, output_binary, *, opt_level, warnings_as_errors):
    clang = run.tools['clang++']
    deps = target / HOST / 'debug/deps'
    command = [
        run.tools['rustc'], '--edition=2024', '--target', HOST, f'-Copt-level={opt_level}',
        *(('-Dwarnings',) if warnings_as_errors else ()),
        '-Clinker=' + str(clang), '-Clink-arg=--no-default-config',
        '-Clink-arg=--ld-path=/usr/bin/x86_64-linux-gnu-ld.bfd',
        '-Ldependency=' + str(deps),
    ]
    for name, library in libraries.items():
        command += ['--extern', name + '=' + str(library)]
    command += ['-Clink-arg=' + str(obj) for obj in objects]
    command += [
        '-Clink-arg=-fsanitize=address,undefined',
        '-Clink-arg=-fno-sanitize-recover=all',
        *['-Clink-arg=' + arg for arg in crypto_flags()[2:]],
        workspace / 'tests/producer_fill_policy/host.rs', '-o', output_binary,
    ]
    run.run(command, workspace)


def main():
    support = load('fill_policy_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/producer-fill-policy'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 480)
        # Keep RUSTFLAGS constant across positive and mutated builds so Cargo
        # never invalidates untouched crate fingerprints; rust/check.py --landing
        # already compiles the workspace with -Dwarnings.
        target = output / 'target'
        epub = output / 'synthetic.epub'
        write_synthetic_epub(epub)
        try:
            native_pins = load('fill_policy_native', workspace / 'tests/phased_reader/native.py')
            native_pins.verify_native_inputs(support, ROOT / 'vendor/freeink-sdk')
            sdk = ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
            include = [value for path in [sdk / 'include', sdk / 'third_party/expat',
                       sdk / 'third_party/miniz', sdk / 'third_party/libunibreak']
                       for value in ('-I', path)]
            objects = []
            clang = run.tools['clang++']
            for name in ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge', 'board'):
                source = (workspace / 'target-expat/native/expat_bridge.c' if name == 'bridge'
                          else workspace / 'tests/reader_checkpoint/board_fake.c' if name == 'board'
                          else sdk / 'src/vendor' / (name + '.c'))
                obj = output / (name + '.o')
                run.run([clang, '--no-default-config', '-x', 'c', '-std=c99', '-O1', '-g',
                         '-fsanitize=address,undefined', '-fno-sanitize-recover=all', *include,
                         '-I', workspace / 'salamander/board/native', '-c', source, '-o', obj], workspace)
                objects.append(obj)

            libraries = build_libraries(run, workspace, target)
            positive = output / 'positive'
            compile_host(run, workspace, target, libraries, objects, positive,
                         opt_level=1, warnings_as_errors=False)
            stdout = run.run([positive, epub], workspace)
            support.require('PASS producer fill policy (15 cases)' in stdout,
                            'all 15 fill-policy and cold-boot cache hit cases must pass')
            print(stdout, end='', flush=True)

            if negative_controls(__file__):
                for name, rel_source, good, bad, case, witness in CONTROLS:
                    path = workspace / rel_source
                    original = path.read_text()
                    support.require(original.count(good) == 1,
                                    f'control "{name}": anchor must appear once in rust/{rel_source}')
                    path.write_text(original.replace(good, bad, 1))
                    try:
                        # firmware/ files are #[path]-included by host.rs, never compiled
                        # into the Cargo library graph, so they reuse clean libraries directly.
                        mut_libs = libraries if rel_source.startswith('firmware/') else build_libraries(run, workspace, target)
                        binary = output / name
                        compile_host(run, workspace, target, mut_libs, objects, binary,
                                     opt_level=0, warnings_as_errors=False)
                        number = len(run.commands)
                        try:
                            run.run([binary, epub, '--case', case], workspace)
                        except RuntimeError:
                            stderr = (run.output / f'{number:03d}.stderr').read_text()
                            support.require('panicked at ' in stderr and witness in stderr,
                                            f'control "{name}": expected witness "{witness}" in stderr:\n{stderr}')
                        else:
                            raise RuntimeError(f'control "{name}": {case} passed with rule broken')
                    finally:
                        path.write_text(original)
                    print(f'PASS control: {name} rejected by {case}', flush=True)

                # Unit negative control for pause_point_with Stop::Terminal override.
                name, rel_source, good, bad, unit_test = UNIT_CONTROL
                path = workspace / rel_source
                original = path.read_text()
                support.require(original.count(good) == 1,
                                f'control "{name}": anchor must appear once in rust/{rel_source}')
                path.write_text(original.replace(good, bad, 1))
                try:
                    wire = run.cargo(workspace, target, 'test', '-p', 'newt-core', '--lib',
                                     '--target', HOST, '--no-run', '--message-format=json')
                    bins = [row['executable'] for row in map(json.loads, wire.splitlines())
                            if row.get('reason') == 'compiler-artifact' and row.get('executable')]
                    support.require(len(bins) == 1, 'one newt-core test binary expected')
                    number = len(run.commands)
                    try:
                        run.run([bins[0], unit_test, '--exact'], workspace)
                    except RuntimeError:
                        stdout = (run.output / f'{number:03d}.stdout').read_text()
                        support.require('test result: FAILED. 0 passed; 1 failed' in stdout
                                        and 'panicked at' in stdout,
                                        f'control "{name}": {unit_test} must fail on an assertion')
                    else:
                        raise RuntimeError(f'control "{name}": {unit_test} passed with rule broken')
                finally:
                    path.write_text(original)
                print(f'PASS control: {name} rejected by {unit_test}', flush=True)
        except RuntimeError:
            for log in sorted((output / 'logs').glob('*.std*'))[-2:]:
                print(log.read_text(), file=sys.stderr, end='')
            raise


if __name__ == '__main__':
    main()
