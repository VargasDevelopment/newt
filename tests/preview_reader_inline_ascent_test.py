#!/usr/bin/env python3
"""Host preview goldens, pixel checks and negative controls for #148 (inline ascent/descent).

A line with an oversized or vertically shifted inline run (such as a span drop cap,
an oversized superscript/subscript, or an oversized descender) reserves enough height
above and below its baseline so glyphs never overprint adjacent lines, clip at the
top margin, or slice off descenders at the bottom content boundary. `::first-letter`
stays ignored and unaffected, and no float/wrap-beside behavior is added here
(#43 slice 2).
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import firmware_host_build as host_build  # noqa: E402
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, LEFT, RIGHT = 0, 1, 2, 3

# Visually inspected real host-preview PBM hashes for the synthetic CC0 EPUB below.
GOLDENS = {
    'portrait_midpage': 'f58e2a452f5a2ccf77653d4268a4c3809b85b853bd74716f7bc06ee14da46707',
    'portrait_firstletter': 'e55a777d0e154bbb1102f3bea2dafc9bf1bd09b8a96cb88eea9afcacf0016a43',
    'portrait_pagetop': 'd981051dd864518b1cc4c7820840e8821b7020e3baf53d00c8c05d7d4c1d61ce',
    'portrait_sup': '143720ef8d94e8f6f2dca6dffff4edd5a571a560fa3014f14b4def5d911d9dfc',
    'portrait_desc_p1': '5938433fe15e3d32b988836f79156b082fbbabf679caf56a7cd8e3a232d79bc1',
    'portrait_desc_p2': '27bee9f40d2e1a12a2eb91d4e5cdde96f2edbd7283337626c733b4bbc248b0a5',
    'landscape_midpage_p1': 'af851f5093469fb6e53c4892fc609f3438d0b39de8d6c4151b97c899e922e1b9',
    'landscape_midpage_p2': '45502830c81db9b98865356cf5bed498983dab4e600692c01f3ff63aac834e1e',
}


def make_synthetic_epub(path: Path) -> None:
    """Deterministic CC0 synthetic EPUB exercising drop caps, superscripts and descenders."""
    css = (
        'p { text-indent: 1.2em; margin-bottom: 0.4em; }\n'
        'p.noindent { text-indent: 0; }\n'
        'span.dropcap { float: left; font-size: 3em; line-height: 0.85; padding-right: 0.12em; }\n'
        'span.bigsup { font-size: 250%; vertical-align: super; }\n'
        'span.bigsub { font-size: 250%; vertical-align: sub; }\n'
        'p.firstletter { text-indent: 0; }\n'
        'p.firstletter::first-letter { float: left; font-size: 3em; line-height: 0.85; padding-right: 0.12em; }\n'
    )
    ch1 = (
        '<h2>Chapter One: Harbor Watch</h2>\n'
        '<p class="noindent"><span class="dropcap">T</span>he small lantern burned steadily beside '
        'the stone seawall while the tide turned along the harbor mouth and the timber pilings '
        'creaked in the swell.</p>\n'
        '<p>Beneath the pier, the harbor master checked the mooring lines and noted the wind '
        'in his logbook before climbing the wet steps.</p>\n'
        '<p class="noindent"><span class="dropcap">W</span>hen the fog horn sounded from the outer '
        'shoal, every skiff tied along the inner quay swayed together in the narrow lane of dark water.</p>\n'
        '<p class="firstletter">Down at the south slipway, the signal lamp blinked twice across the '
        'quiet bay to greet the returning mail boat.</p>\n'
    )
    ch2 = (
        '<p class="noindent"><span class="dropcap">A</span>t the very top of a page without a heading '
        'above it, a span drop cap reserves full ascent above its baseline so its apex renders cleanly '
        'inside the top margin.</p>\n'
        '<p>Normal body prose below continues with standard line spacing.</p>\n'
    )
    ch3 = (
        '<p class="noindent"><span class="bigsup">\u01d9</span>nder the beacon, a raised capital '
        'whose bearing exceeds one em stays unclipped at the top margin.</p>\n'
        '<p>A normal line of prose precedes a mid-page raised accented capital so any top overflow '
        'would collide with this line.</p>\n'
        '<p class="noindent"><span class="bigsup">\u00c9</span>vening mist settled over the pilings '
        'while the beacon turned.</p>\n'
    )
    # 12 standard single-line paragraphs + entry 13 with 0.8em bottom margin leave 92 px above
    # content_bottom (744): fits an ascent-only 81 px row (clipping 'ℊ's 30 px descender at
    # y=743), but requires 105 px with ink_extents so 'ℊ' moves cleanly onto page 2.
    filler = ''.join(f'<p class="noindent">Watch log entry number {i}.</p>\n' for i in range(1, 13))
    ch4 = (
        filler
        + '<p class="noindent" style="margin-bottom: 0.8em">Watch log entry number 13.</p>\n'
        + '<p class="noindent"><span class="dropcap">\u210a</span>azing over the jetty.</p>\n'
        + '<p class="noindent"><span class="bigsub">g</span>entle swells rose against the stone wall.</p>\n'
    )
    chapters = [
        ('Under Heading', ch1),
        ('Page Top', ch2),
        ('Raised Superscript', ch3),
        ('Oversized Descender', ch4),
    ]
    with zipfile.ZipFile(path, 'w') as zf:
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
        zf.writestr(
            'META-INF/container.xml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
            '  <rootfiles><rootfile full-path="OEBPS/content.opf" '
            'media-type="application/oebps-package+xml"/></rootfiles>\n'
            '</container>\n',
        )
        zf.writestr('OEBPS/style.css', css)
        for idx, (ctitle, body) in enumerate(chapters, 1):
            zf.writestr(
                f'OEBPS/ch{idx}.xhtml',
                '<?xml version="1.0" encoding="UTF-8"?>\n'
                '<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en">\n'
                f'<head><title>{ctitle}</title><link rel="stylesheet" type="text/css" href="style.css"/></head>\n'
                f'<body>{body}</body></html>\n',
            )
        toc = '\n'.join(f'  <li><a href="ch{i}.xhtml">{t}</a></li>' for i, (t, _) in enumerate(chapters, 1))
        zf.writestr(
            'OEBPS/nav.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">\n'
            f'<head><title>Contents</title></head><body><nav epub:type="toc"><ol>\n{toc}\n'
            '</ol></nav></body></html>\n',
        )
        manifest = '\n'.join(
            f'    <item id="c{i}" href="ch{i}.xhtml" media-type="application/xhtml+xml"/>'
            for i in range(1, len(chapters) + 1)
        )
        spine = ''.join(f'<itemref idref="c{i}"/>' for i in range(1, len(chapters) + 1))
        zf.writestr(
            'OEBPS/content.opf',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<package xmlns="http://www.idpf.org/opf" version="3.0" unique-identifier="id">\n'
            '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
            '    <dc:identifier id="id">urn:newt-test:inline-ascent-cc0</dc:identifier>\n'
            '    <dc:title>Harbor Watch</dc:title><dc:language>en</dc:language>\n'
            '  </metadata>\n'
            '  <manifest>\n'
            '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
            '    <item id="css" href="style.css" media-type="text/css"/>\n'
            f'{manifest}\n'
            '  </manifest>\n'
            f'  <spine>{spine}</spine>\n'
            '</package>\n',
        )
    os.utime(path, (1790812800, 1790812800))


def parse_pbm(data: bytes):
    lines = data.split(b'\n', 2)
    assert lines[0] == b'P4', f'unexpected PBM header {lines[0]!r}'
    width, height = map(int, lines[1].split())
    raw = lines[2]
    stride = width // 8
    assert len(raw) == stride * height
    return width, height, stride, raw


def row_ink(raw: bytes, stride: int, y: int, x0: int = 0, x1: int | None = None) -> int:
    if x1 is None:
        x1 = stride * 8
    return sum(1 for x in range(x0, x1) if raw[y * stride + (x >> 3)] & (0x80 >> (x & 7)))


def widest_white_band(raw: bytes, stride: int, y0: int, y1: int, x0: int = 16, x1: int = 512) -> int:
    widest = 0
    current = 0
    for y in range(y0, y1):
        if row_ink(raw, stride, y, x0, x1) == 0:
            current += 1
            widest = max(widest, current)
        else:
            current = 0
    return widest


def render_frame(binary: Path, actions: tuple[int, ...], orientation: str | None = None) -> bytes:
    with tempfile.TemporaryDirectory(prefix='newt-inline-ascent-') as scratch:
        directory = Path(scratch)
        qs = preview_driver.QUICK_START[orientation] if orientation else preview_driver.SKIP_QUICK_START
        preview_driver.first_boots(binary, directory, [qs])
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        make_synthetic_epub(books / '01-harbor.epub')
        preview_driver.fresh(directory)
        preview_driver.play(binary, directory, [CONFIRM, CONFIRM, *actions])
        return (directory / 'frame.pbm').read_bytes()


def check_portrait_midpage(binary: Path, *, check_golden: bool = True) -> str | None:
    pbm = render_frame(binary, ())
    width, height, stride, raw = parse_pbm(pbm)
    if (width, height) != (528, 792):
        return f'unexpected portrait dimensions {(width, height)}'
    # A clean white separation band (>= 20 px) must lie between "climbing the wet steps."
    # and the top of the mid-page 75 px 'W' drop cap (no overprinting), plus clear footer gap.
    gap = widest_white_band(raw, stride, 520, 640)
    if gap < 20:
        return f'mid-page drop cap overprints previous line (widest white gap {gap} px < 20 px)'
    footer_gap = sum(row_ink(raw, stride, y, 16, 512) for y in range(740, 745))
    if footer_gap != 0:
        return f'mid-page content encroaches on bottom margin ({footer_gap} px in y=740..745)'
    if check_golden:
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS['portrait_midpage']:
            return f'portrait_midpage golden mismatch: got {got}, want {GOLDENS["portrait_midpage"]}'
    return None


def check_portrait_pagetop(binary: Path, *, check_golden: bool = True) -> str | None:
    pbm = render_frame(binary, (RIGHT, RIGHT))
    width, height, stride, raw = parse_pbm(pbm)
    if (width, height) != (528, 792):
        return f'unexpected portrait dimensions {(width, height)}'
    margin_ink = sum(row_ink(raw, stride, y) for y in range(0, 16))
    if margin_ink != 0:
        return f'page-top ink bled into top margin ({margin_ink} px)'
    # The unclipped 'A' starts well below y=16 with a narrow apex (<= 14 px wide);
    # a clipped 'A' hits y=16 abruptly across its wider mid-body (>= 20 px wide).
    first_ink_y = next((y for y in range(16, 120) if row_ink(raw, stride, y, 0, 80) > 0), None)
    if first_ink_y is None:
        return 'page-top drop cap missing'
    apex_width = row_ink(raw, stride, first_ink_y, 0, 80)
    if first_ink_y <= 20 or apex_width > 14:
        return f'page-top drop cap clipped at top margin (first_ink_y={first_ink_y}, apex_width={apex_width})'
    if check_golden:
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS['portrait_pagetop']:
            return f'portrait_pagetop golden mismatch: got {got}, want {GOLDENS["portrait_pagetop"]}'
    return None


def check_portrait_sup(binary: Path, *, check_golden: bool = True) -> str | None:
    pbm = render_frame(binary, (RIGHT, RIGHT, RIGHT))
    width, height, stride, raw = parse_pbm(pbm)
    if (width, height) != (528, 792):
        return f'unexpected portrait dimensions {(width, height)}'
    margin_ink = sum(row_ink(raw, stride, y) for y in range(0, 16))
    if margin_ink != 0:
        return f'superscript ink bled into top margin ({margin_ink} px)'
    # The unclipped 75 px superscript 'Ǚ' (80 px top bearing) places its caron tips right at
    # y=16 with full ink (1176 px); without superscript/ink-extent reservation, rows slice off
    # above y=16 and drop the ink count sharply.
    glyph_ink = sum(row_ink(raw, stride, y, 0, 80) for y in range(16, 120))
    if glyph_ink < 1160:
        return f'oversized superscript clipped at top margin (glyph_ink={glyph_ink} < 1160)'
    mid_gap = widest_white_band(raw, stride, 360, 460)
    if mid_gap < 18:
        return f'mid-page superscript overprints previous line (widest white gap {mid_gap} px < 18 px)'
    if check_golden:
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS['portrait_sup']:
            return f'portrait_sup golden mismatch: got {got}, want {GOLDENS["portrait_sup"]}'
    return None


def check_portrait_descender_bottom(binary: Path, *, check_golden: bool = True) -> str | None:
    pbm1 = render_frame(binary, (RIGHT, RIGHT, RIGHT, RIGHT))
    width, height, stride, raw = parse_pbm(pbm1)
    if (width, height) != (528, 792):
        return f'unexpected portrait dimensions {(width, height)}'
    # Without descender reserve, the 75 px 'g' is admitted onto page 1 at y=658 (baseline 733)
    # and clips straight across the bottom content boundary rows y=740..744.
    clip_edge = sum(row_ink(raw, stride, y, 16, 512) for y in range(740, 744))
    if clip_edge != 0:
        return f'oversized descender clipped at bottom content boundary ({clip_edge} px in y=740..744)'
    if check_golden:
        got1 = hashlib.sha256(pbm1).hexdigest()
        if got1 != GOLDENS['portrait_desc_p1']:
            return f'portrait_desc_p1 golden mismatch: got {got1}, want {GOLDENS["portrait_desc_p1"]}'
        pbm2 = render_frame(binary, (RIGHT, RIGHT, RIGHT, RIGHT, RIGHT))
        got2 = hashlib.sha256(pbm2).hexdigest()
        if got2 != GOLDENS['portrait_desc_p2']:
            return f'portrait_desc_p2 golden mismatch: got {got2}, want {GOLDENS["portrait_desc_p2"]}'
    return None


def check_other_goldens(binary: Path) -> str | None:
    for name, actions, orientation in (
        ('portrait_firstletter', (RIGHT,), None),
        ('landscape_midpage_p1', (), 'landscape-ccw'),
        ('landscape_midpage_p2', (RIGHT,), 'landscape-ccw'),
    ):
        pbm = render_frame(binary, actions, orientation)
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS[name]:
            return f'{name} golden mismatch: got {got}, want {GOLDENS[name]}'
    return None


def build_mutated_preview(
    scratch: Path,
    rust_copy: Path,
    target: Path,
    name: str,
    needle: str,
    replacement: str,
) -> Path:
    """Compile a scratch firmware preview with one targeted mutation in core/engine.rs."""
    engine = rust_copy / 'core/src/chapter_layout/engine.rs'
    original = (ROOT / 'rust/core/src/chapter_layout/engine.rs').read_text()
    assert original.count(needle) == 1, f'mutation site missing in engine.rs for {name}'
    engine.write_text(original.replace(needle, replacement, 1))

    link = json.loads(rust_preview.LINK.read_text())
    tool = ROOT / '.toolchain'
    cargo = tool / 'rust/toolchain/bin/cargo'
    env = os.environ.copy()
    env.update(
        RUSTC=str(tool / 'rust/toolchain/bin/rustc'),
        RUSTDOC=str(tool / 'rust/toolchain/bin/rustdoc'),
        CARGO_HOME=str(tool / 'rust/cargo-home'),
        CARGO_NET_OFFLINE='true',
        CARGO_INCREMENTAL='0',
        CARGO_PROFILE_DEV_DEBUG='0',
        RUSTFLAGS='-C panic=abort',
    )
    packages = tomllib.loads((rust_copy / 'firmware/Cargo.toml').read_text())['dependencies']
    result = subprocess.run(
        [
            str(cargo), 'build', '--lib',
            *[arg for pkg in packages for arg in ('-p', pkg)],
            '--features', ','.join((*host_build.BASE_FEATURES, *rust_preview.DEPENDENCY_FEATURES)),
            '--target', host_build.HOST,
            '--manifest-path', str(rust_copy / 'Cargo.toml'),
            '--offline', '--frozen', '--locked', '--jobs', '2',
            '--target-dir', str(target),
            '--message-format=json',
        ],
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
        text=True,
        timeout=900,
    )
    rlibs = {}
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact':
            for filename in item['filenames']:
                if filename.endswith('.rlib'):
                    rlibs[item['target']['name']] = filename
    deps_dir = str(target / host_build.HOST / 'debug/deps')

    base_cmd = rust_preview._control_command(link['command'], optimize=False)
    rewritten = []
    skip_next = False
    for idx, arg in enumerate(base_cmd):
        if skip_next:
            skip_next = False
            continue
        if arg.startswith('-Ldependency='):
            rewritten.append(f'-Ldependency={deps_dir}')
        elif arg == '-L' and idx + 1 < len(base_cmd) and base_cmd[idx + 1].startswith('dependency='):
            rewritten += ['-L', f'dependency={deps_dir}']
            skip_next = True
        elif arg == '--extern' and idx + 1 < len(base_cmd) and '=' in base_cmd[idx + 1]:
            crate_name, old_path = base_cmd[idx + 1].split('=', 1)
            rewritten += ['--extern', f'{crate_name}={rlibs.get(crate_name, old_path)}']
            skip_next = True
        else:
            rewritten.append(arg)

    program = scratch / f'firmware-mutant-{name}'
    link_env = {**os.environ, **link['environment']}
    subprocess.run(
        [*rewritten, str(rust_preview.WORK / 'host.rs'), '-o', str(program)],
        cwd=ROOT,
        env=link_env,
        check=True,
        capture_output=True,
        text=True,
        timeout=900,
    )
    return program


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for check in (
        check_portrait_midpage,
        check_portrait_pagetop,
        check_portrait_sup,
        check_portrait_descender_bottom,
        check_other_goldens,
    ):
        problem = check(binary)
        assert problem is None, f'{check.__name__}: {problem}'
    if not negative_controls(__file__):
        print('PASS: 8 inline-ascent preview goldens and pixel checks passed (controls not in this tier)')
        return
    with tempfile.TemporaryDirectory(prefix='newt-inline-ascent-ctrl-') as scratch_name:
        scratch = Path(scratch_name)
        rust_copy = scratch / 'rust'
        shutil.copytree(
            ROOT / 'rust',
            rust_copy,
            ignore=shutil.ignore_patterns('target', 'tests', '__pycache__'),
        )
        target = scratch / 'target'

        disabled = build_mutated_preview(
            scratch,
            rust_copy,
            target,
            'disabled',
            '        if !self.params.inline_ascent || line.end <= line.start {\n'
            '            return LineExtra::default();\n'
            '        }',
            '        let _ = (backend, line, size, lh);\n'
            '        return LineExtra::default();',
        )
        mid_fail = check_portrait_midpage(disabled, check_golden=False)
        assert mid_fail is not None and 'overprints previous line' in mid_fail, (
            f'mid-page control did not fail as expected: {mid_fail!r}'
        )
        top_fail = check_portrait_pagetop(disabled, check_golden=False)
        assert top_fail is not None and 'clipped at top margin' in top_fail, (
            f'page-top control did not fail as expected: {top_fail!r}'
        )
        sup_fail = check_portrait_sup(disabled, check_golden=False)
        assert sup_fail is not None and 'clipped at top margin' in sup_fail, (
            f'superscript control did not fail as expected: {sup_fail!r}'
        )

        # Second mutant keeps ascent reservation but drops below-baseline descender reservation.
        ascent_only = build_mutated_preview(
            scratch,
            rust_copy,
            target,
            'ascent-only',
            '        LineExtra {\n'
            '            above: (max_above - base_ascent).clamp(0, i32::from(i16::MAX)) as i16,\n'
            '            below: (max_below - base_below).clamp(0, i32::from(i16::MAX)) as i16,\n'
            '        }',
            '        let _ = (max_below, base_below);\n'
            '        LineExtra {\n'
            '            above: (max_above - base_ascent).clamp(0, i32::from(i16::MAX)) as i16,\n'
            '            below: 0,\n'
            '        }',
        )
        desc_fail = check_portrait_descender_bottom(ascent_only, check_golden=False)
        assert desc_fail is not None and 'clipped at bottom content boundary' in desc_fail, (
            f'descender bottom-fit control did not fail as expected: {desc_fail!r}'
        )
    print(
        'PASS: 8 inline-ascent preview goldens and pixel checks passed; '
        'compiled negative controls caught mid-page overprint, page-top clip, '
        'oversized superscript clip and bottom descender clip'
    )


if __name__ == '__main__':
    main()
