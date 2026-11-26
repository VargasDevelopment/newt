#!/usr/bin/env python3
"""Deterministic synthetic EPUB fixture generator for page-turn measurement (#168).

Generates three CC0 plain-prose / synthetic-PNG EPUBs on demand (never committed):
  - book-s.epub (S): short chapters (~10-15 pages each), including one declared-CSS chapter.
  - book-l.epub (L): one long chapter (~900-1,000 pages at default, ~1,100-1,152 at XL)
    plus a short Chapter 2 for crossing back into the end of Chapter 1.
  - book-i.epub (I): ~300-page chapter with valid synthetic PNG images placed near the
    128-page and 256-page segment edges, plus a short Chapter 2.

Usage:
  python3 tools/page_turn_fixtures.py --out /path/to/dir
  python3 tools/page_turn_fixtures.py --out /path/to/dir --books s l i
"""
from dataclasses import dataclass
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True

DOS_TIME = 0
DOS_DATE = ((2026 - 1980) << 9) | (11 << 5) | 1

# Original CC0 synthetic prose; never derived from any external book.
SENTENCES = (
    "A narrow footpath crossed the meadow toward the quiet creek.",
    "Small stones lined the water where clear ripples met the reeds.",
    "Morning mist lifted above the cedar ridge as a cool breeze arrived.",
    "Notes in a pocket journal recorded the distance between each milestone.",
    "A wooden bridge spanned the shallow ford beside two leaning willows.",
    "Lantern light flickered along the lane while evening clouds gathered.",
    "Simple maps marked every bend, orchard gate, and hillside spring.",
    "Calm weather kept the gravel trail dry from dawn until dusk.",
    "Timber rafters creaked softly as rain settled over the slate roof.",
    "An iron kettle hummed beside the hearth while cedar embers glowed.",
    "Fresh timber stacked beneath the eaves smelled faintly of pine resin.",
    "A brass compass rested on the map table near two folded letters.",
)

STYLE_CSS = (
    b"body { margin: 0; }\n"
    b"h1 { font-size: 1.25em; margin-bottom: 0.6em; }\n"
    b"p { text-indent: 1em; margin-top: 0.25em; margin-bottom: 0.25em; }\n"
    b".note { font-style: italic; margin-left: 1em; }\n"
)


def _png_chunk(kind: bytes, payload: bytes) -> bytes:
    crc = zlib.crc32(kind + payload) & 0xFFFFFFFF
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", crc)


def make_synthetic_png(width: int = 240, height: int = 160, variant: int = 0) -> bytes:
    """Build a valid, deterministic 8-bit grayscale PNG using stdlib zlib only."""
    rows = []
    for y in range(height):
        row = bytearray(1 + width)
        row[0] = 0  # filter type None
        for x in range(width):
            checker = ((x // 16) + (y // 16) + variant) & 1
            border = x < 4 or x >= width - 4 or y < 4 or y >= height - 4
            row[1 + x] = 24 if border else (64 if checker else 224)
        rows.append(bytes(row))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    idat = zlib.compress(b"".join(rows), level=6)
    return (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", ihdr)
        + _png_chunk(b"IDAT", idat)
        + _png_chunk(b"IEND", b"")
    )


def _paragraph(seed: int, chapter: int, index: int, anchor_id: str | None = None) -> str:
    base = (seed * 131 + chapter * 37 + index * 17) % len(SENTENCES)
    text = " ".join(SENTENCES[(base + k) % len(SENTENCES)] for k in range(3))
    if anchor_id is not None:
        return f'<p id="{anchor_id}">Section {index // 40 + 1}. {text}</p>\n'
    if index % 40 == 0:
        return f'<p id="s{chapter:02d}_{index:05d}">Section {index // 40 + 1}. {text}</p>\n'
    return f"<p>{text}</p>\n"


def _chapter_xhtml(
    title: str,
    chapter_index: int,
    paragraphs: int,
    *,
    seed: int,
    stylesheet: str | None = None,
    images_at: dict[int, tuple[str, str]] | None = None,
    anchors_at: dict[int, str] | None = None,
) -> bytes:
    css = (
        f'<link rel="stylesheet" type="text/css" href="{stylesheet}"/>'
        if stylesheet
        else ""
    )
    chunks = [
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" lang="en">'
        f"<head><title>{title}</title>{css}</head><body>\n"
        f"<h1>{title}</h1>\n"
    ]
    for i in range(paragraphs):
        if images_at and i in images_at:
            href, alt = images_at[i]
            chunks.append(f'<p><img src="{href}" alt="{alt}"/></p>\n')
        anchor = anchors_at.get(i) if anchors_at else None
        chunks.append(_paragraph(seed, chapter_index, i, anchor_id=anchor))
    chunks.append("</body></html>\n")
    return "".join(chunks).encode("utf-8")


def _write_deterministic_zip(entries: list[tuple[str, bytes, bool]]) -> bytes:
    """Serialize a deterministic ZIP archive with fixed timestamps and no ZIP64."""
    local_parts = []
    central_parts = []
    offset = 0
    for name, data, compress in entries:
        name_bytes = name.encode("utf-8")
        crc = zlib.crc32(data) & 0xFFFFFFFF
        if compress:
            comp = zlib.compressobj(level=6, method=zlib.DEFLATED, wbits=-15)
            payload = comp.compress(data) + comp.flush()
            method = 8
        else:
            payload = data
            method = 0
        local = (
            struct.pack(
                "<IHHHHHIIIHH",
                0x04034B50,
                20,
                0,
                method,
                DOS_TIME,
                DOS_DATE,
                crc,
                len(payload),
                len(data),
                len(name_bytes),
                0,
            )
            + name_bytes
            + payload
        )
        central = (
            struct.pack(
                "<IHHHHHHIIIHHHHHII",
                0x02014B50,
                20,
                20,
                0,
                method,
                DOS_TIME,
                DOS_DATE,
                crc,
                len(payload),
                len(data),
                len(name_bytes),
                0,
                0,
                0,
                0,
                0,
                offset,
            )
            + name_bytes
        )
        local_parts.append(local)
        central_parts.append(central)
        offset += len(local)
    cd = b"".join(central_parts)
    eocd = struct.pack(
        "<IHHHHIIH",
        0x06054B50,
        0,
        0,
        len(entries),
        len(entries),
        len(cd),
        offset,
        0,
    )
    return b"".join(local_parts) + cd + eocd


def _build_epub(
    book_id: str,
    title: str,
    chapters: list[tuple[str, str, bytes]],
    extras: list[tuple[str, str, bytes]] | None = None,
    toc_entries: list[tuple[str, str]] | None = None,
) -> bytes:
    extras = extras or []
    toc = toc_entries if toc_entries is not None else [(href, label) for href, label, _ in chapters]
    nav_items = "\n".join(
        f'<li><a href="{href}">{label}</a></li>' for href, label in toc
    )
    nav_xhtml = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" '
        'xmlns:epub="http://www.idpf.org/2007/ops" lang="en">'
        "<head><title>Contents</title></head><body>\n"
        f'<nav epub:type="toc"><h1>Contents</h1><ol>\n{nav_items}\n</ol></nav>'
        "</body></html>\n"
    ).encode("utf-8")

    manifest = [
        '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
    ]
    spine = []
    for idx, (href, _, _) in enumerate(chapters):
        manifest.append(
            f'<item id="c{idx:02d}" href="{href}" media-type="application/xhtml+xml"/>'
        )
        spine.append(f'<itemref idref="c{idx:02d}"/>')
    for idx, (href, media_type, _) in enumerate(extras):
        manifest.append(f'<item id="x{idx:02d}" href="{href}" media-type="{media_type}"/>')

    opf = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">\n'
        '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
        f'<dc:identifier id="uid">urn:newt:fixture:{book_id}</dc:identifier>\n'
        f"<dc:title>{title}</dc:title>\n"
        "<dc:language>en</dc:language>\n"
        '<meta property="dcterms:modified">2026-11-01T00:00:00Z</meta>\n'
        "</metadata>\n"
        "<manifest>\n"
        + "\n".join(manifest)
        + "\n</manifest>\n<spine>\n"
        + "\n".join(spine)
        + "\n</spine>\n</package>\n"
    ).encode("utf-8")

    container = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
        "<rootfiles>"
        '<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>'
        "</rootfiles></container>\n"
    ).encode("utf-8")

    entries: list[tuple[str, bytes, bool]] = [
        ("mimetype", b"application/epub+zip", False),
        ("META-INF/container.xml", container, True),
        ("OEBPS/content.opf", opf, True),
        ("OEBPS/nav.xhtml", nav_xhtml, True),
    ]
    for href, _, data in chapters:
        entries.append((f"OEBPS/{href}", data, True))
    for href, _, data in extras:
        entries.append((f"OEBPS/{href}", data, True))
    return _write_deterministic_zip(entries)


@dataclass(frozen=True)
class FixtureSpec:
    key: str
    filename: str
    title: str
    description: str


SPECS = {
    "s": FixtureSpec(
        key="s",
        filename="book-s.epub",
        title="Synthetic Short Chapters (S)",
        description="Six short chapters (~10-15 pages each); chapter 2 declares CSS.",
    ),
    "l": FixtureSpec(
        key="l",
        filename="book-l.epub",
        title="Synthetic Long Chapter (L)",
        description="Long chapter 1 (~900-1,000 pp default, ~1,100-1,152 pp XL) plus short chapter 2.",
    ),
    "i": FixtureSpec(
        key="i",
        filename="book-i.epub",
        title="Synthetic Edge Images (I)",
        description="~300-page chapter with synthetic PNGs near the 128-page and 256-page segment edges.",
    ),
}

# Calibrated paragraph counts for the pinned X3 default (Px30) and XL (Px34) layouts.
# Roughly 2.80 paragraphs/page at Px30 (~929 pp for L, ~302 pp for I) and ~1,135 pp for L at Px34.
SHORT_CHAPTER_PARAGRAPHS = (34, 38, 30, 36, 32, 35)
LONG_CHAPTER_PARAGRAPHS = 2600
LONG_TAIL_CHAPTER_PARAGRAPHS = 24
IMAGE_CHAPTER_PARAGRAPHS = 840
IMAGE_EDGE_PARAGRAPHS = {
    356: ("img-edge-128.png", "Synthetic plate near segment 1 boundary"),
    712: ("img-edge-256.png", "Synthetic plate near segment 2 boundary"),
}
IMAGE_EDGE_ANCHORS = {
    350: "near-edge-128",
    706: "near-edge-256",
}


def build_book(key: str) -> bytes:
    if key == "s":
        chapters = []
        for idx, count in enumerate(SHORT_CHAPTER_PARAGRAPHS):
            css = "style.css" if idx == 1 else None
            data = _chapter_xhtml(
                f"Short Chapter {idx + 1}",
                idx,
                count,
                seed=11,
                stylesheet=css,
            )
            chapters.append((f"c{idx:02d}.xhtml", f"Short Chapter {idx + 1}", data))
        return _build_epub(
            "page-turn-s",
            SPECS["s"].title,
            chapters,
            extras=[("style.css", "text/css", STYLE_CSS)],
        )
    if key == "l":
        ch1 = _chapter_xhtml("Long Chapter 1", 0, LONG_CHAPTER_PARAGRAPHS, seed=23)
        ch2 = _chapter_xhtml("Short Chapter 2", 1, LONG_TAIL_CHAPTER_PARAGRAPHS, seed=23)
        return _build_epub(
            "page-turn-l",
            SPECS["l"].title,
            [
                ("c00.xhtml", "Long Chapter 1", ch1),
                ("c01.xhtml", "Short Chapter 2", ch2),
            ],
            toc_entries=[
                ("c00.xhtml", "Long Chapter 1"),
                ("c00.xhtml#s00_00400", "Deep Section 1"),
                ("c00.xhtml#s00_01400", "Deep Section 2"),
                ("c01.xhtml", "Short Chapter 2"),
            ],
        )
    if key == "i":
        ch1 = _chapter_xhtml(
            "Illustrated Chapter 1",
            0,
            IMAGE_CHAPTER_PARAGRAPHS,
            seed=37,
            images_at=IMAGE_EDGE_PARAGRAPHS,
            anchors_at=IMAGE_EDGE_ANCHORS,
        )
        ch2 = _chapter_xhtml("Afterword", 1, 20, seed=37)
        extras = [
            ("img-edge-128.png", "image/png", make_synthetic_png(240, 160, variant=0)),
            ("img-edge-256.png", "image/png", make_synthetic_png(240, 160, variant=1)),
        ]
        return _build_epub(
            "page-turn-i",
            SPECS["i"].title,
            [
                ("c00.xhtml", "Illustrated Chapter 1", ch1),
                ("c01.xhtml", "Afterword", ch2),
            ],
            extras=extras,
            toc_entries=[
                ("c00.xhtml", "Illustrated Chapter 1"),
                ("c00.xhtml#near-edge-128", "Near Edge 128"),
                ("c00.xhtml#near-edge-256", "Near Edge 256"),
                ("c01.xhtml", "Afterword"),
            ],
        )
    raise ValueError(f"Unknown fixture key: {key}")


def generate(out_dir: Path, keys: list[str]) -> dict:
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = {"books": {}}
    for key in keys:
        spec = SPECS[key]
        data = build_book(key)
        dest = out_dir / spec.filename
        dest.write_bytes(data)
        manifest["books"][key] = {
            "filename": spec.filename,
            "title": spec.title,
            "description": spec.description,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="output directory for generated EPUBs")
    parser.add_argument("--books", nargs="+", choices=sorted(SPECS), default=["s", "l", "i"])
    args = parser.parse_args()
    manifest = generate(args.out, args.books)
    for key, info in manifest["books"].items():
        print(f"{key}: {info['filename']} ({info['bytes']} B, sha256={info['sha256'][:16]}...)")


if __name__ == "__main__":
    main()
