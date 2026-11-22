# Bounded styled-layout profile

> Publication status: an isolated integration. Host suites and native compile
> evidence are separate from reviewed golden frames and physical acceptance;
> none of this document is hardware evidence.

The owner authorized consistent styled layout while retaining Newt's chosen visual
system. The separately reviewed navigation slice is followed by matching font
faces and one owned SDK rule projection per laid-out chapter.

## Font boundary

Use the existing Newt reading family, with matching regular, bold, italic and
bold-italic assets. One immutable face selector must govern advances, kerning,
semantic spaces and painting. Mask the SDK's bold/italic bits; unrelated style
flags do not choose a different face. Forward run flags through both ordinary
Reader and PhasedReader painting, including copied-page repaint.

Keep `NewtReadingAtlas.json` and the regular generated prefix byte-identical.
Import only the 341 supported codepoints from each pinned first face, using
stdlib tooling and exact source hashes. Do not import the mixed fallback catalog
or CrossInk renderer. The three added faces require 120,404 bytes independently;
shared immutable class maps and identical bold/bold-italic kerning reduce this
to 116,982 bytes, plus descriptors, alignment and code. No runtime font heap or
decompression. All four source profiles declare ascent 30, baseline step 36 and
descent -8; extreme styled ink can extend two pixels farther between rows.

Retain the existing 2018 font notice and the source TTFs' embedded 2019 Lexend
copyright and SIL OFL 1.1 notices. Use derivative Newt names. The bold source's
name records say Regular, but its pinned OS/2 weight is 700; provenance must use
verified source hashes and weight, not filenames alone. Preserve source and
generator attribution. Ordinary generation must not require upstream files.

The composed font identity `0xf211011a` invalidates old serialized page layouts.
Changing font geometry reflows pages, while canonical extracted-text anchors and
durable progress schemas retain their meaning. Exact-page RTC compatibility
must be assessed separately from validated-anchor durable recovery.

## CSS boundary

Reuse the pinned SDK's XML, path and CSS helpers. Keep null-ZIP text layout and
`embeddedStyles=false`, supplying owned rules instead of allocating the SDK's
512-rule builder inside the near-full layout arena. One rule slot holds the
current chapter's styles, keyed by spine and dropped on close, on every source
change and by every select. It adds no allocation: between passes the reader
holds it, with an integrity seal, at the top of the shared workspace block,
which bounds the shared arena below it; a layout pass needs the whole shared
block, so the slot and the pass's watch are lent to the pass at the top of the
parse block. Layout passes no ZIP to the SDK, so no image probe shares that
block; the pass's parse demand is one chapter stream, the XML chunk and the
entity filter (45,824 bytes on host for a deflated chapter), below the loan.
A Book may need the whole shared block, so select() and the semantic index
drop the slot before opening one. A held slot whose seal no longer matches is
not used; the next layout selects and resolves again.
`PhasedReader::select()` fills the slot in two `ReaderStyles` phases, so no
path opens a Book for styles alone:

- **Resolve** runs while select's canonical Book is open, after navigation
  binding: in open, a new-spine layout, a cache build and a warm or restart
  reselection whose spine the page cache lacks. A reselection that a cache hit
  follows is plain: it reads no style source and holds no slot, so that open
  reads exactly what it read before styles existed. A head-only SAX scan
  (16 KiB entity-filtered input; stops at body) records up to eight ordered
  source events and copies the embedded `<style>` text (4 KiB aggregate).
  It keeps scanning after `</head>` to reject any later head, ignoring gap
  elements; a long head or gap before body can now exhaust the same byte limit.
  Each of at most four `<link rel=stylesheet>` gets a validated ZIP member
  location (local href, a `text/css` manifest item with no contradictory
  declaration, an existing stored or deflated member, consistent stored
  sizes, the inflate input bound and a 256 KiB decoded-work limit) or a deterministic
  rejection. The record sits at the top of the parse block and the text just
  below the slot's home, never on a stack or in a Selection; they hold no
  Book, name or arena pointer. Resolve allocates only from the parse block
  (one member-name directory and one path, `kResolvePathBytes`). The text
  takes the room the open Book's metadata leaves, up to 4 KiB; when that was
  too little, build captures the head again without the Book, with the whole
  budget, keeping the Book's link outcomes. So no metadata size decides a
  chapter's rules: more than 4 KiB of embedded text is a text limit on every
  route.
- **Build** runs once that Book is gone, in fresh arenas. It reads each
  resolved member exactly through ChapterSource, builds the rules, and
  neutralizes `display:none` while keeping at most 64 rules (1,792 bytes) in
  the slot using the streaming admission policy below. A slot with rules is Pending; an empty or limited one is Settled.
- **Watch.** Layout's own decoded chapter bytes pass through
  `ReaderStyleWatch`; there is no separate body pass. For a Pending slot it
  tracks body nesting, and a clear, complete pass settles the slot. Nesting
  the SDK style stack cannot hold stops the pass: the slot drops every rule
  (`StyleNestingLimit`) and the chapter is laid out again.

A Book whose manifest declares no stylesheet skips resolve: its chapters can
carry only embedded styles. Its slot is Deferred, and the watch admits only
`html`, one `head`, `title` and `meta` elements before `body`, which must start
within 2,048 decoded bytes. Anything else stops the pass: a `style` or `link`
element of any kind, `script`, `base`, `xml:base`, an internal DTD subset,
markup it cannot classify, or a head longer than the window. The chapter is
then resolved without a Book, built and laid out again. So a chapter takes at
most three passes. In such a Book only a chapter whose head the watch admits
reads nothing extra; any stop costs one Book-free head scan and another pass,
even when the head names no style source, and costs it again whenever that
spine is selected again. The result is the same unstyled layout; only the cost
differs. The cache writer runs the same
watched pass; a stop discards the partial cache file and restarts the writer,
so a built chapter is decoded once unless its pass stops.

Same-spine page turns and restores reuse the slot. Another spine's select
replaces it; a later layout of the replaced spine selects again, except in a
Book declaring no stylesheet, which defers again without reopening it.
`repaint()`, captured pages and cache hits read no style source. Open (import
and strict admission, not warm or restart opens) resolves spine 0. Strict
admission shows it next unless a saved locator names another spine, whose first
layout then selects it. A cache build selects with its own Book, as before
styles, so import (open, then spine 0's cache build) resolves spine 0 twice. No
firmware route builds a cache: the firmware shows with `buildOnMiss` off.

Empty and limited sheets attach `nullptr`. An active sheet consumes one of the
SMALL engine's sixteen style states, so at most fourteen nested elements below
body are admitted; deeper, multiple or missing bodies drop the optional sheet.
Fatal policy/resource limits, unsupported provenance, malformed head XML and
damaged stylesheet bytes discard the entire external/embedded profile with
issue bits; the chapter's own malformed XML still fails layout as before. A
linked member is damaged (`StyleDamagedSource`) when its stored sizes disagree,
or when it fails to inflate or decode to its declared size although every read
succeeded and the source is still live at the same size. Source I/O faults,
card removal, cancellation and allocation failures propagate and never yield a
differently styled layout. A head scan that fails on content records
`StyleMalformed`; if layout's own parse of that chapter then succeeds, the scan
failed for allocation, and the attempt reports OutOfMemory. That cannot make a
chapter unopenable: the scan's Expat allocations are the deterministic prefix
of layout's own parse (the same filtered chunks, configuration and heap), so a
failure that persists fails layout's parse as well, as it did before styles,
and a transient one leaves no slot, so the next attempt resolves again.

The cache route and generation bind the font identity, policy identity
`0x4e535409` and engine revision 3; regular-only revision-2 pages are rejected.
A pinned digest of the policy inputs (the policy files plus the marked
`style-policy` regions elsewhere) requires a reviewed history entry for any
change, and a policy identity bump unless every chapter keeps its rules. The
computed sheet hash stays out of the route, so cache probes need no styles.

Filter new external/embedded visibility declarations so they cannot change
existing extracted-text anchor coordinates. Inline visibility behavior remains
the baseline. Supported properties use the pinned SDK declaration parser. Simple selectors are parsed and hashed by the pinned SDK, including its UTF-8,
escaped-identifier and malformed-token quirks; this is not full CSS or arbitrary EPUB support. The pinned numeric CSS
parser has unchecked signed arithmetic. A conservative six-digit-run preflight
protects the new external/embedded path; the baseline inline hazard remains a
separate limitation. No arbitrary-input safety claim is made.

## Verification and next image phase

Require independent source-data proofs for every styled raster, advance and
kerning pair, regular parity, nested style inheritance, center/right alignment,
spacing, both orientations, safe clipping and fallback behavior. Exercise
interleaved sheet order, 64/65/512-rule and text/source limits, numeric overflow,
copy/arena teardown, I/O/lease cleanup, saved anchors and serialized-cache parity.
Measure exact owner allocation, clean native images and individual frames; do
not raise a ceiling merely to pass or infer runtime peaks from static SRAM.

Home/Library covers remain the higher-priority following image phase. Inline
image support then uses existing SDK decoder/cache facilities with explicit
workspace bounds. Distinguish image-only content from genuinely empty content
before zero-text-page skipping. No full-book decoding or whole reader port.

## NEWT-4 streaming admission (policy 10)

The compatibility boundary is the pinned SDK parser, not a second identifier or
declaration grammar. For a source at most 8,192 bytes with at most 64 SDK rules,
Newt uses the unchanged source only when every raw SDK selector range is
nonstructural and the canonical scan finds no protected boundary risk. Within
that domain, rule count, order, selector hashes and every declaration equal
`73c5ed0`, including zero and empty declarations, UTF-8 and safe escaped names,
BOM behavior, malformed comma members, `!important` and SDK quirks. NUL or a run
of more than six digits remains a fatal numeric limit, and `display:none`
remains neutralized. Ordered sources remain separate parses.

`src/reader_css_scan.h::Scanner` is the sole scanner for fast-path eligibility
and projected rule/list/declaration boundaries. It distinguishes code from
comments, strings, functions (including unquoted `url()`), attributes and
escapes across 256-byte chunks, following CSS Syntax Level 3 token-consumption
algorithms and current non-ASCII identifier ranges. Its eight-byte lookahead
and 64-byte block stack are fixed workspace; token errors/overflow report
nonfatal `StyleTruncated|StyleCssSyntax`. A protected `{`, `}`, `;` or `,` anywhere makes
the source outside the exact domain. A comment after a selector token or comma
is removed even when it contains no delimiter; leading comments that the SDK
itself skips may remain exact before any selector begins. Escaped structural
punctuation, including `.`, is projected and vetoed; safe escaped identifier
bytes retain the SDK's raw hashing quirks.

Projection emits complete CSS units and never permits protected payload bytes
to manufacture a rule, irrespective of the SDK's raw cursor context.
Selector-list members containing combinators (`+`, `>`, `~`, descendant
whitespace), IDs, attributes or pseudos are vetoed individually; supported
members such as `.safe` in `h1+p,.safe` survive. Unsupported context never
becomes its rightmost simple selector. DROP therefore makes
`p{text-indent:0}p+p{text-indent:1.5em}` all flush-left; sibling/child matching
remains unimplemented.

Each projected selector/declaration must keep the source significant token
sequence. Exact two-bit token-start marks are checked by the canonical scanner
before publication. A join such as `.safe/**/p` rejects that member with an
issue; `p/**/.safe` remains valid. Unsafe declarations are dropped whole with
`StyleTruncated|StyleCssSyntax`, preserving safe neighbours and empty rules
until relevance compaction. No masked or truncated value reaches the SDK.

The reusable arena holds one 8,192-byte source/projected-rule buffer, one fixed
8,192-byte SDK batch scratch buffer, 2,048 bytes of identity marks and a
64-rule pending queue (1,792 bytes).
The retained slot remains 64 rules (1,792 bytes). Larger sources stream without
member-sized allocation. SDK batches contain at most 512 raw selector members, independently of the
64 retained slots; direct capacity tests verify the pinned SDK ceiling. External input uses 256-byte chunks
and a 262,144-byte decoded-work cap. The other bounds remain four links, eight
ordered sources, 4,096 embedded bytes, 16,384 head bytes and body depth 14.
Overlong/nested rules and capacity loss are nonfatal truncation; at-rules are
skipped. Fatal numeric, source, head, body nesting and damage issues still discard
the profile, and real I/O/allocation errors propagate without publication.

All SDK rules, including empty and unsupported-property rules, keep exact
positions while the small/exact source fits. Once compaction is required, an
explicit text-indent declaration, including zero, is priority 2; another
applied declaration is priority 1; empty/neutralized-only rules are priority 0.
Admission merges each pending batch with the retained rules, compacts once per
64 candidates, caches the minimum retained priority, keeps the latest rules in
each admitted tier, then restores their original order. Capacity semantics and
`StyleTruncated|StyleRuleLimit` reporting are unchanged. The policy depends on
properties and source order, never book identity, filename or class name.

See [Decision 063](063-reader-css-fidelity.md) for the R3 diagnosis, option
measurements, exact-domain regressions and verification status. The
[probe verdict contract](epub-probe-styles.md) defines the neutral read-only
report. Host, native compile/resource and physical evidence remain separate.
