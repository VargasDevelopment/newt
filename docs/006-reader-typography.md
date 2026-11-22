# Decision 006: a bounded real reading font before native Frosted gray

October 1, 2026. Build on `spike/reader-menu-settings` at `5d8d45f`, preserving its exact firmware and all earlier checkpoints. This slice chooses typography because it can improve the real shared EPUB page without adding an unverified panel waveform or runtime font heap.

The approved round5 reading reference uses a baked Lexend Deca regular atlas. Only printable ASCII 32–126 is extracted into the derived **Newt Reading Atlas** with its SIL OFL 1.1 notice. The pinned source generator selects the first eligible face containing a glyph: Lexend comes first and the restricted fallback intervals exclude printable ASCII. The mixed-font full source header and CrossInk implementation code are not imported. This remains a FreeInk-based reader/application platform; a licensed font data asset does not change the platform foundation.

[Pinned glyph source](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/builtinFonts/lexenddeca_14_regular.h) SHA-256 `14e0fcd4fda8857e54184088c026c21da276a97c3c90aa56812169b86c8de107`; [pinned source generator](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/scripts/fontconvert.py). The local design reference and its license were inspected read-only. Printable-ASCII bitmap bytes hash to `a74a165ff3448e250643132ca3f74a4d220bb0c71ec607851ccb91153747e907`.

The source's literal 14pt profile was generated at 150 DPI, approximately 29.17 em pixels. Its baked ascent is 30 pixels and baseline step is 36. Newt's internal nominal size tag of 30 selects those exact baked metrics; it does not redefine 14pt as 30 SDK pixels. The reader's first baseline is margin 5 + ascent 30. Proportional advances and kerning come from the same data as drawing. The pair adjustment preserves the reference's combined sixteenth-pixel rounding, including cases where separately rounding kerning would change placement. FreeInk layout and drawing reset kerning consistently at run boundaries.

Keep 2-bit coverage in read-only flash; the existing monochrome path thresholds it at coverage 2/3. No runtime decompression, TTF rasterizer, glyph cache, extra framebuffer or font allocation is needed. This is a monochrome projection of the selected glyphs, not native antialiasing or full Frosted rendering. Sized headings use bounded nearest resampling of the fixed regular atlas; there are no real bold/italic faces. Unsupported Unicode still has an honest question-mark fallback. System/menu/footer typography remains separate and unchanged.

Native gray/Frosted blur, full Unicode/font selection, real AA, complete paragraph-preset parity and native input/display quality remain later work. Pinned FreeInk can drop soft hyphens while measurement and rendering retain different kerning predecessors; the immutable ASCII fixture is unaffected, but general EPUB shaping needs a focused regression and SDK-boundary resolution before claiming parity. The mock's authored paragraphs use different spacing than FreeInk's current defaults; do not call this a full reproduction of the owner's reported preset. Real EPUB pagination may differ from synthetic mock paragraphs. Semantic anchors preserve reading location across reflow; page counts are recalculated rather than copied from older screenshots.

Before a typography hardware trial, complete the preserved baseline bring-up. Then review this candidate's exact image against the observed app slot, recovery backups and unit identity. Compare a photograph of the cracked X3's real reading page with the archived host PNG at the same semantic anchor. Check glyph shape, thresholded stroke legibility, polarity, margins, footer separation, chapter boundaries and scaled heading clipping in both reader orientations. Record actual free/largest heap and repeated page/menu/sleep behavior. No SD/font-file access or waveforms are added by this slice.


```sh
python3 tools/make-reader-font.py --check
python3 tests/reader_font_proof_test.py
python3 tools/check.py
python3 tools/firmware-build.py --clean
python3 tools/preview.py --screen epub
python3 tools/preview.py --screen epub --reader-orientation portrait
```

[Landscape reading](../artifacts/typography-reading.png) · [Next page](../artifacts/typography-reading-next.png) · [Portrait reading](../artifacts/typography-portrait.png) · [Menu over real text](../artifacts/typography-menu.png) · [Exact glyph proof](../artifacts/typography-font-proof.png). The independent oracle pins all 95 glyphs, 899 kerning pairs and the bitmap, then compares all packed pixels of two known lines. Reader tests separately compare official SDK placement, exercise long words and scaled headings, and verify anchors/failure paths in both orientations. Host checks and clean C3 compilation pass; both bounded reviews approve the defined fixture scope.

The font tables occupy 11,871 bytes of read-only flash. Reader workspace is unchanged at 69,968 bytes on C3 / 70,088 on host. Arena peaks remain 694 / 48,328 / 2,624 bytes; Expat's separate temporary requested-payload peak remains 10,024 bytes and is released. The sample reflows to four pages per chapter. All runtime timing, heap and optical claims await hardware.

Compiled source **`86d13c072998d5a5885659903764b5b4d08e55f3`**; `.build/typography-86d13c0/firmware.bin` is **657,456 bytes**, SHA-256 `fe584741e6bfa764efb2bf01c056a1b13eaad76a25cfa3646f37b573dda16779`. This is +12,720 bytes over the preserved menu candidate. PlatformIO static RAM remains 71,372 bytes; linked IDF DRAM remains 131,460 bytes with 189,836 bytes not linked. Those values are not runtime free heap. [Exact receipt](../artifacts/typography-verification.json) includes source, licensed assets, ELF/map/image metadata, previews, logs and all six preserved image hashes.


## 2026-10-18: NEWT-3 full source-font coverage

The candidate preserves all original 341 glyphs and kerning per face, and adds
1,700 Unicode-block-selected glyphs to each of four faces (2,041 total). Source
imports verify all 17 approved pins. Lexend and matching Noto Sans come first;
Math leads symbol fallback for arrows/operators, Symbols for miscellaneous/music,
and Symbols 2 for geometric shapes. These are generic per-block manifest rules,
with source decisions recorded per glyph and a reproducible source comparison.
All arrows, math operators, miscellaneous symbols and U+2669–U+266F are covered.
CJK, combining sequences, Greek Extended and complex shaping remain outside scope.

FreeType 2.14.2 reproduces all 341 legacy glyphs in all four faces exactly
with `FT_LOAD_RENDER | FT_LOAD_NO_BITMAP | FT_LOAD_NO_AUTOHINT` (32780).
The earlier 338/339 Regular/Bold mismatches were caused by allowing automatic
hinting fallback; no historical FreeType-version explanation is needed.
The same validated size/DPI/flags/threshold/rounding profile applies to every
source. Any declared-profile deviation or source reproduction mismatch fails
before publication. The legacy assets/prefix remain unchanged compatibility
fixtures, checked against the same raster path used for additions.

Source bearings, ascent 30, line height 36 and margins remain unchanged by Lead
ruling. Ink overlap beyond the nominal box is accepted font behavior, not a reason
to fit or reposition glyphs. Sanitized host regressions must prove viewport/frame
safety and fresh/cache/repaint equality, with visible clipping reported explicitly.
The per-face/block outliers and pixel examples are in the font README.

Added tables cost 638,817 bytes before descriptors/alignment/code. The Lead reviewed
the earlier 600 KiB checkpoint and authorized continued measurement under 700 KiB
per-variant image growth. No resource ceiling or protected budget record changed.
The corrected-profile substring-pooling diagnostic would save 29,399 bytes; the current
runtime/table format is retained. See [the font README](../assets/fonts/README.md)
for exact per-block/face costs, pins, coverage, source proof and limitations.
Hardware optical/runtime acceptance remains a separate NEWT-9 step.
