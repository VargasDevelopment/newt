# Reader text fallback policy

This change closes one recurring class: a valid decoded scalar outside the
reading font's repertoire could reject an entire selected page as `Unsupported`,
stop the reader, and obstruct forward/back/reopen/recovery. A font repertoire
is not an EPUB validity rule.

## Exact historical evidence

- `002-bounded-offline-reader.md` and `006-reader-typography.md` promised visible
  question-mark fallback. Decision 006 separately records the SDK soft-hyphen
  measurement/emission predecessor mismatch.
- `015-m1-library-trial.md` changed the external profile to explicit unknown-glyph
  rejection. Its historical 61-book probe recorded one glyph rejection separately
  from one oversized archive member. Those old aggregate observations are not
  rerun against private books here.
- `039-battery-recovery-diagnosis.md` used a missing glyph as one of seven failed
  chapter/recovery fixtures. That fixture now uses malformed scalar encoding;
  the recovery checks still exercise a real bounded failure.
- `042-runtime-recovery-integration.md` and `043-reader-spacing-failure.md` record
  first-Next failure in the same chapter from U+200B, then U+2003. Source
  `29fffd2` fixed those two font cases and kept every other missing glyph fatal.
  This isolated work starts at clean evidence HEAD `e2c7cff`, includes that fix,
  and never reads the private book or operates the device.

## Canonical invariants and enforcement

`ReaderFont` is the existing shared measurement/kerning/raster boundary. Actual
atlas glyphs retain their original advances, every kerning pair and raster.
Every missing graphic scalar uses the existing `?` ink, a cell of one clamped
em and zero kerning. The wider cell prevents uniform-size CJK punctuation's
SDK half-em compression from collapsing multiple missing glyphs to one marker.
It intentionally repaginates formerly rejected text. `covers()` reports actual
atlas coverage only: SDK Arabic/ligature probes must not mistake fallback ink
for a real shaping glyph.

Unicode 17 `Default_Ignorable_Code_Point` uses bounded zero-advance/no-ink
fallback. Seventeen merged immutable ranges occupy 68 flash bytes. Fixed Unicode
spaces have explicit face-local widths: em/quad fractions, figure/period width,
thin/narrow, hair and mathematical space. NBSP keeps its original atlas metrics.
OGHAM SPACE MARK is normally visible, so gets graphic fallback. NEL and Unicode
line/paragraph separators have no ink; SDK line-breaking owns their boundaries.
No generic `Cf`, combining-mark or control-character deletion is used.
The property scan is bounded by 17 entries (ASCII exits at the first entry);
space widths use a 22-byte immutable table. One semantic-width lookup classifies
graphics, invisible controls and fixed spaces for all three font paths. Its
1,440 units per em preserve exact atlas/fraction rounding in one calculation.
No heap allocation or font cache
is added. Unsupported graphics intentionally acquire wider cells than the old
embedded-reader fallback, so their pagination can change.

The parser's existing ASCII whitespace collapse, entity decoding and normalized
codepoint anchor definition are unchanged. Original text remains in layout and
captured plans; neither scalar count nor persistence stores substituted `?`
bytes. Page anchors remain extracted-codepoint coordinates, not source bytes,
graphemes or cross-engine locators. Font fallback cannot return a book status.
`SelectingSink` validates complete selected-run UTF-8 but never asks coverage
for admission. The obsolete fatal glyph diagnostic state/getters and recovery
error branch are removed, including the standalone probe's retired accessor.

## Path and failure audit

| Boundary | Behavior retained |
| --- | --- |
| ZIP admission / cold or explicit open | Size, expansion, duplicate/name, ZIP method/flags, encryption, CRC and bounded resource checks remain independent of font repertoire. |
| XML / entity decode | Expat validates malformed UTF-8, illegal XML scalars and syntax. Existing entity-filter behavior is unchanged. Malformed selected-run UTF-8 is `ParseError`, never glyph fallback. |
| Embedded `Reader` | Already drew fallback; now shares the same canonical semantic/cell policy. Stored-only fixture/profile limits remain explicit. |
| External `PhasedReader` page/anchor selection | No remaining coverage-to-status producer. Frame overlap, page ceiling and unsafe/oversized paths can still be `Unsupported`; parser/lease/I/O/stale failures retain their statuses. |
| `SampleReader` first open / next / previous / chapters / settings / cold recovery | Delegates page rendering to the same font and sink. Failure rollback, saved-anchor policy, empty-chapter skip bounds and statistics remain unchanged. |
| Captured-plan repaint/cache | Copies original emitted text and resolves the same font policy after parser lease teardown. Live cancellation/source checks remain mandatory. No missing-glyph state can mask later memory/I/O errors. |
| UI / standalone aggregate probe | Existing bounded errors remain distinct from font fallback. Probe compiles without reading any private input. |

## Standards and limits

[EPUB 3.3 XML constraints](https://www.w3.org/TR/epub-33/#sec-xml-constraints)
permit UTF-8 and UTF-16 and require well-formed XML; that authoring standard does
not turn Newt's fixed Latin atlas into an EPUB validity predicate. This change
neither adds an EPUB validator nor promises full EPUB conformance or UTF-16
pipeline compatibility.

[Unicode 17 §5.21.6](https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-5/)
distinguishes visible fallback for graphic characters from invisible fallback
for default-ignorables. The property ranges follow the official
[Unicode 17 data](https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt).
[Unicode spaces](https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-6/)
and [UAX #14](https://www.unicode.org/reports/tr14/) distinguish fixed widths,
break opportunities and nonbreaking controls. Retaining source scalars lets the
pinned SDK use its existing rules instead of converting all whitespace to ASCII.

Fallback permits continued reading, but a missing script may consist largely
of question marks. It adds no glyph face, font cache, heap allocation, source
normalization, SDK patch or typesetting engine. Complex shaping, full bidi,
variation/joining effects and exact script typography are not established.
The existing SDK drops soft hyphens from emitted text while retaining them as
measurement predecessors; transparent cross-SHY kerning is not repaired here.
SDK CJK compression across differently sized spans uses the next span's size,
so one-em fallback does not establish nonoverlapping typography for arbitrary
mixed-size markup. Those are distinct layout fidelity limits, not font-driven
book rejection. Parser/resource/security failures continue to fail explicitly.

## Verification and handoff

The gate includes exhaustive font metrics/kerning for all 1,112,064 Unicode
scalars, an independent raw Unicode-property oracle with 4,174 default-ignorables,
and deterministic missing/blank raster controls in both geometries. The frozen
341-glyph/116,281-pair atlas proof remains separate.

Production authored fixtures exercise emoji, CJK, combining marks, private use,
unassigned scalars, supplementary planes, U+200B/U+2003 and sibling spaces,
joiners, variation selectors and soft hyphens. Missing-scalar metamorphism
compares different original scalars with identical fallback cells, exact anchors
and pixels in owned/borrowed storage, one-/seven-byte reads, both orientations
and bar settings, next/back/chapter transitions, captured repaint, settings
reflow and exact reopen. Later source failure retains its actual I/O error.
Twelve malformed UTF-8/XML-scalar controls fail `ParseError` before ink with
one-byte source reads and no retained parser allocation. Existing admission,
resource, rollback, persistence and recovery checks remain active.

Native compilation and resource-gate results are separate evidence. No ceiling,
partition, SDK pin, memory arena or stack policy is changed. Hardware installation,
optical acceptance, SD and private-book compatibility remain with the runtime
owner.

The complete `python3 tools/check.py` at `80db228` passed with ASan/UBSan, exhaustive
scalar/property checks, exact source-anchor/placement/raster controls, existing
failure/recovery checks and PBM/PNG previews. Separate compiled negative controls
reintroduced coverage rejection and narrow placeholder cells; each was rejected
by its intended regression without changing production source. The standalone
probe compiled without executing any book input.

One subsequent bounded size pass replaced duplicate blank classification/width
branches with the single semantic-width lookup. The exact-source exhaustive
scalar/font checks passed again, with independent fraction/atlas rounding
assertions at every clamped size 8..90. Both independent atlas proofs, production
CJK layout, owned/borrowed navigation/cache/settings/reopen efficiency fixtures,
phased malformed-input checks and the structural gate passed again. Unchanged
whole-repository audits were not repeated. Mechanical/math review approved the
representation and integer bounds; no further optimization rounds were taken.

Fresh bounded adversarial and maintainability reviews found the uniform-size
CJK placement and retired probe-accessor defects, which were fixed. The final
mechanical review is clean; the adversarial review's mixed-size SDK limitation
is explicitly retained above. Review is not physical-device evidence.

All four final clean native compilations and linked NVS/sleep-guard checks passed.
Their legacy resource gates **failed only the unchanged flash ceilings**:

| Variant | Measured flash | Legacy ceiling | Growth | Partition headroom | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| default | 1,244,992 | 1,244,928 | 64 | 6,553,792 | 133,264 | 300 |
| dev-usb | 1,260,224 | 1,260,176 | 48 | 6,538,560 | 134,080 | 300 |
| regional | 1,248,784 | 1,248,736 | 48 | 6,550,000 | 133,776 | 300 |
| regional-dev-usb | 1,264,064 | 1,264,000 | 64 | 6,534,720 | 134,592 | 300 |

SRAM is unchanged, no selected static-stack ceiling is exceeded, and all values
are bytes. Each current application partition has capacity 7,798,784 bytes;
headroom is capacity minus the complete firmware image, before any future reserve
policy. Net feature cost is attributable to the canonical Unicode/space policy,
with no atlas, SDK, partition or unrelated production changes. The bounded pass
saved 80 bytes in default/regional/regional-dev-usb and 96 in dev-usb relative to
`80db228`.

The owner approved a two-level size policy and this attributable correctness growth
through the parent thread: a physical partition/reserve hard limit plus reviewed
feature budgets, with separate RAM/stack constraints. The CI owner implements
that mechanism. This isolated checkout keeps the old baseline and tooling
unchanged, so its legacy flash gate continues to fail. Source is ready for
integration with the reviewed budget mechanism; these builds are not an
installation handoff and do not delay the separate device fix.

`artifacts/reader-text-policy-verification.json` binds the summary to source
hashes, the existing baseline, full host log and each raw native receipt/log.
Raw final evidence is retained locally under `.build/text-policy-size-pass-evidence/`
and the earlier complete host evidence under `.build/text-policy-evidence/`, rather
than overwriting the earlier runtime's tracked resource receipts. No runtime
tree edits, device operations, private-book access or publication were performed.
