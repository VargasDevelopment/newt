# Rust third-party notices

SDK paths below are relative to `vendor/freeink-sdk` at the pinned FreeInk
commit `cd6f5b831c8fad899723b6d406586f00c575f021`. Copyright (c) 2026 FreeInk;
its MIT terms are in `vendor/freeink-sdk/LICENSE`, with upstream attribution in
`vendor/freeink-sdk/NOTICE`. Newt-derived code is MIT under the root `LICENSE`.

| Derived Rust module(s) | Upstream at that SDK commit | Licence |
| --- | --- | --- |
| `core/src/xml_input.rs` | `libs/book/FreeInkBook/src/text/EntityFilter.cpp`, `src/epub/XmlUtil.h`, `src/epub/PackageParsers.cpp` | FreeInk MIT |
| `core/src/xml_session.rs`, `target-expat` | `libs/book/FreeInkBook/src/epub/XmlSax.cpp`, `include/epub/XmlSax.h`; native Expat | FreeInk MIT; Expat MIT |
| `core/src/package_parsers.rs`, `package_workflow.rs` | `libs/book/FreeInkBook/src/epub/PackageParsers.cpp`, `include/epub/PackageParsers.h` | FreeInk MIT |
| `core/src/zip_container.rs` | `libs/book/FreeInkBook/src/epub/ZipCatalog.cpp`, `include/epub/ZipCatalog.h` | FreeInk MIT |
| `core/src/zip_window/source.rs` | `libs/book/FreeInkBook/include/BookStorage.h` (read/size contract); Newt validation policy | FreeInk MIT; Newt MIT |
| `core/src/zip_window/arena.rs` | `libs/font/FreeInkFont/include/FontArena.h` | FreeInk MIT |
| `core/src/css.rs` | `libs/book/FreeInkBook/src/css/Css.cpp`, `include/css/Css.h` | FreeInk MIT |
| `core/src/hyphenator.rs` | `libs/book/FreeInkBook/src/text/Hyphenator.cpp`, `include/text/Hyphenator.h` | FreeInk MIT; pattern terms below |
| `core/src/font_layout.rs` | `libs/font/FreeInkFont/src/Gpos.cpp`, `src/Gsub.cpp` and matching headers | FreeInk MIT |
| `core/src/ft_font.rs` | `libs/font/FreeInkFont/src/FtFont.cpp`, `include/FtFont.h`; native FreeType | FreeInk MIT; FreeType FTL |
| `core/src/image_probe.rs` | `libs/book/FreeInkBook/src/epub/ImageProbe.cpp`, `include/epub/ImageProbe.h` | FreeInk MIT |
| `core/src/page_renderer.rs` | `libs/book/FreeInkBook/src/render/PageRenderer.cpp`, `include/render/PageRenderer.h` (including FontChain) | FreeInk MIT |
| `core/src/chapter_layout/` | `libs/book/FreeInkBook/src/layout/ChapterLayout.cpp`, `include/layout/ChapterLayout.h` | FreeInk MIT; Unicode data below |
| `core/src/pngle.rs`, `pngle/` | `libs/book/FreeInkBook/third_party/pngle/pngle.c`; `src/render/ImageRenderer.cpp` | pngle MIT; FreeInk MIT |
| `core/src/jpeg/` | `libs/book/FreeInkBook/third_party/tjpgd/tjpgd.c`; `src/render/ImageRenderer.cpp` | ChaN terms below; FreeInk MIT |
| `core/src/inflate.rs`, `inflate/` | `libs/book/FreeInkBook/third_party/miniz/miniz.c`, `miniz.h` | miniz public domain/Unlicense or MIT |

The remaining Newt-derived policy, storage, UI, phased-reader, reader-navigation
and firmware modules follow Newt `bc9ffda52b85f277e46c63718ac88d62fba5d368`,
with the documented alpha.16 catch-up at `9c627fe5`; their licence is the root
`LICENSE`. This includes `core/src/utf8.rs` (`include/newt/utf8.h`),
the source adapters (`firmware/sd_book_source.h`), admission/canonical
and cache/producer policy (`firmware/`), and `reader-navigation/src/lib.rs`
(`firmware/reader_navigation.h`). Original host fixtures and transport are Newt MIT. All Newt MIT notices use the
root `LICENSE`; duplicate packet notices are omitted.

Canonical dependency terms and retained notices:

- Expat: `vendor/freeink-sdk/libs/book/FreeInkBook/third_party/expat/COPYING`.
- miniz: `vendor/freeink-sdk/libs/book/FreeInkBook/third_party/miniz/LICENSE`.
- pngle, Copyright 2019 kikuchan: `vendor/freeink-sdk/libs/book/FreeInkBook/third_party/pngle/LICENSE`.
- Hyphenation patterns, Copyright 1990, 2004, 2005 Gerard D.C. Kuiken:
  `vendor/freeink-sdk/libs/book/FreeInkBook/third_party/hyphen-patterns/LICENSE`.
- FreeType: `vendor/freeink-sdk/libs/font/FreeInkFont/third_party/freetype/FTL.TXT`
  and `vendor/freeink-sdk/libs/font/FreeInkFont/third_party/freetype/LICENSE.TXT`.
  This software is based in part on the work of the FreeType Team.
- TJpgDec, Copyright (C) 2021 ChaN: the full notice remains in
  `rust/tests/jpeg_decoder/NOTICE` and canonical `third_party/tjpgd/tjpgd.c`.
- Arabic shaping: Unicode License v3, retained in
  `rust/tests/chapter_layout/licenses/UNICODE-LICENSE-V3.txt`; canonical data and
  attribution are under `libs/book/FreeInkBook/third_party/unicode/`.
- Generated reader/phased font data retain the OFL terms and attribution under
  `rust/reader-atlas/licenses/` and `rust/phased-sink/licenses/`.

JSON provenance records retain historical source identities and digests when
copies retire. Command receipts spell repository paths relative to its root;
Newt MIT licence citations resolve to the root `LICENSE`.
