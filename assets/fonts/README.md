# NewtReadingAtlas

All four immutable reading faces cover **2,041 codepoints**, including **1,700**
block-defined additions to the original 341 glyphs per face. All seven music
scalars U+2669–U+266F, all 112 arrows U+2190–U+21FF, all 256 mathematical
operators U+2200–U+22FF and all 256 miscellaneous symbols U+2600–U+26FF are
covered. This is precomposed coverage, not full Unicode shaping.

## Pinned sources, licensing and generation

[`sources.json`](sources.json) records all 17 acquired files, exact URLs, commits
and SHA-256 pins, ordered source entries, per-block order and rendering parameters.
Every hash is verified before source parsing. No downloaded script is executed
or copied. TTFs stay outside the repository; source mode loads system FreeType
only through the independently authored ctypes adapter.

| Acquired source | Exact source URL | SHA-256 |
| --- | --- | --- |
| crossink-lexend-regular/LexendDeca-Regular.ttf | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/builtinFonts/source/LexendDeca/LexendDeca-Regular.ttf) | `434707ddcc72cb6f0949469f77c67b92bdaf86dc26ec9ab53e3f21fe8f7b0721` |
| crossink-lexend-bold/LexendDeca-Bold.ttf | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/builtinFonts/source/LexendDeca/LexendDeca-Bold.ttf) | `b7fd349e9d1330d2e39fda62e3f2167335d89fbc6996c7812599bdaa0f12a642` |
| crossink-lexend-italic/LexendDeca-Italic.ttf | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/builtinFonts/source/LexendDeca/LexendDeca-Italic.ttf) | `2b2d45a5925c66987283af3cbcd5598147aff1249ec32d83ccfdf2a1e8e9ecc1` |
| crossink-lexend-bolditalic/LexendDeca-BoldItalic.ttf | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/builtinFonts/source/LexendDeca/LexendDeca-BoldItalic.ttf) | `4c257cb370a592c56622aaed9c82183200806a1fd74bd05c9641563a33e1b6b4` |
| crossink-fontconvert/fontconvert.py | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/scripts/fontconvert.py) | `81c93f88ff03aea59a94ad8e3bf0c2ab2876763c82d363bc5838f99496eefe1d` |
| crossink-convert-builtin/convert-builtin-fonts.sh | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/lib/EpdFont/scripts/convert-builtin-fonts.sh) | `924166c7699032e6d3a2b6db71bfe1de3cc38bced509cc7126ac2c0b9fb058c9` |
| crossink-license/LICENSE | [pinned commit `b25beb13761d`](https://raw.githubusercontent.com/uxjulia/CrossInk/b25beb13761d5851f98e7eada09aa5e7d430df48/LICENSE) | `7212963b5b1b5ee34ca158163356beb1e6945dd2cc5d320f1fe7573c424ea640` |
| noto-sans-regular/NotoSans-Regular.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSans/hinted/ttf/NotoSans-Regular.ttf) | `478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823` |
| noto-sans-bold/NotoSans-Bold.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSans/hinted/ttf/NotoSans-Bold.ttf) | `1df075a380fc7cb898acf64c1f7b3b4dd780de3caa860178bf929de35817a913` |
| noto-sans-italic/NotoSans-Italic.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSans/hinted/ttf/NotoSans-Italic.ttf) | `467e3f89eeca4108bb8710a2b9e0cf2281ac56d5b0609211a83776d0505eecb5` |
| noto-sans-bolditalic/NotoSans-BoldItalic.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSans/hinted/ttf/NotoSans-BoldItalic.ttf) | `1b602a9d6353be42c91df097a4857b69fa2696f26703d7a33b54a15d87c2622c` |
| noto-symbols2-regular/NotoSansSymbols2-Regular.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSansSymbols2/hinted/ttf/NotoSansSymbols2-Regular.ttf) | `c4a0a80f0041ce4be81e2478faad22776d23edb98ae3f0d19bd37044820ecf9d` |
| noto-sans-ofl/OFL.txt | [pinned commit `71cf6f3ca9c1`](https://raw.githubusercontent.com/notofonts/latin-greek-cyrillic/71cf6f3ca9c185d809dd800ce9a9e58e1c16f1b1/OFL.txt) | `cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a` |
| noto-symbols-ofl/OFL.txt | [pinned commit `91ef396f7d14`](https://raw.githubusercontent.com/notofonts/symbols/91ef396f7d14a6373ea7e02a3d90f24f45a8f72f/OFL.txt) | `b118dd41337806a5d4797052c77caf3bd096aed783e5eb21b4d11154351e1ac0` |
| noto-symbols-regular/NotoSansSymbols-Regular.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSansSymbols/hinted/ttf/NotoSansSymbols-Regular.ttf) | `d0e98e9a2c046594c5021437273943be7e79e0fd980fde125279e22302212595` |
| noto-math-regular/NotoSansMath-Regular.ttf | [pinned commit `5c3913063bb1`](https://raw.githubusercontent.com/notofonts/notofonts.github.io/5c3913063bb189df4edc94b766584dfb4c04a187/fonts/NotoSansMath/hinted/ttf/NotoSansMath-Regular.ttf) | `d51afd5739c7ba6c44fcab35a88160e25dfb69a2d4ad0bd99533f8d894af1f96` |
| noto-math-ofl/OFL.txt | [pinned commit `6764bbdf67f2`](https://raw.githubusercontent.com/notofonts/math/6764bbdf67f2f94b26e2705c7c6e124efa2816f3/OFL.txt) | `403a95275b469061b7d4371c328e0ada3bc7d63328abe2e88aad5cd243b2fe21` |

The derivative names are **NewtReadingAtlas**, **NewtReadingAtlasBold**,
**NewtReadingAtlasItalic** and **NewtReadingAtlasBoldItalic**. Source Reserved Font
Names are attribution, not derivative names. Lexend notices and OFL remain in
[OFL.txt](OFL.txt); Noto Sans uses [Noto-sans-OFL.txt](Noto-sans-OFL.txt); both
Symbols families use [Noto-symbols-OFL.txt](Noto-symbols-OFL.txt); Math uses
[Noto-math-OFL.txt](Noto-math-OFL.txt). Retain all notices and OFL conditions;
do not sell the Font Software alone or use Reserved Font Names for derivatives.

Source import pins **FreeType 2.14.2**, Unicode **15.0.0**, 14 pt at 150 DPI,
`FT_LOAD_RENDER | FT_LOAD_NO_BITMAP | FT_LOAD_NO_AUTOHINT` (flags 32780),
source-font hinting with automatic hinting explicitly disabled for every source, proportional-figure substitution and linear advances rounded from
16.16 to sixteenths. Grayscale cutoffs 48/96/160 produce 2-bit coverage; drawing
keeps coverage ≥2. Profile tag 30 identifies this 29.1667-em-pixel source scale,
with **ascent 30, line height 36**; it does not redefine the point size. Sizes
8–90 use the existing nearest-center resampling.

The C++ font generator (`tools/make-reader-font.py` and
`tools/reader_font_extension.py`) was retired when the C++ firmware was removed
in NEWT-90. The checked-in JSON tables here and their converted Rust literal
tables in `rust/reader-atlas/src/data.rs` are now pinned literals;
`python3 tools/pack-reader-atlas.py --check` verifies the packed Rust atlas
against `rust/reader-atlas/source/reader_font_data.inc`.

Historical C++ import/check commands that produced the pinned JSON tables:

```sh
# Retired in NEWT-90; kept as provenance record of the pinned JSON inputs:
python -B tools/make-reader-font.py --font-source-dir /path/to/newt3-fonts
python -B tools/make-reader-font.py --check
```

Normal builds and checks use only stdlib and checked-in data; no FreeType,
source TTF, network or Python font package is needed. Source import also
regenerated the hash-bound [source-comparison PNG](../../artifacts/reader-symbol-source-proof.png)
and PBM. All source rasters were held in bounded host inputs (largest full face
bitmap 195,259 bytes; 200,000-byte host import bound). That bound is separate
from firmware resource ceilings.

## Generic per-block source rule

Lexend comes first, then the matching Noto Sans style. The symbol sources are
upright Regular in all four faces, in this fixed order per Unicode block:

| Block | Symbol order after Lexend and Noto Sans |
| --- | --- |
| Arrows; Mathematical Operators; Letterlike Symbols | Math → Symbols → Symbols 2 |
| Geometric Shapes | Symbols 2 → Symbols → Math |
| Miscellaneous Symbols | Symbols → Symbols 2 → Math |
| Remaining declared blocks | Symbols 2 → Symbols → Math |

The first source with a real cmap glyph wins. Orders are manifest data, must
preserve the two primary entries and contain every source exactly once. There
are no scalar-specific or book-specific selection rules. Every added glyph
records its winning source. Kerning applies only within the same source face;
cross-source kerning is zero. Noncontextual GPOS PairPos (including extensions)
is supported during import; contextual positioning requires shaping and is
outside scope. Runtime uses immutable sparse classes, without font state or heap.

The [pixel comparison](../../artifacts/reader-symbol-source-proof.png) renders
representative arrows, operators, shapes and music directly from all three
symbol fonts at the same size/baseline/threshold. Math covers 112/112 arrows
and 256/256 operators; Symbols covers 10/112 and 0/256, Symbols 2 covers 13/112
and 6/256. Math therefore supplies a coherent complete operator/arrow source.
Symbols 2 covers all 96 geometric shapes with the established square/triangle/
circle rasters. Symbols covers 135 miscellaneous glyphs, complemented by 121
from Symbols 2; Math covers only 41. Symbols is the designed music source:
U+2669/266A/266B have 36/56/93 thresholded ink pixels versus Math’s 35/57/92
at the same dimensions/advances; U+266E and U+266F also differ in vertical
extent. The choice is per whole block, not tuned to those examples.

Lexend’s italic TTFs have baked slanted outlines (post angle −11.30994°,
consistent with a 0.2 x-shear). Shearing regular/bold source outlines at profile
size with hinting disabled differs from their italic TTFs by 2/1/4/1 and
1/0/2/2 ink pixels for a/f/g/A, respectively, supporting oblique derivation
with outline cleanup/rounding; historical editing tools are unestablished.
Noto Sans uses genuine supplied bold/italic/bold-italic faces. Symbol sources
remain upright and regular across styles, preserving notation/direction and
geometric shapes. Bold scene-separator runs retain their style semantics with
upright squares; no separately designed bold symbol face is claimed.

## Coverage in every face

Counts below apply separately to Regular, Bold, Italic and BoldItalic. Lexend
and Noto Sans select matching faces; all symbol columns select Regular.
Counts are winning sources, not overlapping cmap totals.

| Block | Lexend | Noto Sans | Symbols 2 | Symbols | Math | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Printable ASCII | 95 | 0 | 0 | 0 | 0 | 95 |
| Latin-1 | 95 | 0 | 0 | 0 | 0 | 95 |
| Latin Extended-A | 127 | 1 | 0 | 0 | 0 | 128 |
| Latin Extended-B | 73 | 135 | 0 | 0 | 0 | 208 |
| Latin Extended Additional | 164 | 92 | 0 | 0 | 0 | 256 |
| Greek and Coptic | 4 | 117 | 0 | 0 | 0 | 121 |
| Cyrillic | 0 | 249 | 0 | 0 | 0 | 249 |
| General Punctuation | 24 | 62 | 0 | 0 | 0 | 86 |
| Letterlike Symbols | 5 | 75 | 0 | 0 | 0 | 80 |
| Arrows | 0 | 0 | 0 | 0 | 112 | 112 |
| Mathematical Operators | 15 | 0 | 0 | 0 | 241 | 256 |
| Geometric Shapes | 1 | 1 | 94 | 0 | 0 | 96 |
| Miscellaneous Symbols | 0 | 0 | 121 | 135 | 0 | 256 |

Legacy U+20AC and U+FB01/U+FB02 are also preserved. Combining marks,
Greek Extended, CJK, complex-shaping scripts, contextual positioning and
book-embedded font loading remain unsupported. Unsupported valid graphics paint
an honest selected-face `?` in an em cell; malformed UTF-8 remains a separate
error with no partial publication. No text, anchors or search/migration witnesses
are normalized or stripped by this font change.

## Legacy reproduction

The one supported rendering profile lived in `tools/reader_font_extension.py`
(retired in NEWT-90; its parameters remain recorded in `sources.json`).
Source import consumed its size, DPI, flags
and thresholds; import, normal generation and `--check` rejected a manifest
whose complete declared profile differed, including advance and kerning
rounding. All sources used the same profile.

With flags 32780, all original 341 rasters, metrics, advances and bearings and
all 116,281 kerning pairs reproduce exactly in every face:

| Face | Different metrics | Different bitmaps | Different 2-bit pixels | Different ink pixels | Different kerning pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Regular | 0 | 0 | 0 | 0 | 0 |
| Bold | 0 | 0 | 0 | 0 | 0 |
| Italic | 0 | 0 | 0 | 0 | 0 |
| BoldItalic | 0 | 0 | 0 | 0 | 0 |

Source mode fails before publishing any generated asset if any legacy mismatch
exists. The four unchanged legacy JSON files and packed prefix remain frozen
compatibility fixtures, checked against the same raster path/profile used for
additions. They are retained to preserve the existing format and independent
oracles, rather than to accommodate a rendering mismatch.

The previous flags 12 allowed FreeType's automatic hinting fallback and caused
338/339 Regular/Bold legacy bitmap mismatches. Explicit `NO_AUTOHINT` fixes them
on the same FreeType 2.14.2 runtime. The earlier attribution to a historical
FreeType version was not established and is withdrawn.

Relative to round 1, the 732 Noto Sans additions per style, 135 Symbols glyphs
and 353 Math glyphs per face have zero changed rasters or metrics. Symbols 2
changes 185 of its 215 used glyph bitmaps and 95 metric records per face:
13,630 local 2-bit pixel differences and 6,959 thresholded ink differences.
Lexend additions change 257/258 Regular/Bold bitmaps and 232/241 metric records;
Italic/BoldItalic additions are unchanged. Symbol selection stays per block,
with upright Regular reused across styles. The regenerated source proof shows
the corrected profile without introducing a face-specific hinting exception.

## Spaces and accepted ink overflow

U+2000–U+200A, U+202F and U+205F use real advance-only source glyphs.
At profile size, EM SPACE is 29 px (467/16 source pixels), THIN SPACE 6 px,
NARROW NBSP 5 px and MEDIUM MATHEMATICAL SPACE 7 px. Figure/punctuation spaces
use their source’s advances. ASCII and NBSP metrics are unchanged; U+3000
retains the existing semantic em cell. Default-ignorable/zero-width controls,
line separators and U+00AD retain inkless semantic behavior. Break policy is
unchanged: EM/THIN spaces supply normal break opportunities; NBSP does not;
forced overlong-word splitting remains the SDK’s existing policy.

The Lead explicitly accepted source bearings crossing the nominal line box.
**Do not rescale, shift or squash glyphs, change margins or change the 30/36
baseline recipe to hide overflow.** Neighboring-line ink overlap is ordinary
font behavior. Every added bitmap is at most 36 px high, but bearings can
extend 4 px above ascent 30 or 6 px below descent 6. Viewport clipping remains
a visible limit; framebuffer memory safety and fresh/cache/repaint parity are
mandatory and tested separately from optical device acceptance.

Outlier tables below count glyphs independently per block and face. Excess is
in source pixels beyond ascent 30/descent 6; examples prioritize maximum excess.
Blocks absent from a face table have zero outliers.

### Regular outliers

| Block | Above count | Max excess | Examples | Below count | Max excess | Examples |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| Latin Extended-B | 4 | 2 | U+01D9, U+01FA, U+01D7 | 35 | 2 | U+0218, U+0219, U+021A |
| Latin Extended Additional | 23 | 2 | U+1E2E, U+1EAE, U+1EB0 | 65 | 3 | U+1E00, U+1E01, U+1E08 |
| Greek and Coptic | 0 | 0 | — | 21 | 1 | U+037A, U+03AE, U+03B2 |
| Cyrillic | 1 | 1 | U+047C | 30 | 1 | U+0440, U+0443, U+0444 |
| General Punctuation | 0 | 0 | — | 8 | 1 | U+2016, U+2038, U+203F |
| Letterlike Symbols | 0 | 0 | — | 6 | 6 | U+210A, U+2118, U+2125 |
| Arrows | 0 | 0 | — | 3 | 3 | U+21F6, U+21D5, U+2195 |
| Mathematical Operators | 2 | 1 | U+221B, U+221C | 13 | 1 | U+2210, U+222C, U+222D |
| Geometric Shapes | 0 | 0 | — | 2 | 3 | U+25AE, U+25AF |
| Miscellaneous Symbols | 0 | 0 | — | 4 | 3 | U+26C8, U+26C6, U+266F |


### Bold outliers

| Block | Above count | Max excess | Examples | Below count | Max excess | Examples |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| Latin Extended-B | 6 | 4 | U+01FA, U+01FB, U+022A | 45 | 3 | U+0218, U+0219, U+021A |
| Latin Extended Additional | 26 | 4 | U+1E14, U+1E16, U+1E2E | 83 | 2 | U+1E00, U+1E01, U+1E08 |
| Greek and Coptic | 0 | 0 | — | 19 | 1 | U+037A, U+03AE, U+03B2 |
| Cyrillic | 1 | 2 | U+047C | 37 | 1 | U+0440, U+0443, U+0444 |
| General Punctuation | 0 | 0 | — | 8 | 1 | U+2016, U+2038, U+203F |
| Letterlike Symbols | 0 | 0 | — | 6 | 6 | U+210A, U+2118, U+2125 |
| Arrows | 0 | 0 | — | 3 | 3 | U+21F6, U+21D5, U+2195 |
| Mathematical Operators | 2 | 1 | U+221B, U+221C | 13 | 1 | U+2210, U+222C, U+222D |
| Geometric Shapes | 0 | 0 | — | 2 | 3 | U+25AE, U+25AF |
| Miscellaneous Symbols | 0 | 0 | — | 4 | 3 | U+26C8, U+26C6, U+266F |


### Italic outliers

| Block | Above count | Max excess | Examples | Below count | Max excess | Examples |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| Latin Extended-A | 0 | 0 | — | 1 | 1 | U+017F |
| Latin Extended-B | 4 | 2 | U+01D9, U+01FA, U+01D7 | 33 | 2 | U+021D, U+018D, U+0191 |
| Latin Extended Additional | 16 | 2 | U+1E14, U+1E16, U+1E2E | 20 | 3 | U+1E00, U+1E01, U+1E08 |
| Greek and Coptic | 0 | 0 | — | 21 | 1 | U+037A, U+03AE, U+03B2 |
| Cyrillic | 1 | 1 | U+047C | 31 | 1 | U+0440, U+0443, U+0444 |
| General Punctuation | 0 | 0 | — | 6 | 1 | U+2016, U+2038, U+203F |
| Letterlike Symbols | 0 | 0 | — | 6 | 6 | U+210A, U+2118, U+2125 |
| Arrows | 0 | 0 | — | 3 | 3 | U+21F6, U+21D5, U+2195 |
| Mathematical Operators | 2 | 1 | U+221B, U+221C | 14 | 1 | U+220F, U+2210, U+222C |
| Geometric Shapes | 0 | 0 | — | 2 | 3 | U+25AE, U+25AF |
| Miscellaneous Symbols | 0 | 0 | — | 4 | 3 | U+26C8, U+26C6, U+266F |


### BoldItalic outliers

| Block | Above count | Max excess | Examples | Below count | Max excess | Examples |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| Latin Extended-A | 0 | 0 | — | 1 | 1 | U+017F |
| Latin Extended-B | 7 | 4 | U+01FA, U+01FB, U+022A | 36 | 2 | U+0218, U+0219, U+021A |
| Latin Extended Additional | 26 | 4 | U+1E14, U+1E16, U+1E2E | 36 | 2 | U+1E00, U+1E01, U+1E08 |
| Greek and Coptic | 0 | 0 | — | 19 | 1 | U+037A, U+03AE, U+03B2 |
| Cyrillic | 0 | 0 | — | 37 | 1 | U+0440, U+0443, U+0444 |
| General Punctuation | 0 | 0 | — | 8 | 1 | U+2016, U+2038, U+203F |
| Letterlike Symbols | 0 | 0 | — | 6 | 6 | U+210A, U+2118, U+2125 |
| Arrows | 0 | 0 | — | 3 | 3 | U+21F6, U+21D5, U+2195 |
| Mathematical Operators | 2 | 1 | U+221B, U+221C | 13 | 1 | U+2210, U+222C, U+222D |
| Geometric Shapes | 0 | 0 | — | 2 | 3 | U+25AE, U+25AF |
| Miscellaneous Symbols | 0 | 0 | — | 4 | 3 | U+26C8, U+26C6, U+266F |

## Per-block and per-face flash table costs

The following exact byte attribution follows generation order: Regular, Bold,
Italic, BoldItalic, with sorted codepoints. Existing identical-bitmap reuse is
charged to its first emitted owner; later faces pay only their records when
the bitmap is shared. Each face column includes glyph records, 32-bit offsets
and classes for added glyphs. The CP column is the shared codepoint index.
Kerning metadata needed for legacy/new pairs is separate because it spans blocks.

| Added block | Regular | Bold | Italic | BoldItalic | Shared CP | All four |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Latin-1 | 0 | 0 | 0 | 0 | 0 | 0 |
| Latin Extended-A | 85 | 82 | 166 | 161 | 2 | 496 |
| Latin Extended-B | 26,947 | 29,084 | 27,889 | 30,231 | 416 | 114,567 |
| Latin Extended Additional | 35,044 | 39,083 | 37,436 | 41,130 | 512 | 153,205 |
| Greek and Coptic | 12,218 | 12,920 | 12,626 | 13,554 | 242 | 51,560 |
| Cyrillic | 28,402 | 30,979 | 29,951 | 32,500 | 498 | 122,330 |
| General Punctuation | 4,327 | 4,507 | 3,382 | 4,511 | 132 | 16,859 |
| Letterlike Symbols | 9,758 | 6,031 | 6,487 | 5,339 | 160 | 27,775 |
| Arrows | 14,191 | 1,792 | 1,792 | 1,792 | 224 | 19,791 |
| Mathematical Operators | 26,617 | 5,284 | 5,362 | 5,540 | 510 | 43,313 |
| Geometric Shapes | 11,037 | 1,651 | 1,641 | 1,651 | 192 | 16,172 |
| Miscellaneous Symbols | 35,710 | 4,096 | 4,096 | 4,096 | 512 | 48,510 |
| Global kerning metadata | 5,948 | 5,983 | 6,148 | 6,160 | 0 | 24,239 |
| **Total** | **210,284** | **141,492** | **136,976** | **146,665** | **3,400** | **638,817** |

Added tables total **638,817 bytes**, including the 502,378-byte shared
bitmap and 3,400-byte codepoint index, before descriptors, alignment and code.
Original font tables remain unchanged. All data are immutable flash; no runtime
font loading, synthesis, decompression, cache, per-glyph allocation or heap is
added. ASCII retains direct lookup; supplemental lookup has at most 20 binary
probes. The current checkpoint is **700 KiB measured image growth per variant**.
Repository flash ceilings/protected records are unchanged and require separate
Lead adoption. Usable app capacity remains 7,016,448 bytes.

A storage-only diagnostic on this corrected profile found that generic bitmap
substring pooling could reduce the shared bitmap to 472,979 bytes, saving
**29,399 bytes**. This optimization is **not implemented**; neither are compact
IDs. The current, reviewed table format is retained.

Current font SHA-256 is `ce2bda740b0031ba41b864e73c9d1107e4c59383b8ae1a64b16fd3fa9d91f553`; the 32-bit identity is
`0xce2bda74`. It hashes compact sorted-key JSON of the complete
coverage asset, including source pins, rendering/legacy profile, glyphs,
kerning and source decisions. Cache identity changes once; source coordinates
and semantic anchors do not.

## Verification limits

Exact source-derived host oracles cover all added glyphs in four faces, original
legacy pixels/advances/kerning, mixed lines, centered bold square separators,
wrapping/page boundaries, spaces, visible unsupported fallback, malformed input
publication, cached reopen and compiled failure controls. Edge/overlap tests
use sanitizers and compare clipped pixels with independent source-data drawing.
Host proof, native compile/resource evidence and device optical/runtime evidence
are separate. NEWT-9 physical checks and Linux-excluded Mac ImageIO decoding
remain unestablished by this work.


## Viewport overflow evidence

Source bearings, ascent 30, line height 36 and existing margins are retained.
The content viewport clips ink even where the physical framebuffer has spare
margin or footer space. That clipping is a visible limit, not an out-of-bounds
access. At source size the maximum excess is 4 pixels above and 6 below;
resampling scales those bearings with the glyph.

The actual SDK page-flow fixture uses 75-pixel text. Its first baseline is 91
in both orientations; the last baseline is 721 in portrait (528×792) and 451
in landscape (792×528). Tallest-glyph discarded ink counts for
Regular/Bold/Italic/BoldItalic are 64/99/62/90 in both orientations.
The deepest glyph, U+210A, loses 58/58/58/58 ink pixels in portrait
and 8/8/8/8 in landscape. All those pixels would fit inside
the physical framebuffer but are discarded by the content viewport: the margins
do not preserve them in the rendered page.

At size 90, forced first-line content baselines lose 81/135/90/126
tallest-glyph pixels; forced last-line content baselines lose
207/207/207/207 U+210A pixels, in both orientations. Tests also place
glyphs at physical framebuffer edges, where clipping is safe. ASan/UBSan and
framebuffer guards cover every added glyph at 90 pixels and per-face extrema
at every integer size 8–90. Fresh, cached and repainted SDK pages are identical;
no metric, baseline or margin adjustment hides overflow.

## Lookup measurement

The round-1 optimized host benchmark compared HEAD 73c5ed0 with the runtime
(lookup code and table dimensions remain unchanged in round 2)
on 1,200-glyph ASCII-heavy and mixed-script pages, with 1,000 iterations over
all four styles. Seven alternating before/after samples gave median times for
a `covers`/`advance`/`kerning` triple of 9.815 → 9.912 ns
(ASCII), and 20.469 → 42.489 ns (mixed script).
At 1,200 glyphs that is 11.778 → 11.894 μs
and 24.563 → 50.987 μs respectively.
The baseline returned visible fallback for previously unsupported mixed scalars;
the candidate resolves real glyphs and same-source kerning. These measurements
are host lookup evidence, not device page-turn latency, and impose no timing
pass/fail threshold. Runtime work remains bounded and allocation-free.


## Fresh-checkout and profile regressions (historical C++ gate)

In the retired C++ gate (removed in NEWT-90),
`tests/reader_font_generation_test.py` copied an explicit font input file set
with only the committed PNG, no PBM, TTFs or build caches. It ran `--check`,
normal generation and sanitized unit/Unicode/three styled-face proof binaries.
It also rejected size, DPI, threshold, flag and both rounding-policy mutations
in source mode and `--check`, even when embedded provenance was edited to match.
A corrupt committed PNG failed validation. The optional pinned-source mode
mutated a legacy advance and proved source import failed on reproduction drift.
The Rust atlas is now checked by `tools/pack-reader-atlas.py --check` and
`rust/tests/reader_atlas/`.


## Deterministic lookup work and limits (round 3, historical C++ gate)

In the retired C++ gate (removed in NEWT-90), `tests/reader_font_work_test.py`
instrumented copies of the C++ runtime from 73c5ed0 and the candidate. The
baseline source was hash-pinned; its table projection was verified byte-for-byte
against the original table hash, except for the identity assertion. The
measurement required no Git history or TTFs.
One unit is an evaluated lookup key comparison: glyph-index and kerning searches
(including final equality and linear kerning scans), plus Default_Ignorable
range comparisons. Direct indexing, plain scalar bounds and raster pixel loops
are excluded. This is lookup-work evidence, not CPU cycles or device latency.

Each authored page has 1,200 glyphs and is exercised separately in all four
faces at profile size. ASCII repeats ordinary letters, digits and punctuation;
legacy text densely repeats Latin-1/Extended-A accents and typographic quotes,
dashes and ellipsis, entirely inside the old 341 set. Mixed text repeats Latin
extensions, Greek, Cyrillic, letterlike symbols, arrows, operators, geometry,
miscellaneous symbols and music. It resolves visible fallback in the baseline
and real glyphs in the candidate.

| Page | Before average | Before worst | After average | After worst |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 10.579792 | 35 | 9.579792 | 34 |
| Legacy non-ASCII | 70.405000 | 110 | 70.405000 | 110 |
| Dense mixed additions | 101.500000 | 121 | 161.906250 | 181 |

Averages cover all 4,800 face-glyphs; worst is the maximum complete four-operation
sum for one glyph, not the sum of unrelated operation maxima. Operation averages
below are ordered `covers / advance / kerning / draw`:

| Page | Before | After |
| --- | --- | --- |
| ASCII | 0 / 1 / 8.579792 / 1 | 0 / 0 / 8.579792 / 1 |
| Legacy non-ASCII | 8.961667 / 12.771667 / 35.9 / 12.771667 | unchanged |
| Dense mixed additions | 9 / 23.125 / 46.25 / 23.125 | 20.875 / 35 / 71.03125 / 35 |

ASCII saves one comparison per glyph; legacy text is unchanged. The registered
regression enforces the 1.25× ceiling for both average and worst work separately
in every face on both old-text pages. Neither page reaches that ceiling, so
no runtime optimization or format change is made in this round: optimization
flash cost is zero. Dense mixed text adds a mean 72,487.5 comparisons per page
across styles. Converting these comparisons to milliseconds would require
separate target-cycle/cache measurements; none are claimed here.

The generator no longer exports a stale mutable `FONT_IDENTITY` or duplicates
the legacy coverage/rendering identity schema. Its module-level `font_identity()`
loads the validated complete extension profile; emission receives that profile's
identity explicitly. The fresh-file-set test compares the imported module,
both generated identity assertions, the public header and cache fingerprint
before `main()` runs. Reintroducing the exact former identity block makes this
check fail.

## Advance-cell overhang after integration

Layout fits advance cells inside the 16 px text box. A glyph whose cell lies
inside the box keeps ink that overhangs the cell, up to half the side margin
(8 px). Larger CSS-scaled ink can exceed it and is clipped at that boundary
so a physical inset remains. A cell that overflows the box keeps the text-box
clip. Rows never leave the box. The same cell-width calculation covers legacy
and added glyphs; missing graphics use the existing one-em fallback cell.
The retired C++ test `tests/reader_overhang_layout_test.py` (removed in
NEWT-90) checked styled exact-fit legacy line ends through layout, repaint,
back turns, restore and reopen; Rust reader layout and atlas suites carry the
current checks. This painting change did not change advance widths, pagination,
anchors or the font identity.
