# Read-only CrossInk passage preview

Trying Newt should preserve the existing reader's library and state. This slice
provides a host preview for one explicitly chosen book and saved passage. It
neither applies progress nor imports settings, and it never opens credentials.
The proposed position always needs human review. Firmware and SD are untouched.

## Verified formats and provenance

The compatibility evidence is the official `uxjulia/CrossInk` repository, MIT.
The initial historical snapshot was
`9656361d3329af43143babf1319f8342fe7d5570`. Release tags were subsequently
verified on October 2, 2026 UTC: 1.5.1 at
`4f2da6b7777b9a38fdf2606796d6da34caa7d2cb` and 1.6.0 at
`b25beb13761d5851f98e7eada09aa5e7d430df48`. Newt still
uses official FreeInk `cd6f5b831c8fad899723b6d406586f00c575f021`; no CrossInk
runtime, SDK or fetched software was executed or added as a dependency.
The decoder is an independent small implementation of the documented serialized
fields. Authored fixtures are CC0; no owner book/cache text is in source history.

| Evidence | Supported preview behavior |
| --- | --- |
| [1.5.1 progress helper](https://github.com/uxjulia/CrossInk/blob/9656361d3329af43143babf1319f8342fe7d5570/src/activities/reader/EpubReaderUtils.h) | Exactly 4, 6 or 10 bytes: LE u16 zero-based spine/page; optional u16 page count; optional u32 decoded visible-source-codepoint offset. The 65535 page sentinel means page zero. Conflicting primary/backup records stop the preview. |
| [1.5.1 section cache](https://github.com/uxjulia/CrossInk/blob/9656361d3329af43143babf1319f8342fe7d5570/lib/Epub/Epub/Section.cpp) | Full schema 66 only, magic `FF C X S`, sequential ESP32 PODs, 53-byte header. Page count at byte 27, page LUT pointer at 33, visible-offset LUT pointer at 49. LUT bounds/order, page bounds and agreement with saved progress are checked. |
| [Page serialization](https://github.com/uxjulia/CrossInk/blob/9656361d3329af43143babf1319f8342fe7d5570/lib/Epub/Epub/Page.cpp) and [TextBlock](https://github.com/uxjulia/CrossInk/blob/9656361d3329af43143babf1319f8342fe7d5570/lib/Epub/Epub/blocks/TextBlock.cpp) | At most eight consecutive leading text elements are decoded as a passage witness, including word-metadata arena sizing, strict UTF-8 and NUL/offset validation. This is a prefix decoder, not a complete page/cache validator. Non-text prefixes, ruby and RTL blocks are unsupported. |
| [Legacy character callback](https://github.com/uxjulia/CrossInk/blob/9656361d3329af43143babf1319f8342fe7d5570/lib/Epub/Epub/parsers/ChapterHtmlSlimParser.cpp) | Visible offsets count decoded source codepoints before whitespace collapse. Skipped publisher pagebreaks, hidden CSS content, ruby and synthetic content alter what is counted or rendered. They cannot simply become Newt anchors. |
| Newt pinned `ChapterLayout.h` and actual `ChapterLayoutSession` | `Page.charStart` counts extracted codepoints after the SDK's collapse/entity handling. The host witness uses that actual value and actual page runs. Null ZIP, embedded styles off, landscape and the current reader font match the bounded text-only profile. |

The explicit `--crossink-version` selector defaults to 1.5.1 for the verified
owner backup; it never infers an installed release from a header or filename.
1.5.1 requires full section schema 66; 1.6.0 requires full schema 77. Unknown
releases, mismatched schemas and partial caches (0xF6/0xF3) fail closed. This is
compatibility for the bounded leading-text prefix, not a complete cache decoder,
settings importer or full-release compatibility promise.

The pinned [1.6.0 progress helper](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/activities/reader/EpubReaderUtils.h)
and [TextBlock serializer](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/Epub/Epub/blocks/TextBlock.cpp)
are byte-identical to the inspected 1.5.1 files. The
[section header](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/Epub/Epub/Section.cpp)
retains the same 53-byte sequential POD layout and LUT offsets. The
[page serializer](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/Epub/Epub/Page.cpp)
retains its element-count and text-tag prefix; new trailing page features are
outside the prefix decoder. The source-codepoint coordinate remains distinct
from Newt's extracted text even where these byte formats agree.

SHA-256 of the inspected historical source documents:

| Source | SHA-256 |
| --- | --- |
| `EpubReaderUtils.h` | `bb73b5a6a119c5cb7cc2755b47c6f0ef350dda655928495856d3e3ac9481d926` |
| `Section.cpp` | `df4c6b8a955fb1d1d4281d7e0ff2051146ceefdafee57d8b11795ce1a7002130` |
| `Page.cpp` | `5cf08091fc3dd952f714c2816ba270839912d3b3eef6d435992d7cb727c3f37a` |
| `TextBlock.cpp` | `4f2dca5ccbb368aa9f16543d9dbc718d60bfd7e55bd46f97234fbb8a56e420ec` |
| `BlockStyle.h` | `f06918020096411559395ce7671e01ff6b8265482a7bd85af9bca5f7a45a78e9` |
| `CssStyle.h` | `2c3937c9e31dcc937ff16bc0ff6118d8f97ee19d526212371f3f8fdd8dfeb5b6` |
| `ChapterHtmlSlimParser.cpp` | `4251ac71c95272832de578470f7a7fb5edb25011ace363f3b5aedf67a6df3717` |
| `LICENSE` | `7212963b5b1b5ee34ca158163356beb1e6945dd2cc5d320f1fe7573c424ea640` |

Additional 1.6.0 source SHA-256: `Section.cpp`
`717bf74c863d517936550a4576fab45b1220420c6fcd40cc29e167466283d46c`,
`Page.cpp` `9d532bb2f7992453de05f1376d8ebc11fae9a5b6a811eb1ce0d95666f12ee1d6`,
`Page.h` `69390ca533fbba1fef10cce292e0dacadc2109eb19e0940e107fd1da2608a76a`,
`ChapterHtmlSlimParser.cpp`
`f3049a2942d3fac413e2141d7fe32855316ba71c18b7965d373d9642c01542ac`.
The 1.5.1 tagged progress/section/page/TextBlock files also match the historical
hashes above. These are publicly fetched source bytes; no upstream runtime was
executed or added as a dependency.

## What the proposal proves

The source and explicitly selected target EPUB must both match an expected full
SHA-256. A moved/renamed identical file works; a changed edition stops. Duplicate
content in a library is an explicit selection ambiguity. The legacy spine is
resolved through the book's actual package and compared with the SDK-selected
chapter href. The selected XHTML digest is retained in the private proposal.
Book identity is checked again after witness generation. Before Python ZIP
catalog allocation, the preview validates a single-disk ordinary EOCD and walks
at most 1,024 central member records in at most 1 MiB of central metadata. ZIP64
EOCDs, overlong member names, trailing directory records and count disagreement
are unsupported. Individual metadata/chapter reads are capped at 4 MiB, cache
evidence at 16 MiB and private SDK output at 8 MiB. These are host preview
bounds, not claims about device heap.

The standalone preview builds its helper on every invocation through
`tools/trusted_host.py`. Before compiling it verifies the actual SDK commit,
official origin and a clean checkout, including untracked or ignored inputs.
Actual tracked bytes must match committed blobs even when Git index flags hide
a modification from status.
The executable and SDK objects live in a fresh owner-only temporary directory;
existing `.build/migration-anchor` executables and cached SDK objects are never
accepted as migration evidence. The private proposal binds SDK, reviewed build
procedure/source bytes, compiler identity and actual executable SHA-256. Those
inputs and executable are checked again after witness generation and before a
proposal is published. A helper's embedded revision label alone grants no trust.
This requires the existing local compiler and pinned checkout, and acquires no
dependency or hardware access. A changed input stops the preview.

`tools/migration-anchor.cpp` streams the chapter with the actual pinned SDK and
emits private UTF-8 page runs and actual page-start anchors. It uses separate
48-KiB parse/layout arenas, with metadata destroyed before arena reuse. Its
witness is bounded to 4,096 pages and 64 KiB of text per page, and an existing
output is never overwritten. No fonts/glyphs are synthesized by the witness.

Matching preserves non-Latin text and combining marks. NFC canonical spelling,
Unicode whitespace and invisible soft hyphens are normalized only for passage
comparison. Those comparison-string lengths never become character anchors.
Occurrences, including overlapping occurrences, must be unique across the whole
chapter. A short leading legacy word requires a longer unique corroborating
passage in the next 512 comparison characters. Intermediate publisher labels may
differ between engines; the proposal remains explicitly approximate. The chosen
anchor is the actual earlier SDK page containing the leading passage, including
when a long passage spans pages. No within-page offset is guessed.

A private owner-backup run verified agreement of primary/backup progress, the
schema-66 page LUT and the selected saved offset. It matched the cached leading
passage uniquely against actual pinned SDK text, with nearby longer prose
corroboration. The proposal remains `approximate_review_required` and
`automatic_apply: false`. Private book identity, passage, paths and position are
kept outside Git. This is host evidence, not a device reading test.

The exact source-offset-to-extracted-codepoint conversion remains unsupported.
A future exact migration needs an actual engine source-coordinate witness or a
verified cross-engine text map, plus restored visible-passage acceptance. Glyph
coverage also remains a separate requirement: preserving a character in SDK
runs does not mean the current font can draw it. Settings, statistics, caches,
full-release compatibility, credentials and automatic progress installation are
outside this slice.

## Reproducible developer checks

```sh
python3 tests/trusted_host_test.py
python3 tests/crossink_migration_test.py
```

The focused tests build the helper with the existing local host compiler and
ASan/UBSan, exercise authored 4/6/10-byte progress and schema-66/77 cache fixtures,
and run an original deflated Unicode EPUB through actual SDK layout. They cover
combining characters, non-Latin text, entities, whitespace, soft hyphens, spine
bounds, changed/renamed/duplicate content, missing/ambiguous passages and atomic
private no-overwrite publication. No private book or connected device is read by
these tests. Root integrates this test into the full repository gate.

An agent can then create a preview using explicitly selected local paths:

```sh
python3 tools/crossink_migration.py \
  --source-epub /private/source/book.epub \
  --crossink-version 1.5.1 \
  --expected-sha256 EXPECTED_FULL_BOOK_SHA256 \
  --target-epub /private/trial/book.epub \
  --progress /private/source/progress.bin \
  --backup /private/source/progress.bin.bak \
  --section /private/source/chapter-layout.bin \
  --output /private/new-preview.json
```

The output path must be new and outside preserved source state. Publication is
an exclusive atomic link from an owner-readable temporary file; there is no
replacement option. Errors print only a generic status. The private JSON records
the precise supported/unsupported reason, book/chapter identity and reviewable
passage. It is a preview artifact, not runtime progress. People should eventually
see “Try my existing library” with a passage confirmation; these parameters are
agent plumbing, not the intended product onboarding experience.
