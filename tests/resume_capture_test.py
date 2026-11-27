#!/usr/bin/env python3
"""End-to-end resume-point capture verification over real Expat, libunibreak,
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
    ('non-utf8-decl-ignored', 'core/src/resume_point.rs',
     '                if !enc.eq_ignore_ascii_case(b"utf-8") {\n                    return Err(Decline::Encoding);\n                }\n',
     '                if false && !enc.eq_ignore_ascii_case(b"utf-8") {\n                    return Err(Decline::Encoding);\n                }\n',
     'known_utf8_encoding_gate',
     'spaced ISO-8859-1 XMLDecl must decline capture'),
    ('budget-preflight-bypassed', 'phased-reader/src/resume_capture.rs',
     '    if !preflight_fit(\n        writer.cache.offset(),\n        writer.cache.page_count(),\n        total_bytes,\n        credit,\n    ) {\n',
     '    if false && !preflight_fit(\n        writer.cache.offset(),\n        writer.cache.page_count(),\n        total_bytes,\n        credit,\n    ) {\n',
     'tight_credit_declines_budget',
     'enforced marginal credit must still seal page-only v4 segment'),
    ('edge-done-marked-on-refusal', 'phased-reader/src/resume_capture.rs',
     '    if let CaptureOutcome::Captured(done) = outcome {\n        capture.tap.edge_done = done;\n    }\n',
     '    capture.tap.edge_done = edge;\n',
     'refusal_then_eligibility_in_window',
     'refusal inside [edge-4, edge) must not burn edge_done'),
)

UNIT_CONTROL = (
    'dead-pool-not-zeroed',
    'core/src/chapter_layout/engine/resume.rs',
    '*dst = if live { src } else { 0 };',
    '*dst = src;',
    'chapter_layout::engine::resume::tests::save_resume_zeros_dead_pool_bytes_and_declines_embedded_css_or_stopped',
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
            '<dc:title>Resume Capture Synthetic</dc:title>'
            '<dc:language>en</dc:language>'
            '<dc:identifier id="id">urn:uuid:00000000-0000-4000-8000-000000000212</dc:identifier>'
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
    # Replay-rich & multi-window point spine (~350 pages = 3 segments):
    # - 480 B prolog comment + 20 nested <blockquote>s (220 B Names) + rich
    #   inline runs guarantee every interior FIRP point > 2,100 B (> 2 store windows).
    # - Dense long attributes + named HTML entities guarantee every 2,048 B
    #   filtered feed boundary inside [124, 128) splits mid-tag after entity
    #   expansion, yielding a guaranteed nonempty Replay section.
    replay_item = (
        '<span class="replay-harbor-marker-0123456789abcdef" '
        'data-token="lantern-tide-pier-0123456789abcdef0123456789">'
        'Harbor&nbsp;&mdash;&nbsp;tide&hellip; quiet stones and wooden pier. </span>'
    )
    replay_paras = ''.join(
        f'<p id="r{i}">{replay_item * 22}</p>\n' for i in range(1, 320)
    )
    bq_open = '<blockquote>' * 18
    bq_close = '</blockquote>' * 18
    ch2_replay = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!-- ' + ('R' * 450) + ' -->\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body>'
        + bq_open + '<h1>Replay &amp; Multi-Window</h1>\n'
        + replay_paras + bq_close + '</body></html>'
    ).encode('utf-8')
    write_epub(directory / 'stored.epub', [('ch0.xhtml', ch0), ('ch1.xhtml', ch1), ('ch2.xhtml', ch2_replay)], zipfile.ZIP_STORED)
    write_epub(directory / 'deflated.epub', [('ch0.xhtml', ch0), ('ch1.xhtml', ch1)], zipfile.ZIP_DEFLATED)

    # Encoding fixtures (known-UTF-8 gate): ~280 pages each (3 segments).
    plain_sentence = (
        'A lantern glowed beside the quiet harbor road while the tide drew '
        'back across the stones and gulls circled above the wooden pier. '
    )
    enc_body = ''.join(
        f'<p>Paragraph {i}: {plain_sentence * 12}</p>\n' for i in range(1, 130)
    )
    latin1_doc = (
        "<?xml version='1.0' encoding = 'ISO-8859-1'?>\n"
        '<caf\xe9 xmlns="http://www.w3.org/1999/xhtml"><body><h1>Caf\xe9</h1>\n'
        + enc_body + '</body></caf\xe9>'
    ).encode('latin-1')
    utf16_doc = (
        '<?xml version="1.0" encoding="UTF-16"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>UTF16</h1>\n'
        + enc_body + '</body></html>'
    ).encode('utf-16')  # includes UTF-16 LE BOM 0xFF 0xFE on x86_64
    utf8_bom_doc = (
        b'\xef\xbb\xbf'
        + (
            '<?xml version="1.0" encoding = "UTF-8"?>\n'
            '<café xmlns="http://www.w3.org/1999/xhtml"><body><h1>Café</h1>\n'
            + enc_body + '</body></café>'
        ).encode('utf-8')
    )
    write_epub(directory / 'encoding.epub', [
        ('latin1.xhtml', latin1_doc),
        ('utf16.xhtml', utf16_doc),
        ('utf8bom.xhtml', utf8_bom_doc),
    ])

    # Decline & window-recovery fixtures:
    # For spine 0 (refusal then eligibility in [124, 128)):
    # 48 paragraphs reach ~page 120; then 50 nested <div>s stay open across
    # 18 short padded <p> blocks so feed boundaries at pages 124..126 decline
    # (Decline::Depth), then all 50 </div>s close and verbose span-padded <p>
    # blocks capture edge 128 at page 126 cleanly inside [124, 128).
    prefix_121 = ''.join(
        f'<p>Lead {i}: {plain_sentence * 12}</p>\n' for i in range(1, 49)
    )
    padded_word = '<span class="harbor-window-pad">lantern </span>'
    deep_window = (
        ('<div>' * 50)
        + ''.join(f'<p>{padded_word * 28}</p>\n' for _ in range(18))
        + ('</div>' * 50)
    )
    after_window = ''.join(f'<p>{padded_word * 24}</p>\n' for _ in range(12))
    tail_160 = ''.join(
        f'<p>Tail {i}: {plain_sentence * 12}</p>\n' for i in range(1, 70)
    )
    s0_refusal_then_ok = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Window Recovery</h1>\n'
        + prefix_121 + deep_window + after_window + tail_160 + '</body></html>'
    ).encode('utf-8')

    # Spine 1: CDATA spans pages ~115..138 (entire [124, 128) window).
    prefix_112 = ''.join(
        f'<p>Lead {i}: {plain_sentence * 12}</p>\n' for i in range(1, 45)
    )
    wide_cdata = f'<p><![CDATA[{plain_sentence * 150}]]></p>\n'
    s1_cdata = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>CDATA Decline</h1>\n'
        + prefix_112 + wide_cdata + tail_160 + '</body></html>'
    ).encode('utf-8')

    # Spine 2: 50 nested <div> elements open across the entire [124, 128) window.
    open_50 = '<div>' * 50
    close_50 = '</div>' * 50
    mid_30 = ''.join(f'<p>Deep {i}: {plain_sentence * 12}</p>\n' for i in range(1, 14))
    s2_depth = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Depth Decline</h1>\n'
        + prefix_112 + open_50 + mid_30 + close_50 + tail_160 + '</body></html>'
    ).encode('utf-8')

    # Spine 3: 52 nested <div> elements near start, fully closed long before page 124.
    early_deep = ('<div>' * 52) + '<p>Brief deep block.</p>' + ('</div>' * 52)
    s3_unwound = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Depth Unwound</h1>\n'
        + early_deep + enc_body + '</body></html>'
    ).encode('utf-8')

    # Spine 4: prolog > 512 B (600-byte XML comment before root).
    s4_prolog = (
        '<?xml version="1.0" encoding="UTF-8"?>\n<!--' + ('p' * 580) + '-->\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Long Prolog</h1>\n'
        + enc_body + '</body></html>'
    ).encode('utf-8')

    # Spine 5: compact <hr/> block crosses from ~page 112 past page 128 inside one 2048 B feed.
    s5_late = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Late Jump</h1>\n'
        + prefix_112 + ''.join('H<hr/>' for _ in range(320)) + tail_160 + '</body></html>'
    ).encode('utf-8')

    # Spine 6: reaches page 124 and opens a 2,400 B XML comment so an unfinished
    # token spans across feed boundaries (backlog > last_feed.len() ->
    # Decline::Backlog), followed by closing `-->` and eligible paragraphs inside
    # [124, 128) so a later eligible cut still captures cleanly.
    s6_backlog = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Backlog Recovery</h1>\n'
        + ''.join(f'<p>Lead {i}: {plain_sentence * 12}</p>\n' for i in range(1, 51))
        + '<!--' + ('b' * 2400) + '-->\n'
        + after_window
        + tail_160
        + '</body></html>'
    ).encode('utf-8')

    write_epub(directory / 'declines.epub', [
        ('s0.xhtml', s0_refusal_then_ok),
        ('s1.xhtml', s1_cdata),
        ('s2.xhtml', s2_depth),
        ('s3.xhtml', s3_unwound),
        ('s4.xhtml', s4_prolog),
        ('s5.xhtml', s5_late),
        ('s6.xhtml', s6_backlog),
    ])

    # Sparse-prefix / dense-tail chapter for enforced marginal credit preflight:
    # 124 very sparse pages (~35 B serialized each, ~4.5 KiB total prefix),
    # followed by dense styled multi-run paragraphs (~2.5-3.5 KiB/page tail).
    sparse_page = '<p>S</p>' + ('<br/>' * 19) + '\n'
    dense_runs = ''.join(
        f'<b>w{k}</b> <i>x{k}</i> <a href="ch1.xhtml#n1">l{k}</a> '
        for k in range(42)
    )
    dense_tail = ''.join(f'<p id="d{i}">{dense_runs}</p>\n' for i in range(1, 35))
    budget_doc = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!-- ' + ('B' * 380) + ' -->\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Sparse Dense</h1>\n'
        + ''.join(f'<p>P{i}</p>' + ('<br/>' * 18) + '\n' for i in range(1, 125))
        + dense_tail + '</body></html>'
    ).encode('utf-8')
    write_epub(directory / 'budget.epub', [('budget.xhtml', budget_doc), ('ch1.xhtml', ch1)], zipfile.ZIP_STORED)


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
        workspace / 'tests/resume_capture/host.rs', '-o', output_binary,
    ]
    run.run(command, workspace)


def main():
    support = load('resume_capture_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/resume-capture'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        fixtures = output / 'fixtures'
        fixtures.mkdir()
        make_fixtures(fixtures)
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 300)
        target = output / 'target'
        try:
            native_pins = load('resume_capture_native', workspace / 'tests/phased_reader/native.py')
            native_pins.verify_native_inputs(support, ROOT / 'vendor/freeink-sdk')
            sdk = ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
            obj_names = ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge', 'board')
            objects = {name: compile_c(run, workspace, output, sdk, name) for name in obj_names}

            libraries = build_libraries(run, workspace, target)
            positive = output / 'positive'
            compile_host(run, workspace, target, libraries, list(objects.values()), positive, opt_level=1)
            stdout = run.run([positive, fixtures], workspace)
            support.require('PASS resume capture (11 cases)' in stdout,
                            f'all 11 resume capture cases must pass:\n{stdout}')
            print(stdout, end='', flush=True)

            if negative_controls(__file__):
                for name, rel_source, good, bad, case, witness in CONTROLS:
                    path = workspace / rel_source
                    original = path.read_text()
                    support.require(original.count(good) == 1,
                                    f'control "{name}": anchor must appear once in rust/{rel_source}')
                    path.write_text(original.replace(good, bad, 1))
                    try:
                        mut_objects = dict(objects)
                        if rel_source.endswith('expat_bridge.c'):
                            mut_objects['bridge'] = compile_c(run, workspace, output, sdk, 'bridge')
                            mut_libs = libraries
                        else:
                            mut_libs = build_libraries(run, workspace, target)
                        binary = output / name
                        compile_host(run, workspace, target, mut_libs, list(mut_objects.values()), binary, opt_level=0)
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
                        if rel_source.endswith('expat_bridge.c'):
                            compile_c(run, workspace, output, sdk, 'bridge')
                    print(f'PASS control: {name} rejected by {case}', flush=True)

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
