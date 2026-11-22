# EPUB style-admission verdicts

The reader policy is defined in [reader-styled-layout-plan.md](reader-styled-layout-plan.md),
with current evidence in [Decision 063](063-reader-css-fidelity.md).
The separate `tools/epub-probe` branch is not part of this checkout. Its
chapter-style verdict helper must compile the shared production ReaderStyles
and use policy identity `0x4e53540a` when integrating this change.

For each sampled chapter, distinguish rules applied (`sheet.ruleCount > 0`),
fatal limitation (`StyleLimited`) and partial admission (`StyleTruncated`). A
partial profile can apply useful rules. Report kept rule counts and the
`StyleRuleLimit` / `StyleTextLimit` reasons; `StyleSelectorRejected` marks
unsupported full selectors, and `StyleCombinatorRejected` marks rejected
structural context. `StyleCssSyntax` reports CSS token recovery or bounded
block-stack overflow, and whole declarations dropped to preserve projected
token identity, with `StyleTruncated`. Selector token joins are rejected with
`StyleSelectorRejected`. Do not label a partial chapter complete. Small compatible sources retain empty SDK rules as well; a nonzero rule count
does not prove that a declaration changes layout. Empty sheets
without a limitation are neither an applied nor a fatally limited profile.

The internal streaming Admission exposes `truncatedRules()` for a host verdict
helper that replays the captured ordered sources through the same policy and
the canonical scanner in `src/reader_css_scan.h`.
It counts capacity evictions/rejections and skipped oversized/malformed rule
units; it does not count intentionally unsupported selectors or empty
rules. No counters or pointers are added to the firmware's retained rule slot.
A replay must verify its kept rules equal ReaderStyles::build before using
this diagnostic count. Damaged streams retain the production fatal result.

Private owner files are read only. A permitted neutral report contains only
book labels, sampled chapter counts, applied/limited/partial chapter counts,
kept/truncated rule counts, and whether a combinator was rejected. Never emit
paths, titles, selector/class names, CSS, text, digests or book-derived fixtures.
Sampling remains capped at 64 distinct spine members; admission is host
policy evidence and does not establish device layout or power behavior.
