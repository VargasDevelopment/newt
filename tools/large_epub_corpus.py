#!/usr/bin/env python3
"""Deterministic synthetic large-EPUB corpus generator for Newt (#61).

Generates plain-text, synthetic (CC0) EPUBs on each side of every admission,
48 KiB bank, TOC, page-cache, and file-size limit in issues #60 and #61.
Large books are built on demand and never committed to the repository.

Usage:
  python3 tools/large_epub_corpus.py --list
  python3 tools/large_epub_corpus.py --out .build/corpus --subset fast
  python3 tools/large_epub_corpus.py --out .build/corpus --subset full
  python3 tools/large_epub_corpus.py --out .build/corpus --case zip-entries-at

Axes covered (each case declares target axis and exact expected outcomes):
  zip-entries     1,024-entry cap (admission.rs)
  metadata-arena  48 KiB parse bank (spine_hashes) & 48 KiB shared metadata arena
  central-dir     1 MiB central directory cap (admission.rs)
  member-size     4 MiB uncompressed member cap (zip_record_rules.rs)
  expanded-total  64 MiB expanded total cap (zip_record_rules.rs)
  file-size       64 MiB source file cap (EXTERNAL_MAX_BYTES / source_file.rs)
  zip64           ZIP64 EOCD sentinel/locator refusal (zip_container.rs)
  name-length     511 B member name cap & xml_input MAX_PATH OOM boundary
  long-ids        Long manifest IDs competing for the 48 KiB metadata arena
  toc-size        Author-supplied nav TOC vs 48 KiB bank exhaustion
  toc-depth       Deeply nested EPUB 2 NCX and EPUB 3 nav hierarchies
  chapter-anchors 192-anchor cap in PageCacheWriter (Profile::Small)
  chapter-pages   768-page sync index (6x128) & 4,096-page Profile::Small cap
  long-web-novel  Combined multi-chapter text-only web novel profiles
"""
from dataclasses import asdict, dataclass
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True

KIB, MIB = 1024, 1024 * 1024
MAX_ENTRIES, MAX_CD_BYTES = 1024, MIB
MAX_MEMBER_BYTES, MAX_EXPANDED_BYTES, MAX_FILE_BYTES = 4 * MIB, 64 * MIB, 64 * MIB
MAX_ANCHORS_SMALL = 192
DOS_TIME, DOS_DATE = 0, ((2026 - 1980) << 9) | (10 << 5) | 1

# Original synthetic prose (CC0); never derived from any external book.
SENTENCES = (
    "A narrow footpath crossed the meadow toward the quiet creek.",
    "Small stones lined the water where clear ripples met the reeds.",
    "Morning mist lifted above the cedar ridge as a cool breeze arrived.",
    "Notes in a pocket journal recorded the distance between each milestone.",
    "A wooden bridge spanned the shallow ford beside two leaning willows.",
    "Lantern light flickered along the lane while evening clouds gathered.",
    "Simple maps marked every bend, orchard gate, and hillside spring.",
    "Calm weather kept the gravel trail dry from dawn until dusk.",
)
XHTML_HEAD = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<html xmlns="http://www.w3.org/1999/xhtml" lang="en">'
    "<head><title>{t}</title></head><body>\n<h1>{t}</h1>\n"
)
XHTML_TAIL = "</body></html>\n"


@dataclass(frozen=True)
class BookSpec:
    chapters: int = 2
    chapter_bytes: int = 0
    first_chapter_bytes: int = 0
    chapter_paragraphs: int = 0
    chapter_anchors: int = 0
    extra_entries: int = 0
    extra_manifest_items: int = 0
    toc_kind: str = "nav"
    toc_entries: int | None = None
    toc_depth: int = 1
    toc_absent: bool = False
    toc_malformed: bool = False
    path_dir: str = "OEBPS"
    first_chapter_href_len: int | None = None
    first_chapter_href_prefix: str = ""
    id_len: int = 0
    compress: bool = True
    target_file_bytes: int | None = None
    target_expanded_bytes: int | None = None
    target_cd_bytes: int | None = None
    zip64: bool = False
    seed: int = 1


@dataclass(frozen=True)
class Case:
    name: str
    axis: str
    subset: str
    expected_admit: str
    expected_open: str
    check_mode: str
    description: str
    spec: BookSpec
    expected_menu_valid: bool | None = None
    expected_authored: bool | None = None
    expected_menu_count: int | None = None
    expected_anchors: int | None = None
    expected_sync6: bool | None = None
    expected_small32: bool | None = None
    expected_pages_range: tuple[int, int] | None = None
    expected_fallback: str | None = None


def _paragraph(seed: int, chapter: int, index: int) -> str:
    base = (seed * 131 + chapter * 37 + index * 17) % len(SENTENCES)
    return f"<p>{' '.join(SENTENCES[(base + k) % len(SENTENCES)] for k in range(3))}</p>\n"


def chapter_xhtml(chapter: int, *, seed: int = 1, target_bytes: int = 0, paragraphs: int = 0, anchors: int = 0) -> bytes:
    head = XHTML_HEAD.format(t=f"Chapter {chapter + 1}")
    chunks = [head]
    if anchors > 0:
        chunks.extend(f'<p id="a{i:04d}">Anchor {i + 1}: {SENTENCES[i % len(SENTENCES)]}</p>\n' for i in range(anchors))
    elif paragraphs > 0:
        chunks.extend(f"<p>Line {i + 1}: {SENTENCES[(chapter + i) % len(SENTENCES)]}</p>\n" for i in range(paragraphs))
    elif target_bytes > 0:
        remaining = target_bytes - len(head) - len(XHTML_TAIL)
        if remaining < 0:
            raise ValueError(f"target_bytes {target_bytes} below minimum header+tail")
        block = "".join(_paragraph(seed, chapter, i) for i in range(8))
        if remaining >= len(block) + 8:
            reps = (remaining - 8) // len(block)
            chunks.append(block * reps)
            remaining -= reps * len(block)
        if remaining >= 8:
            unit = "The quiet trail winds onward through the cedar valley. "
            inner = remaining - 8
            chunks.append(f"<p>{(unit * (inner // len(unit) + 1))[:inner]}</p>\n")
        elif remaining > 0:
            chunks.append(" " * remaining)
    else:
        chunks.extend(_paragraph(seed, chapter, i) for i in range(4))
    chunks.append(XHTML_TAIL)
    data = "".join(chunks).encode("utf-8")
    if target_bytes > 0 and len(data) != target_bytes:
        raise AssertionError(f"exact size mismatch: got {len(data)}, want {target_bytes}")
    return data


def _chapter_href(spec: BookSpec, index: int) -> str:
    if index != 0 or spec.first_chapter_href_len is None:
        return f"c{index:04d}.xhtml"
    left = spec.first_chapter_href_len - len(".xhtml")
    if left < 1:
        raise ValueError("first_chapter_href_len too small")
    parts, seg = [], 0
    while left > 48:
        parts.append(f"d{seg:02d}_" + "a" * 44)
        left -= 49
        seg += 1
    parts.append("c" * left)
    href = "/".join(parts) + ".xhtml"
    assert len(href) == spec.first_chapter_href_len
    return href


def _item_id(spec: BookSpec, prefix: str, index: int) -> str:
    raw = f"{prefix}{index:04d}"
    return raw if spec.id_len <= len(raw) else raw + "_" + "x" * (spec.id_len - len(raw) - 1)


def _toc_bytes(kind: str, hrefs: list[str], count: int, depth: int) -> bytes:
    depth = max(1, min(depth, max(1, count)))
    def href_for(i: int) -> str:
        return hrefs[i * len(hrefs) // max(1, count)]
    if kind == "ncx":
        lines = [
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">\n'
            '<head><meta name="dtb:uid" content="urn:newt:corpus:1"/></head>\n'
            "<docTitle><text>Synthetic Corpus Book</text></docTitle>\n<navMap>\n"
        ]
        for i in range(depth):
            lines.append(f'<navPoint id="n{i}" playOrder="{i + 1}"><navLabel><text>Part {i + 1}</text></navLabel><content src="{href_for(i)}"/>\n')
        for i in range(depth, count):
            lines.append(f'<navPoint id="n{i}" playOrder="{i + 1}"><navLabel><text>Part {i + 1}</text></navLabel><content src="{href_for(i)}"/></navPoint>\n')
        lines.append("</navPoint>\n" * depth + "</navMap></ncx>\n")
        return "".join(lines).encode("utf-8")

    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en">'
        "<head><title>Contents</title></head><body>\n"
        '<nav epub:type="toc"><h1>Contents</h1>\n'
    ]
    for i in range(depth):
        lines.append(f'<ol><li><a href="{href_for(i)}">Part {i + 1}</a>\n')
    for i in range(depth, count):
        lines.append(f'</li><li><a href="{href_for(i)}">Part {i + 1}</a>\n')
    lines.append("</li></ol>\n" * depth + "</nav></body></html>\n")
    return "".join(lines).encode("utf-8")


def _opf(spec: BookSpec, hrefs: list[str], extra_manifest: list[str]) -> bytes:
    is_ncx = spec.toc_kind == "ncx"
    manifest = [
        '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>' if is_ncx
        else '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
    ]
    if spec.toc_absent:
        manifest.clear()
    spine = []
    for i, href in enumerate(hrefs):
        item_id = _item_id(spec, "c", i)
        opf_href = f"{spec.first_chapter_href_prefix}{href}" if i == 0 else href
        manifest.append(f'<item id="{item_id}" href="{opf_href}" media-type="application/xhtml+xml"/>')
        spine.append(f'<itemref idref="{item_id}"/>')
    for i, href in enumerate(extra_manifest):
        manifest.append(f'<item id="{_item_id(spec, "m", i)}" href="{href}" media-type="text/plain"/>')
    spine_attr = ' toc="ncx"' if is_ncx and not spec.toc_absent else ""
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<package version="3.0" unique-identifier="uid" xmlns="http://www.idpf.org/2007/opf">\n'
        '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
        f'<dc:identifier id="uid">urn:newt:corpus:{spec.seed}</dc:identifier>'
        "<dc:title>Synthetic Corpus Book</dc:title><dc:creator>Newt Synthetic</dc:creator>"
        "<dc:language>en</dc:language></metadata>\n"
        "<manifest>\n" + "\n".join(manifest) + "\n</manifest>\n"
        f"<spine{spine_attr}>\n" + "\n".join(spine) + "\n</spine>\n</package>\n"
    ).encode("utf-8")


def write_zip(entries: list[tuple[str, bytes, bool]], *, target_cd_bytes: int | None = None, zip64: bool = False) -> bytes:
    """Deterministic ZIP writer with exact CD sizing and optional ZIP64 EOCD."""
    names = [name.encode("utf-8") for name, _, _ in entries]
    comments = [b""] * len(entries)
    if target_cd_bytes is not None:
        pad = target_cd_bytes - sum(46 + len(n) for n in names)
        if pad < 0:
            raise ValueError(f"target_cd_bytes {target_cd_bytes} below minimum")
        for i in range(len(entries)):
            take = min(pad, 65535)
            comments[i], pad = b"x" * take, pad - take
            if pad == 0:
                break
        if pad != 0:
            raise ValueError(f"cannot pad central directory to {target_cd_bytes} with {len(entries)} entries")

    cache: dict[tuple[int, bytes], tuple[int, bytes, int]] = {}
    out, central = bytearray(), bytearray()
    for (name, data, compress), name_b, comment in zip(entries, names, comments):
        if compress:
            key = (len(data), hashlib.blake2b(data, digest_size=16).digest())
            if key not in cache:
                comp = zlib.compressobj(level=6, method=zlib.DEFLATED, wbits=-15)
                cache[key] = (8, comp.compress(data) + comp.flush(), zlib.crc32(data) & 0xFFFFFFFF)
            method, payload, crc = cache[key]
        else:
            method, payload, crc = 0, data, zlib.crc32(data) & 0xFFFFFFFF
        offset = len(out)
        out += struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, method, DOS_TIME, DOS_DATE, crc, len(payload), len(data), len(name_b), 0)
        out += name_b + payload
        central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, method, DOS_TIME, DOS_DATE, crc, len(payload), len(data), len(name_b), 0, len(comment), 0, 0, 0, offset)
        central += name_b + comment

    cd_offset, cd_size, count = len(out), len(central), len(entries)
    out += central
    if zip64:
        z64_at = len(out)
        out += struct.pack("<IQHHIIQQQQ", 0x06064B50, 44, 45, 45, 0, 0, count, count, cd_size, cd_offset)
        out += struct.pack("<IIQI", 0x07064B50, 0, z64_at, 1)
        out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 0xFFFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0)
    else:
        out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, count, count, cd_size, cd_offset, 0)
    return bytes(out)


def _split_sizes(total: int, count: int) -> list[int]:
    base, rem = divmod(total, count)
    return [base + (1 if i < rem else 0) for i in range(count)]


def build_epub(spec: BookSpec) -> bytes:
    """Build one deterministic synthetic EPUB from a BookSpec."""
    prefix = f"{spec.path_dir}/" if spec.path_dir else ""
    opf_path = f"{prefix}content.opf"
    toc_name = f"{prefix}{'toc.ncx' if spec.toc_kind == 'ncx' else 'nav.xhtml'}"
    hrefs = [_chapter_href(spec, i) for i in range(spec.chapters)]
    extra_manifest = [f"m{i:04d}.txt" for i in range(spec.extra_manifest_items)]
    toc_count = spec.chapters if spec.toc_entries is None else spec.toc_entries

    container = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
        f'<rootfiles><rootfile full-path="{opf_path}" '
        'media-type="application/oebps-package+xml"/></rootfiles></container>\n'
    ).encode("utf-8")
    fixed: list[tuple[str, bytes, bool]] = [
        ("mimetype", b"application/epub+zip", False),
        ("META-INF/container.xml", container, spec.compress),
        (opf_path, _opf(spec, hrefs, extra_manifest), spec.compress),
        *([] if spec.toc_absent else [(toc_name,
            b'<html><broken' if spec.toc_malformed else _toc_bytes(spec.toc_kind, hrefs, toc_count, spec.toc_depth),
            spec.compress)]),
    ]
    extras: list[tuple[str, bytes, bool]] = [
        (f"{prefix}{name}", b"note\n", spec.compress) for name in extra_manifest
    ] + [(f"e/{i:04d}", b"x", False) for i in range(spec.extra_entries)]

    chapter_sizes = [spec.chapter_bytes] * spec.chapters
    if spec.first_chapter_bytes > 0:
        chapter_sizes[0] = spec.first_chapter_bytes
    if spec.target_expanded_bytes is not None:
        chapter_sizes = _split_sizes(spec.target_expanded_bytes - sum(len(d) for _, d, _ in fixed + extras), spec.chapters)
    elif spec.target_file_bytes is not None:
        if spec.compress:
            raise ValueError("target_file_bytes requires compress=False (ZIP_STORED)")
        all_names = [n for n, _, _ in fixed] + [f"{prefix}{h}" for h in hrefs] + [n for n, _, _ in extras]
        overhead = sum(76 + 2 * len(n.encode("utf-8")) for n in all_names) + 22
        chapter_sizes = _split_sizes(spec.target_file_bytes - overhead - sum(len(d) for _, d, _ in fixed + extras), spec.chapters)

    chapters = [
        (
            f"{prefix}{href}",
            chapter_xhtml(
                i, seed=spec.seed, target_bytes=chapter_sizes[i],
                paragraphs=spec.chapter_paragraphs if i == 0 else 0,
                anchors=spec.chapter_anchors if i == 0 else 0,
            ),
            spec.compress,
        )
        for i, href in enumerate(hrefs)
    ]
    epub = write_zip(fixed + chapters + extras, target_cd_bytes=spec.target_cd_bytes, zip64=spec.zip64)
    if spec.target_file_bytes is not None and len(epub) != spec.target_file_bytes:
        raise AssertionError(f"file size mismatch: got {len(epub)}, want {spec.target_file_bytes}")
    return epub


CASES: tuple[Case, ...] = (
    Case("baseline", "baseline", "fast", "Ok", "Ok", "menu", "2-chapter text-only baseline EPUB",
         BookSpec(chapters=2), expected_menu_valid=True, expected_authored=True, expected_menu_count=2, expected_fallback="None"),
    Case("zip-entries-at", "zip-entries", "fast", "Ok", "Ok", "admit", "Exactly 1,024 ZIP entries (4 core + 2 chapters + 1,018 short extra entries)",
         BookSpec(chapters=2, extra_entries=MAX_ENTRIES - 6)),
    Case("zip-entries-over", "zip-entries", "fast", "Unsupported", "Unsupported", "admit", "1,025 ZIP entries: one over admission.rs 1,024-entry cap",
         BookSpec(chapters=2, extra_entries=MAX_ENTRIES - 5)),
    Case("parse-spine-fit", "metadata-arena", "fast", "Ok", "Ok", "menu", "240 chapters (244 entries): fits both 48 KiB banks with authored TOC",
         BookSpec(chapters=240), expected_menu_valid=True, expected_authored=True, expected_menu_count=240, expected_fallback="None"),
    Case("parse-spine-over", "metadata-arena", "fast", "Ok", "OutOfMemory", "admit", "300 chapters (304 entries < 1,024): admission accepts, 48 KiB parse bank spine_hashes OOMs",
         BookSpec(chapters=300)),
    Case("shared-arena-fit", "metadata-arena", "fast", "Ok", "Ok", "admit", "350 extra manifest items (spine=2): fits inside 48 KiB shared metadata arena",
         BookSpec(chapters=2, extra_manifest_items=350, toc_entries=2)),
    Case("shared-arena-over", "metadata-arena", "fast", "Ok", "OutOfMemory", "admit", "450 extra manifest items (spine=2, 456 entries < 1,024): exhausts 48 KiB shared arena",
         BookSpec(chapters=2, extra_manifest_items=450, toc_entries=2)),
    Case("central-dir-at", "central-dir", "fast", "Ok", "Ok", "admit", "Central directory exactly 1,048,576 B (1 MiB) across 64 entries",
         BookSpec(chapters=2, extra_entries=58, target_cd_bytes=MAX_CD_BYTES)),
    Case("central-dir-over", "central-dir", "fast", "Unsupported", "Unsupported", "admit", "Central directory 1,048,577 B (1 MiB + 1 B): refused by admission.rs",
         BookSpec(chapters=2, extra_entries=58, target_cd_bytes=MAX_CD_BYTES + 1)),
    Case("member-size-at", "member-size", "fast", "Ok", "Ok", "admit", "Chapter 0 exactly 4,194,304 B (4 MiB) uncompressed, deflated",
         BookSpec(chapters=2, first_chapter_bytes=MAX_MEMBER_BYTES)),
    Case("member-size-over", "member-size", "fast", "Unsupported", "Unsupported", "admit", "Chapter 0 is 4,194,305 B (4 MiB + 1 B) uncompressed: refused by zip_record_rules",
         BookSpec(chapters=2, first_chapter_bytes=MAX_MEMBER_BYTES + 1)),
    Case("expanded-total-at", "expanded-total", "fast", "Ok", "Ok", "admit", "17 deflated chapters totaling exactly 67,108,864 B (64 MiB) uncompressed",
         BookSpec(chapters=17, target_expanded_bytes=MAX_EXPANDED_BYTES)),
    Case("expanded-total-over", "expanded-total", "fast", "Unsupported", "Unsupported", "admit", "17 deflated chapters totaling 67,108,865 B (64 MiB + 1 B) uncompressed",
         BookSpec(chapters=17, target_expanded_bytes=MAX_EXPANDED_BYTES + 1)),
    Case("file-size-at", "file-size", "full", "Ok", "Ok", "admit", "Stored EPUB exactly 67,108,864 B (64 MiB) on disk (expanded < 64 MiB)",
         BookSpec(chapters=17, compress=False, target_file_bytes=MAX_FILE_BYTES)),
    Case("file-size-over", "file-size", "full", "Ok", "Unsupported", "admit", "Stored EPUB 67,108,865 B (64 MiB + 1 B) on disk (expanded < 64 MiB): open_phased refuses",
         BookSpec(chapters=17, compress=False, target_file_bytes=MAX_FILE_BYTES + 1)),
    Case("zip64-sentinel", "zip64", "fast", "Unsupported", "Unsupported", "admit", "ZIP64 EOCD record/locator with sentinel 0xFFFF/0xFFFFFFFF fields",
         BookSpec(chapters=2, zip64=True)),
    Case("name-len-511-subdir", "name-length", "fast", "Ok", "Ok", "admit", "511 B ZIP name under OEBPS/ (505 B href): at EPUB_MAX_MEMBER_NAME_BYTES",
         BookSpec(chapters=2, path_dir="OEBPS", first_chapter_href_len=505)),
    Case("name-len-href-512-oom", "name-length", "fast", "OutOfMemory", "OutOfMemory", "admit", "511 B ZIP name with 512 B relative OPF href (sub/../...): xml_input MAX_PATH reports OOM",
         BookSpec(chapters=2, path_dir="OEBPS", first_chapter_href_len=505, first_chapter_href_prefix="sub/../")),
    Case("name-len-512", "name-length", "fast", "Unsupported", "Unsupported", "admit", "512 B ZIP name under OEBPS/ (506 B href): one over 511 B name cap",
         BookSpec(chapters=2, path_dir="OEBPS", first_chapter_href_len=506)),
    Case("long-ids-fit", "long-ids", "full", "Ok", "Ok", "admit", "16 chapters with 200-byte manifest IDs",
         BookSpec(chapters=16, id_len=200)),
    Case("long-ids-over", "long-ids", "full", "Ok", "OutOfMemory", "admit", "160 chapters with 200-byte manifest IDs: exhausts 48 KiB shared metadata arena",
         BookSpec(chapters=160, id_len=200)),
    Case("toc-absent", "toc-size", "fast", "Ok", "Ok", "menu", "No authored Contents: section numbers without an error notice",
         BookSpec(chapters=2, toc_absent=True), expected_menu_valid=True, expected_authored=False, expected_menu_count=2, expected_fallback="None"),
    Case("toc-malformed", "toc-size", "fast", "Ok", "Ok", "menu", "Malformed authored Contents: usable section numbers with an explanation",
         BookSpec(chapters=2, toc_malformed=True), expected_menu_valid=True, expected_authored=False, expected_menu_count=2, expected_fallback="Invalid"),
    Case("toc-chapters-248", "toc-size", "fast", "Ok", "Ok", "menu", "Authored Contents at the shared-bank boundary",
         BookSpec(chapters=248), expected_menu_valid=True, expected_authored=True, expected_menu_count=248, expected_fallback="None"),
    Case("toc-chapters-249", "toc-size", "fast", "Ok", "Ok", "menu", "Authored Contents exceeds shared bank: explained section-number fallback",
         BookSpec(chapters=249), expected_menu_valid=True, expected_authored=False, expected_menu_count=249, expected_fallback="Capacity"),
    Case("parse-spine-276", "metadata-arena", "fast", "Ok", "Ok", "menu", "Required metadata at the parse-bank boundary",
         BookSpec(chapters=276), expected_menu_valid=True, expected_authored=False, expected_menu_count=276, expected_fallback="Capacity"),
    Case("parse-spine-277", "metadata-arena", "fast", "Ok", "OutOfMemory", "admit", "Required metadata exceeds the fixed parse bank",
         BookSpec(chapters=277)),
    Case("toc-nav-small", "toc-size", "fast", "Ok", "Ok", "menu", "EPUB 3 nav TOC with 32 entries (authored Contents)",
         BookSpec(chapters=32, toc_kind="nav", toc_entries=32), expected_menu_valid=True, expected_authored=True, expected_menu_count=32, expected_fallback="None"),
    Case("toc-mapping-at", "toc-size", "fast", "Ok", "Ok", "menu", "EPUB 3 nav TOC with 276 entries on 2 chapters: fits with_menu_mapping 1,104 B parse tail (276 authored + 1 fallback row)",
         BookSpec(chapters=2, toc_kind="nav", toc_entries=276), expected_menu_valid=True, expected_authored=True, expected_menu_count=277, expected_fallback="None"),
    Case("toc-mapping-over", "toc-size", "fast", "Ok", "Ok", "menu", "EPUB 3 nav TOC with 277 entries on 2 chapters: exceeds mapping scratch: generic Contents with a capacity notice",
         BookSpec(chapters=2, toc_kind="nav", toc_entries=277), expected_menu_valid=True, expected_authored=False, expected_menu_count=2, expected_fallback="Capacity"),
    Case("toc-nav-over-arena", "toc-size", "fast", "Ok", "Ok", "menu", "EPUB 3 nav TOC with 700 entries on 2 chapters: overflows 48 KiB shared arena, valid generic fallback",
         BookSpec(chapters=2, toc_kind="nav", toc_entries=700), expected_menu_valid=True, expected_authored=False, expected_menu_count=2, expected_fallback="Capacity"),
    Case("toc-ncx-nested", "toc-depth", "fast", "Ok", "Ok", "menu", "EPUB 2 NCX TOC nested 16 levels deep",
         BookSpec(chapters=4, toc_kind="ncx", toc_entries=20, toc_depth=16), expected_menu_valid=True, expected_authored=True, expected_menu_count=20, expected_fallback="None"),
    Case("toc-nav-nested", "toc-depth", "full", "Ok", "Ok", "menu", "EPUB 3 nav TOC nested 16 levels deep",
         BookSpec(chapters=4, toc_kind="nav", toc_entries=20, toc_depth=16), expected_menu_valid=True, expected_authored=True, expected_menu_count=20, expected_fallback="None"),
    Case("chapter-anchors-at", "chapter-anchors", "fast", "Ok", "Ok", "anchors", "192 anchors in chapter 0: at Profile::Small anchor cap",
         BookSpec(chapters=2, chapter_anchors=MAX_ANCHORS_SMALL), expected_anchors=MAX_ANCHORS_SMALL),
    Case("chapter-anchors-over", "chapter-anchors", "fast", "Ok", "Ok", "anchors", "210 anchors in chapter 0: excess past 192 silently dropped by PageCacheWriter",
         BookSpec(chapters=2, chapter_anchors=MAX_ANCHORS_SMALL + 18), expected_anchors=MAX_ANCHORS_SMALL),
    Case("chapter-pages-near-768", "chapter-pages", "full", "Ok", "Ok", "pages", "Single chapter ~717 pages: below 6-chunk (768-page) sync cache index",
         BookSpec(chapters=1, chapter_paragraphs=4300), expected_sync6=True, expected_small32=True, expected_pages_range=(501, 768)),
    Case("chapter-pages-over-768", "chapter-pages", "full", "Ok", "Ok", "pages", "Single chapter ~817 pages: exceeds 6-chunk (768-page) sync index, under 4,096 cap",
         BookSpec(chapters=1, chapter_paragraphs=4900), expected_sync6=False, expected_small32=True, expected_pages_range=(769, 4096)),
    Case("chapter-pages-near-4096", "chapter-pages", "full", "Ok", "Ok", "pages", "Single chapter ~4,001 pages (<4 MiB uncompressed): just below Profile::Small 4,096 cap",
         BookSpec(chapters=1, chapter_paragraphs=24000), expected_sync6=False, expected_small32=True, expected_pages_range=(3501, 4096)),
    Case("chapter-pages-over-4096", "chapter-pages", "full", "Ok", "Ok", "pages", "Single chapter ~4,250 pages (<4 MiB uncompressed): exceeds Profile::Small 4,096-page cap",
         BookSpec(chapters=1, chapter_paragraphs=25500), expected_sync6=False, expected_small32=False),
    Case("web-novel-envelope", "long-web-novel", "full", "Ok", "Ok", "menu", "Long text-only web novel inside current limits (240 chapters x 40 KiB = ~9.6 MiB)",
         BookSpec(chapters=240, chapter_bytes=40 * KIB, toc_kind="nav"), expected_menu_valid=True, expected_authored=True, expected_menu_count=240, expected_fallback="None"),
    Case("web-novel-2000ch", "long-web-novel", "full", "Unsupported", "Unsupported", "admit", "2,000-chapter plain-text web novel (~20 MiB text, 2,004 entries): refused at 1,024",
         BookSpec(chapters=2000, chapter_bytes=10 * KIB, toc_kind="nav")),
)
CASES_BY_NAME = {case.name: case for case in CASES}


def select_cases(*, subset: str = "fast", names: list[str] | None = None) -> list[Case]:
    if names:
        unknown = [n for n in names if n not in CASES_BY_NAME]
        if unknown:
            raise ValueError(f"unknown corpus cases: {', '.join(unknown)}")
        return [CASES_BY_NAME[n] for n in names]
    if subset == "fast":
        return [c for c in CASES if c.subset == "fast"]
    if subset == "full":
        return list(CASES)
    raise ValueError(f"unknown subset: {subset}")


def write_corpus(out_dir: Path, cases: list[Case]) -> list[dict]:
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = []
    for c in cases:
        data = build_epub(c.spec)
        path = out_dir / f"{c.name}.epub"
        path.write_bytes(data)
        manifest.append({**asdict(c), "path": str(path), "bytes": len(data),
                         "sha256": hashlib.sha256(data).hexdigest()})
    return manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--list", action="store_true", help="List cases, target limits, and expected outcomes")
    parser.add_argument("--out", type=Path, help="Output directory for generated EPUBs (never commit outputs)")
    parser.add_argument("--subset", choices=("fast", "full"), default="fast", help="Case subset (default: fast)")
    parser.add_argument("--case", action="append", dest="cases", help="Generate only the named case (repeatable)")
    parser.add_argument("--manifest", type=Path, help="Write JSON manifest of generated cases to this path")
    args = parser.parse_args(argv)

    selected = select_cases(subset=args.subset, names=args.cases)
    if args.list:
        for c in selected:
            print(f"{c.name:24s} [{c.subset:4s}] {c.axis:16s} admit={c.expected_admit:11s} open={c.expected_open:11s}  {c.description}")
        return 0
    if args.out is None:
        parser.error("--out is required unless --list is given")
    records = write_corpus(args.out, selected)
    if args.manifest is not None:
        args.manifest.write_text(json.dumps(records, indent=2) + "\n")
    for row in records:
        print(f"{row['name']:24s} {row['bytes']:10d} B  {row['sha256'][:12]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
