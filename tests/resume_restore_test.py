#!/usr/bin/env python3
"""End-to-end resume-point restore verification over real Expat, libunibreak,
PhasedReader, Producer, ChapterCacheStorage and VerifiedStore, plus negative controls."""
import importlib.util
import json
from pathlib import Path
import shutil
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

CONTROLS = (
    ('skip-pass-b-crc', 'phased-reader/src/resume_restore.rs',
     '    if !point_in.finish_crc(verified.crc) {\n        return None;\n    }\n',
     '    if false && !point_in.finish_crc(verified.crc) {\n        return None;\n    }\n',
     'pass_b_tamper_and_replacement',
     'Pass B CRC must reject tampered replay byte'),
    ('no-shift-translation', 'phased-reader/src/resume_restore.rs',
     '    session.stream.set_restored(shift, prefix.replay_len);\n',
     '    session.stream.set_restored(0, prefix.replay_len);\n',
     'restore_equals_byte0_oracle',
     'outcome must match byte-0 oracle'),
    ('skip-prefix-start-collection', 'phased-reader/src/producer_stream.rs',
     '        if self.writer.restore_points && !self.writer.starts_complete(self.state.count) {\n',
     '        if false && self.writer.restore_points && !self.writer.starts_complete(self.state.count) {\n',
     'missing_map_and_corrupt_prefix_withholding',
     'verified prefix-start collection must complete map publication'),
    ('withhold-bypassed-on-corrupt-prefix', 'phased-reader/src/producer_stream.rs',
     '                Ok(crate::resume_restore::CollectStarts::Defect) => {\n                    // An existing on-card segment failed header/envelope certification.\n                    // Never fall back to byte-0 counting to publish a map over it.\n                    if self.state.sealed > 0 {\n                        return (CurrentOutcome::Progress, Status::Stale);\n                    }\n                    self.writer.cancellation().fail_terminal();\n                    return (CurrentOutcome::NoProgress(Status::Stale), Status::Stale);\n                }\n',
     '                Ok(crate::resume_restore::CollectStarts::Defect) => {\n                    if false && self.state.sealed > 0 {\n                        return (CurrentOutcome::Progress, Status::Stale);\n                    }\n                }\n',
     'missing_map_and_corrupt_prefix_withholding',
     'corrupt prefix segment must withhold map publication as Progress'),
    ('skip-certified-seal-only-starts', 'phased-reader/src/producer_writer.rs',
     '        if self.segmented\n            && !self.restore_points\n            && self.seen % PAGES == 0\n            && (self.seen / PAGES) < MAX_SEGMENTS as u32\n        {\n',
     '        if self.segmented\n            && self.seen % PAGES == 0\n            && (self.seen / PAGES) < MAX_SEGMENTS as u32\n        {\n',
     'missing_map_and_corrupt_prefix_withholding',
     'skipped corrupt middle segment must still be verified and withhold map as Progress'),
    ('skip-engine-utf8-check', 'core/src/chapter_layout/engine/resume.rs',
     '        if core::str::from_utf8(&s.text[..len]).is_err() {\n            return false;\n        }\n',
     '        if false && core::str::from_utf8(&s.text[..len]).is_err() {\n            return false;\n        }\n',
     'adversarial_valid_crc_points',
     'index out of bounds'),
    ('skip-names-match-check', 'phased-reader/src/resume_restore.rs',
     '        if !sec.finish()\n            || !engine.matches_open_names(names_depth, names_suppress, names_style, names_body)\n        {\n',
     '        if !sec.finish() {\n',
     'adversarial_valid_crc_points',
     'engine depth mismatches Names stack'),
    ('skip-restore-receipt-check', 'phased-reader/src/resume_restore.rs',
     '    if guarded {\n        let live = source.with_checks(source_bytes, false, |check| check(ProducerCheck::Receipt));\n',
     '    if false && guarded {\n        let live = source.with_checks(source_bytes, false, |check| check(ProducerCheck::Receipt));\n',
     'real_cancel_duty_terminal_and_stale_receipt',
     'stale_rcpt'),
    ('skip-restored-prolog-utf8-gate', 'phased-reader/src/resume_restore.rs',
     '            if prolog_len > 0 && validate_utf8_prolog(&p[..prolog_len.min(4)], p).is_err() {\n                return false;\n            }\n',
     '            if false && prolog_len > 0 && validate_utf8_prolog(&p[..prolog_len.min(4)], p).is_err() {\n                return false;\n            }\n',
     'adversarial_valid_crc_points',
     'malformed point (forged prolog with latin1 xml decl) must reject locally'),
    ('skip-clear-starts-after-settle', 'phased-reader/src/producer_current.rs',
     '                    attempt.writer.clear_starts();\n',
     '',
     'missing_map_and_corrupt_prefix_withholding',
     'unsettled style with corrupt prefix must withhold map as Progress'),
    ('skip-map-duty-retry', 'phased-reader/src/resume_restore.rs',
     '            if store.envelope_read_suspended() {\n',
     '            if false && store.envelope_read_suspended() {\n',
     'missing_map_and_corrupt_prefix_withholding',
     'map-only collection must survive duty suspension and finish Done'),
    ('skip-point-owner-pages-at-check', 'phased-reader/src/resume_restore.rs',
     '        || binding.pages_at != expected_pages_at\n',
     '',
     'adversarial_valid_crc_points',
     'binding + engine pages_at disagrees with point_owner'),
    ('skip-map-suspended-interrupt', 'phased-reader/src/producer_stream.rs',
     '                    self.writer.cancellation().interrupt();\n',
     '',
     'missing_map_and_corrupt_prefix_withholding',
     'exhausted map-only duty suspensions must stay interrupted, never terminal'),
    ('skip-discard-loop-poll', 'phased-reader/src/producer_stream.rs',
     '                ch.discard_to(source_consumed, scratch, &mut || store.pause_point())\n',
     '                ch.discard_to(source_consumed, scratch, &mut || true)\n',
     'peak_open_files_with_duty',
     'Discard duty must fire inside discard_to loop between source reads'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_epub(path: Path, chapters: list[tuple[str, bytes]], compression=zipfile.ZIP_STORED):
    manifest = ''.join(
        f'<item id="c{i}" href="{href}" media-type="application/xhtml+xml"/>'
        for i, (href, _) in enumerate(chapters)
    )
    spine = ''.join(f'<itemref idref="c{i}"/>' for i in range(len(chapters)))
    with zipfile.ZipFile(path, 'w', compression=compression) as zf:
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
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
            '<dc:title>Resume Restore Synthetic</dc:title>'
            '<dc:language>en</dc:language>'
            '<dc:identifier id="id">urn:uuid:00000000-0000-4000-8000-000000000213</dc:identifier>'
            '</metadata>'
            f'<manifest>{manifest}</manifest>'
            f'<spine>{spine}</spine></package>',
        )
        for href, payload in chapters:
            zf.writestr('OEBPS/' + href, payload, compress_type=compression)


def make_fixtures(directory: Path):
    sentence = (
        '<span class="h">A lantern glowed beside the quiet harbor road &mdash; </span>'
        '<span class="t">while the tide drew back across the stones and gulls circled above the wooden pier. </span>'
    )
    long_paras = ''.join(
        f'<p id="p{i}">Section {i}: <a href="ch1.xhtml#n1">harbor</a> {sentence * 11}</p>\n'
        for i in range(1, 185)
    )
    short_paras = ''.join(
        f'<p id="n{i}">Short note {i}: {sentence * 3}</p>\n' for i in range(1, 22)
    )
    ch0 = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Long Chapter</h1>\n'
        + long_paras + '</body></html>'
    ).encode('utf-8')
    ch1 = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Short Chapter</h1>\n'
        + short_paras + '</body></html>'
    ).encode('utf-8')
    vocab = (
        'harbor', 'lantern', 'stone', 'wooden', 'pier', 'channel', 'anchor', 'current',
        'beacon', 'drift', 'shingle', 'jetty', 'kelp', 'foghorn', 'compass', 'mooring',
        'sloop', 'tackle', 'ballast', 'capstan', 'dory', 'estuary', 'fathom', 'gunwale',
        'halyard', 'inlet', 'keel', 'leeward', 'mainsail', 'nautical', 'oarlock', 'pennant',
        'quarter', 'rigging', 'schooner', 'transom', 'undertow', 'vessel', 'windward', 'yardarm',
        'albatross', 'breezes', 'cormorant', 'dolphins', 'eiders', 'flounder', 'gannets', 'herons',
        'islets', 'kestrels', 'limpets', 'mussels', 'narwhals', 'ospreys', 'pelicans', 'quahogs',
        'razorbills', 'sandpipers', 'terns', 'urchins', 'whelks', 'yellowlegs', 'barnacles', 'clams',
    )
    seed = 0x12345678
    replay_paras_list = []
    for i in range(1, 320):
        parts = []
        for j in range(16):
            seed = (seed * 1664525 + 1013904223) & 0xffffffff
            w1 = vocab[(seed >> 16) & 63]
            w2 = vocab[(seed >> 22) & 63]
            w3 = vocab[(seed >> 10) & 63]
            w4 = vocab[(seed >> 4) & 63]
            parts.append(
                f'<span class="replay-harbor-{i}-{j}" data-token="{seed:08x}-lantern-tide-pier-{i:03d}-{j:02d}">'
                f'<a href="ch1.xhtml#n1">{w1}</a>&nbsp;&mdash;&nbsp;{w2}&hellip; {w3} stones and {w4} pier. </span>'
            )
        replay_paras_list.append(f'<p id="r{i}">' + ''.join(parts) + '</p>\n')
    replay_paras = ''.join(replay_paras_list)
    bq_open = '<blockquote>' * 16
    bq_close = '</blockquote>' * 16
    ch2_replay = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!-- ' + ('R' * 450) + ' -->\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body>'
        + bq_open + '<h1>Replay &amp; Multi-Window</h1>\n'
        + replay_paras + bq_close + '</body></html>'
    ).encode('utf-8')
    write_epub(directory / 'stored.epub', [('ch0.xhtml', ch0), ('ch1.xhtml', ch1), ('ch2.xhtml', ch2_replay)], zipfile.ZIP_STORED)
    write_epub(directory / 'deflated.epub', [('ch0.xhtml', ch0), ('ch1.xhtml', ch1), ('ch2.xhtml', ch2_replay)], zipfile.ZIP_DEFLATED)

    # 5+ segment chapter (~620 pages) for D3 target-first + hole-skipping tests.
    five_paras = ''.join(
        f'<p id="f{i}">Chapter section {i}: <a href="ch1.xhtml#n1">harbor</a> {sentence * 11}</p>\n'
        for i in range(1, 320)
    )
    ch_five = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Five Segments</h1>\n'
        + five_paras + '</body></html>'
    ).encode('utf-8')
    write_epub(directory / 'five_seg.epub', [('five.xhtml', ch_five), ('ch1.xhtml', ch1)], zipfile.ZIP_STORED)

    # UTF-8 BOM + non-ASCII element name (~320 pages).
    plain_sentence = (
        'A lantern glowed beside the quiet harbor road while the tide drew '
        'back across the stones and gulls circled above the wooden pier. '
    )
    enc_body = ''.join(
        f'<p>Paragraph {i}: {plain_sentence * 12}</p>\n' for i in range(1, 145)
    )
    utf8_bom_doc = (
        b'\xef\xbb\xbf'
        + (
            '<?xml version="1.0" encoding = "UTF-8"?>\n'
            '<café xmlns="http://www.w3.org/1999/xhtml"><body><h1>Café</h1>\n'
            + enc_body + '</body></café>'
        ).encode('utf-8')
    )
    write_epub(directory / 'utf8bom.epub', [('utf8bom.xhtml', utf8_bom_doc)], zipfile.ZIP_STORED)


def build_libraries(run, workspace, target):
    wire = run.cargo(
        workspace, target, 'build', '--lib',
        '-p', 'newt-c3-board', '-p', 'newt-core', '-p', 'newt-native-io',
        '-p', 'newt-native-storage', '-p', 'newt-phased-reader',
        '-p', 'newt-phased-sink', '-p', 'newt-reader-atlas', '-p', 'newt-target-expat',
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


def compile_c(run, workspace, output, sdk, name):
    clang = run.tools['clang++']
    include = [
        value for path in [
            sdk / 'include', sdk / 'third_party/expat',
            sdk / 'third_party/miniz', sdk / 'third_party/libunibreak',
        ] for value in ('-I', path)
    ]
    source = (
        workspace / 'target-expat/native/expat_bridge.c' if name == 'bridge'
        else workspace / 'tests/reader_checkpoint/board_fake.c' if name == 'board'
        else sdk / 'src/vendor' / (name + '.c')
    )
    obj = output / (name + '.o')
    run.run([
        clang, '--no-default-config', '-x', 'c', '-std=c99', '-O1', '-g',
        '-fsanitize=address,undefined', '-fno-sanitize-recover=all', *include,
        '-I', workspace / 'salamander/board/native', '-c', source, '-o', obj,
    ], workspace)
    return obj


def compile_host(run, workspace, target, libraries, objects, output_binary, *, opt_level):
    clang = run.tools['clang++']
    deps = target / HOST / 'debug/deps'
    command = [
        run.tools['rustc'], '--edition=2024', '--target', HOST, f'-Copt-level={opt_level}',
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
        workspace / 'tests/resume_restore/host.rs', '-o', output_binary,
    ]
    run.run(command, workspace)


def main():
    support = load('resume_restore_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/resume-restore'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        fixtures = output / 'fixtures'
        fixtures.mkdir()
        make_fixtures(fixtures)
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 480)
        target = output / 'target'
        try:
            native_pins = load('resume_restore_native', workspace / 'tests/phased_reader/native.py')
            native_pins.verify_native_inputs(support, ROOT / 'vendor/freeink-sdk')
            sdk = ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
            obj_names = ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge', 'board')
            objects = {name: compile_c(run, workspace, output, sdk, name) for name in obj_names}

            libraries = build_libraries(run, workspace, target)
            positive = output / 'positive'
            compile_host(run, workspace, target, libraries, list(objects.values()), positive, opt_level=1)
            stdout = run.run([positive, fixtures], workspace)
            support.require('PASS resume restore (10 cases)' in stdout,
                            f'all 10 resume restore cases must pass:\n{stdout}')
            print(stdout, end='', flush=True)

            if negative_controls(__file__):
                for name, rel_source, good, bad, case, witness in CONTROLS:
                    path = workspace / rel_source
                    original = path.read_text()
                    support.require(original.count(good) == 1,
                                    f'control "{name}": anchor must appear once in rust/{rel_source}')
                    path.write_text(original.replace(good, bad, 1))
                    try:
                        mut_libs = build_libraries(run, workspace, target)
                        binary = output / name
                        compile_host(run, workspace, target, mut_libs, list(objects.values()), binary, opt_level=0)
                        number = len(run.commands)
                        try:
                            run.run([binary, fixtures, '--case', case], workspace)
                        except RuntimeError:
                            stderr = (run.output / f'{number:03d}.stderr').read_text()
                            support.require('panicked at ' in stderr and witness in stderr,
                                            f'control "{name}": expected witness "{witness}" in stderr:\n{stderr}')
                        else:
                            raise RuntimeError(f'control "{name}": {case} passed with rule broken')
                    finally:
                        path.write_text(original)
                    print(f'PASS control: {name} rejected by {case}', flush=True)
        except RuntimeError:
            for log in sorted((output / 'logs').glob('*.std*'))[-2:]:
                print(log.read_text(), file=sys.stderr, end='')
            raise


if __name__ == '__main__':
    main()
