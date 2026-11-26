#!/usr/bin/env python3
"""Host preview goldens, pixel checks and negative controls for #43 slice 1
(CSS small caps and text-transform).

Verifies end-to-end rendering through the real host-built firmware (`newt-preview`),
ReaderAtlas, phased-reader stylesheet compaction (>64 rules), and chapter_layout:
- `font-variant: small-caps` / `font-variant-caps: small-caps` synthesize ~80%
  uppercase glyphs aligned to the full-cap baseline (hitting native 24 px at the
  default 30 px reading size), preserve inline `normal` resets, inhibit lowercase
  ligatures, handle non-ASCII bicameral scripts, and flush justified lines cleanly.
- `text-transform: uppercase | lowercase | capitalize | none` transform text in
  the right precedence order before `font-variant: small-caps`.
- Three compiled negative controls (`css-disabled`, `fullsize-caps`,
  `priority-dropped`) prove the structural pixel checks catch broken CSS parsing,
  missing 80% scaling, and dropped compaction priority.
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
    'portrait_ch1': 'bc76e77edc20fc2060b4bc905c3ff414fcbd241131978ca735ab3002063932cc',
    'portrait_ch2': '5d4de3f99f4f86e6dbc44be888503344296a71d9b129dcaf09a8122ecf8a9d16',
    'portrait_ch1_px24': '1d8b54ddec257b140c8feaff2607fa095839e99d99faffa3b04d5250f06566b6',
    'landscape_ch1': 'f4245a8772edec5db2e15bf18c1adc6f73ed3fcaf2f86b4f30f78ad9d76fbd5e',
}


def make_synthetic_epub(path: Path) -> None:
    """Deterministic CC0 synthetic EPUB exercising small caps, text-transform,
    inline resets, non-ASCII scripts, justification, and >64-rule compaction."""
    filler = ''.join(f'.f{i:02d} {{ color: #111; }}\n' for i in range(70))
    css = (
        filler
        + 'p { text-indent: 0; margin-bottom: 0.45em; }\n'
        + '.sc { font-variant: small-caps; }\n'
        + '.fvc { font-variant-caps: small-caps; }\n'
        + '.norm { font-variant: normal; }\n'
        + '.up { text-transform: uppercase; }\n'
        + '.lo { text-transform: lowercase; }\n'
        + '.cap { text-transform: capitalize; }\n'
        + '.none { text-transform: none; }\n'
        + '.just { text-align: justify; }\n'
    )
    ch1 = (
        '<h2>Harbor Notes</h2>\n'
        '<p class="sc">Pygmy quayside lantern, <span class="norm">pygmy normal</span>, '
        'then Small Caps again.</p>\n'
        '<p class="fvc">Font-variant-caps: NASA, Caf\u00e9 na\u00efve, '
        '\u03b1\u03b2\u03c2, \u041f\u0440\u0438\u0432\u0435\u0442, Vi\u1ec7t, 1894.</p>\n'
        '<p class="sc just">The brave aviator watched the harbor beacon sweep across the '
        'quiet pier and stone jetty while the evening tide rose along the quay.</p>\n'
        '<p class="sc">First find fifty flags without ligature drift.</p>\n'
    )
    ch2 = (
        '<p class="up">pygmy quayside <span class="none">pygmy</span></p>\n'
        '<p class="lo">PYGMY QUAYSIDE CAF\u00c9 \u041f\u0420\u0418\u0412\u0415\u0422</p>\n'
        '<p class="cap">don\'t stop \'quoted\' well-known quay</p>\n'
        '<p class="up sc">Uppercase beats small caps here.</p>\n'
        '<p class="lo sc">LOWERCASE THEN SMALL CAPS.</p>\n'
        '<p class="cap sc">capitalize then small caps.</p>\n'
    )
    chapters = [('Small Caps', ch1), ('Text Transform', ch2)]
    with zipfile.ZipFile(path, 'w') as zf:
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
        zf.writestr(
            'META-INF/container.xml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            '<rootfiles><rootfile full-path="OEBPS/content.opf" '
            'media-type="application/oebps-package+xml"/></rootfiles></container>\n',
        )
        zf.writestr('OEBPS/style.css', css)
        for idx, (ctitle, body) in enumerate(chapters, 1):
            zf.writestr(
                f'OEBPS/ch{idx}.xhtml',
                '<?xml version="1.0" encoding="UTF-8"?>\n'
                '<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en">'
                f'<head><title>{ctitle}</title>'
                '<link rel="stylesheet" type="text/css" href="style.css"/></head>'
                f'<body>{body}</body></html>\n',
            )
        toc = '\n'.join(
            f'<li><a href="ch{i}.xhtml">{t}</a></li>' for i, (t, _) in enumerate(chapters, 1)
        )
        zf.writestr(
            'OEBPS/nav.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
            f'<head><title>Contents</title></head><body><nav epub:type="toc"><ol>{toc}'
            '</ol></nav></body></html>\n',
        )
        manifest = '\n'.join(
            f'<item id="c{i}" href="ch{i}.xhtml" media-type="application/xhtml+xml"/>'
            for i in range(1, len(chapters) + 1)
        )
        spine = ''.join(f'<itemref idref="c{i}"/>' for i in range(1, len(chapters) + 1))
        zf.writestr(
            'OEBPS/content.opf',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<package xmlns="http://www.idpf.org/opf" version="3.0" unique-identifier="id">'
            '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
            '<dc:identifier id="id">urn:newt-test:small-caps-cc0</dc:identifier>'
            '<dc:title>Harbor Notes</dc:title><dc:language>en</dc:language></metadata>'
            '<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" '
            f'properties="nav"/><item id="css" href="style.css" media-type="text/css"/>'
            f'{manifest}</manifest><spine>{spine}</spine></package>\n',
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


def ink_bbox(raw: bytes, stride: int, x0: int, x1: int, y0: int, y1: int):
    min_x, max_x, min_y, max_y = x1, -1, y1, -1
    for y in range(y0, y1):
        row = y * stride
        for x in range(x0, x1):
            if raw[row + (x >> 3)] & (0x80 >> (x & 7)):
                if x < min_x:
                    min_x = x
                if x > max_x:
                    max_x = x
                if y < min_y:
                    min_y = y
                if y > max_y:
                    max_y = y
    return None if max_x < 0 else (min_x, max_x, min_y, max_y)


def render_frame(binary: Path, actions: tuple[int, ...], orientation: str | None = None) -> bytes:
    with tempfile.TemporaryDirectory(prefix='newt-small-caps-') as scratch:
        directory = Path(scratch)
        qs = preview_driver.QUICK_START[orientation] if orientation else preview_driver.SKIP_QUICK_START
        preview_driver.first_boots(binary, directory, [qs])
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        make_synthetic_epub(books / '01-small-caps.epub')
        preview_driver.fresh(directory)
        preview_driver.play(binary, directory, [CONFIRM, CONFIRM, *actions])
        return (directory / 'frame.pbm').read_bytes()


def check_portrait_ch1(binary: Path, *, check_golden: bool = True) -> tuple[str, str] | None:
    pbm = render_frame(binary, ())
    width, height, stride, raw = parse_pbm(pbm)
    if (width, height) != (528, 792):
        return ('bad_dimensions', f'unexpected portrait dimensions {(width, height)}')
    # Line 1 of `.sc`: full-size 'P' (30 px) followed by small-cap 'YGMY' (24 px).
    # Small-cap glyphs must sit strictly below the full-cap apex (>= 3 px lower top)
    # while sharing the baseline with zero lowercase descenders ('g', 'y').
    full_p = ink_bbox(raw, stride, 16, 34, 95, 135)
    small_ygmy = ink_bbox(raw, stride, 36, 115, 95, 135)
    norm_pygmy = ink_bbox(raw, stride, 400, 512, 95, 135)
    if full_p is None or small_ygmy is None or norm_pygmy is None:
        return ('missing_glyphs', f'missing ch1 glyphs: full_p={full_p} small_ygmy={small_ygmy} norm_pygmy={norm_pygmy}')
    if small_ygmy[3] > full_p[3] + 1:
        return (
            'small_caps_has_descenders',
            f'small-caps text still has lowercase descenders below baseline '
            f'(small_ygmy bottom={small_ygmy[3]}, full_p bottom={full_p[3]})',
        )
    step = small_ygmy[2] - full_p[2]
    if step < 3:
        return (
            'small_caps_not_scaled',
            f'small-caps glyphs are not scaled below full-cap height '
            f'(full_p top={full_p[2]}, small_ygmy top={small_ygmy[2]}, step={step})',
        )
    if norm_pygmy[3] < full_p[3] + 5:
        return (
            'normal_reset_missing_descenders',
            f'inline font-variant:normal reset lost lowercase descenders '
            f'(norm_pygmy bottom={norm_pygmy[3]}, full_p bottom={full_p[3]})',
        )
    # Justified small-caps lines must flush cleanly to the right content margin (x=512).
    just1 = ink_bbox(raw, stride, 16, 512, 265, 300)
    if just1 is None or just1[1] < 506:
        return ('justify_short', f'justified small-caps line did not reach right margin: {just1}')
    if check_golden:
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS['portrait_ch1']:
            return ('golden_mismatch', f'portrait_ch1 golden mismatch: got {got}, want {GOLDENS["portrait_ch1"]}')
    return None


def check_portrait_ch2(binary: Path, *, check_golden: bool = True) -> tuple[str, str] | None:
    pbm = render_frame(binary, (RIGHT,))
    width, height, stride, raw = parse_pbm(pbm)
    if (width, height) != (528, 792):
        return ('bad_dimensions', f'unexpected portrait dimensions {(width, height)}')
    # Row 1: `.up` "PYGMY" (x=16..115: tall full caps, top <= 27, zero descenders bottom <= 46)
    # vs `<span class="none">pygmy</span>` (lowercase: lower top >= 29 and descenders >= 50).
    up_left = ink_bbox(raw, stride, 16, 115, 20, 60)
    up_reset = ink_bbox(raw, stride, 280, 420, 20, 60)
    if up_left is None or up_reset is None:
        return ('missing_glyphs', f'missing ch2 uppercase row: up_left={up_left} up_reset={up_reset}')
    if up_left[2] > 27 or up_left[3] > 46:
        return ('uppercase_not_applied', f'text-transform:uppercase left text lowercase: up={up_left}')
    if up_reset[2] - up_left[2] < 3 or up_reset[3] < up_left[3] + 5:
        return ('uppercase_reset_failed', f'inline text-transform:none reset failed: up={up_left} reset={up_reset}')
    # Row 2: `.lo` "pygmy quayside..." must emit lowercase descenders (bottom >= 99).
    lo_row = ink_bbox(raw, stride, 16, 500, 68, 106)
    if lo_row is None or lo_row[3] < 99:
        return ('lowercase_missing_descenders', f'text-transform:lowercase missing lowercase descenders: {lo_row}')
    # Precedence rows: `.up.sc` is full-cap height (>= 20 px); `.lo.sc` is uniform small-cap
    # height (<= 18 px, at least 3 px shorter than `.up.sc`).
    up_sc = ink_bbox(raw, stride, 16, 512, 200, 235)
    lo_sc = ink_bbox(raw, stride, 16, 512, 290, 320)
    if up_sc is None or lo_sc is None:
        return ('missing_glyphs', f'missing ch2 precedence rows: up_sc={up_sc} lo_sc={lo_sc}')
    up_h = up_sc[3] - up_sc[2] + 1
    lo_h = lo_sc[3] - lo_sc[2] + 1
    if up_h < 20 or up_h - lo_h < 3:
        return ('precedence_height_mismatch', f'uppercase/lowercase + small-caps precedence mismatch: up_h={up_h} lo_h={lo_h}')
    if check_golden:
        got = hashlib.sha256(pbm).hexdigest()
        if got != GOLDENS['portrait_ch2']:
            return ('golden_mismatch', f'portrait_ch2 golden mismatch: got {got}, want {GOLDENS["portrait_ch2"]}')
    return None


def check_other_goldens(binary: Path) -> str | None:
    # Menu -> Reading settings -> Text size -> cycle 3x (Px30 -> Px34 -> Px22 -> Px24) -> Save.
    px24_actions = (CONFIRM, RIGHT, RIGHT, CONFIRM, RIGHT, CONFIRM, CONFIRM, CONFIRM, RIGHT, RIGHT, RIGHT, CONFIRM)
    for name, actions, orientation in (
        ('portrait_ch1_px24', px24_actions, None),
        ('landscape_ch1', (), 'landscape-ccw'),
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
    rel_path: str,
    needle: str,
    replacement: str,
) -> Path:
    """Compile a scratch firmware preview with one targeted source mutation."""
    src = ROOT / 'rust' / rel_path
    dst = rust_copy / rel_path
    original = src.read_text()
    assert original.count(needle) == 1, f'mutation site missing in {rel_path} for {name}'
    dst.write_text(original.replace(needle, replacement, 1))
    try:
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
    finally:
        dst.write_text(original)


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for check in (check_portrait_ch1, check_portrait_ch2, check_other_goldens):
        problem = check(binary)
        assert problem is None, f'{check.__name__}: {problem}'
    if not negative_controls(__file__):
        print('PASS: 4 small-caps/text-transform preview goldens and pixel checks passed (controls not in this tier)')
        return
    with tempfile.TemporaryDirectory(prefix='newt-small-caps-ctrl-') as scratch_name:
        scratch = Path(scratch_name)
        rust_copy = scratch / 'rust'
        shutil.copytree(
            ROOT / 'rust',
            rust_copy,
            ignore=shutil.ignore_patterns('target', 'tests', '__pycache__'),
        )
        target = scratch / 'target'

        # Control 1: drop CSS parsing of font-variant / font-variant-caps / text-transform.
        css_disabled = build_mutated_preview(
            scratch,
            rust_copy,
            target,
            'css-disabled',
            'core/src/css.rs',
            '        _ => apply_text_case(d, prop, v),',
            '        _ => {},',
        )
        c1_ch1 = check_portrait_ch1(css_disabled, check_golden=False)
        assert c1_ch1 is not None and c1_ch1[0] == 'small_caps_has_descenders', (
            f'css-disabled ch1 control did not fail as expected: {c1_ch1!r}'
        )
        c1_ch2 = check_portrait_ch2(css_disabled, check_golden=False)
        assert c1_ch2 is not None and c1_ch2[0] == 'uppercase_not_applied', (
            f'css-disabled ch2 control did not fail as expected: {c1_ch2!r}'
        )

        # Control 2: uppercase without 80% small-cap scale-down.
        fullsize_caps = build_mutated_preview(
            scratch,
            rust_copy,
            target,
            'fullsize-caps',
            'core/src/chapter_layout/case.rs',
            'pub const fn small_cap_px(base_px: u16) -> u16 {\n'
            '    let scaled = (base_px as u32 * 80 + 50) / 100;\n'
            '    if scaled < 8 { 8 } else { scaled as u16 }\n'
            '}',
            'pub const fn small_cap_px(base_px: u16) -> u16 {\n'
            '    base_px\n'
            '}',
        )
        c2_ch1 = check_portrait_ch1(fullsize_caps, check_golden=False)
        assert c2_ch1 is not None and c2_ch1[0] == 'small_caps_not_scaled', (
            f'fullsize-caps control did not fail as expected: {c2_ch1!r}'
        )

        # Control 3: drop d.text_case from styles::priority compaction.
        priority_dropped = build_mutated_preview(
            scratch,
            rust_copy,
            target,
            'priority-dropped',
            'phased-reader/src/styles.rs',
            '        || d.vert_align >= 0\n'
            '        || d.text_case != 0',
            '        || d.vert_align >= 0',
        )
        c3_ch1 = check_portrait_ch1(priority_dropped, check_golden=False)
        assert c3_ch1 is not None and c3_ch1[0] == 'small_caps_has_descenders', (
            f'priority-dropped ch1 control did not fail as expected: {c3_ch1!r}'
        )
        c3_ch2 = check_portrait_ch2(priority_dropped, check_golden=False)
        assert c3_ch2 is not None and c3_ch2[0] == 'uppercase_not_applied', (
            f'priority-dropped ch2 control did not fail as expected: {c3_ch2!r}'
        )

    print(
        'PASS: 4 small-caps/text-transform preview goldens and pixel checks passed; '
        'compiled negative controls caught disabled CSS parsing, missing 80% small-cap '
        'scaling, and dropped stylesheet compaction priority'
    )


if __name__ == '__main__':
    main()
