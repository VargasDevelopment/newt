# Reader semantics integration contract

> Publication status: this document describes its historical source handoff. The new source-only draft has not run the complete host or native gates and is not an installed image. Consult the draft PR checklist for current dependencies and blockers.

This isolated source branch starts at `cc2db165639fc66587c486280d836323ff85c474`.
The owner authorized proper section names and consistent styled layout. It does
not replace Newt's reader, chosen visual system, source opening, persistence,
battery or sleep paths. The runtime owner retains fast-open integration and
metadata publication; the source owner supplies narrow, separately committed
navigation and typography patches.

## Navigation slice

The source owner owns `reader_navigation.h/.cpp` and their host tests. The
navigation producer copies bounded labels and full canonical targets from the
SDK Book while metadata arenas are live. No Book pointer survives teardown.
Spine indices remain reading coordinates, never fabricated chapter numbers.
The projection owns at most 1 KiB, has no heap allocation, and reports resource
limits. Repeated fragment callbacks resolve canonical character positions;
unresolved or ambiguous targets remain unavailable. SDK callbacks expose only
fragment hashes, so they cannot prove arbitrary fragment identity against an
unrelated colliding HTML id.

The consumer reads the section label for the successfully published page's
canonical spine/character anchor. Per-document page counts keep their existing
meaning. Book-progress inputs identify reading-order coordinates and declared
text-member byte weights; those weights permit an estimate, not a claimed
whole-book page total. Missing navigation uses `Section N`.

The owned projection measures 976 bytes. Display titles keep a complete UTF-8
prefix; exact full SDK title equivalence is recorded before arena teardown,
including declarations omitted from the bounded display projection. Display
truncation never establishes target identity. Conflicts remain ambiguous on
both their source document and unlisted continuation documents.

Changes to `PhasedReader` are limited to metadata projection, anchor observation
and successful-page publication. Footer changes only consume the bounded label.
The runtime owner must provide the same projection when a metadata cache bypasses
Book opening. Loading feedback is separately owned and is outside these patches.

Two independent reviews approved the measured navigation costs: app images
1,249,456 / 1,264,704 / 1,253,264 / 1,268,528 bytes for ordinary / USB /
regional / regional USB. Growth is 4,464 / 4,480 / 4,480 / 4,464 bytes.
Static SRAM and RTC stay unchanged, but dynamic reader ownership grows by
976 bytes. Transactional metadata copies require individual native frames of
1,840 bytes for open and 2,240 for layout. Direct field clearing keeps close at
16 bytes. New exact guards also cover stored Reader layout (1,184), chrome
(304), UI rendering (160), and sample rendering (144). Every original guard
remains. These are compiler frames, not combined call-chain or hardware peaks.

## Typography slice

The source owner owns the reader font/style helpers and host tests. One face
selection must govern measurement, kerning and painting, including copied-page
repaint. The approved regular atlas remains pixel and metric compatible.
Style variants, if added, use bounded licensed first-face data, not CrossInk's
whole font catalog or UI. CSS uses the existing SDK subset and an explicit
memory budget. Presentation policy and font-family identity must invalidate
incompatible page caches. Saved anchors remain source-text coordinates;
unsupported CSS must not silently change their meaning.

Navigation fixtures cover EPUB3 nav, NCX, frontmatter, fragments, orientations,
copied/direct page parity and saved-anchor restore. Typography fixtures cover
nested styles, oversized CSS, depth-boundary visibility and serialized-cache parity.
Host and clean native resource reports are separate from physical evidence.
No hardware, runtime checkout, private book, SD card or public service is changed.

## Following image phase

Home/Library covers have higher priority than inline EPUB images. Both use the
existing SDK image decoder/cache infrastructure, within separately measured
workspace budgets; no full-book decode or wholesale renderer port is planned.
An authored image-only XHTML spine remains a navigation section. A future
layout outcome must distinguish omitted image content from genuinely empty
content before the runtime's zero-text-page skip rule can use it. Current
text-only page counts do not certify that an image-only section is empty.

## Current canonical navigation integration

The preceding sections preserve the original SDK source handoff and its costs.
PR #23 adapts navigation to the canonical runtime through a borrowed metadata
view while arenas are live. Optional navigation does not change the mandatory
`CanonicalBook::open` path. The producer copies bounded labels and validates
full target ambiguity before truncation; unusable declarations retain generic
labels, while I/O, stale-source and cancellation failures reject publication.

Two heap-owned selection slots stage each candidate. The active slot changes
only after lease completion and final source/page publication guards. Cache
streams stage metadata through the same producer; unresolved fragment labels
remain generic. The linked C3 allocation request for the pair is 3,571 bytes,
included in external-reader preflight. This replaces the original large local
transaction copies: the current selected open/layout frames are 64/448 bytes.
Neither allocation requests nor individual frames establish runtime heap/stack
peaks. Current measured costs and protection history are documented in
`CODE_HEALTH.md`; historical image numbers above do not describe this candidate.

Manual review of all 20 authored frames at `80bd6db` approved exactly three
footer changes from the generic label to the declared section name. Final
combined-source software/resource gates and physical acceptance remain separate.
The performance target is still unmet; this navigation work makes no speed claim.

## Current bounded styling integration

Matching regular, bold, italic and bold-italic faces share one selector for
advances, kerning, semantic spaces and painting; captured, cached and immediate
pages paint each run's stored face. The composed font identity is `0xf211011a`.
Chapter CSS follows `reader-styled-layout-plan.md`: select() resolves it while
its canonical Book is open, builds it without the Book into one 64-rule slot,
and layout's own pass checks body nesting in flight. Cache hits, captured pages
and repaint read no style source, and a warm or restart open that a cache hit
follows reads no style source either. The slot, its capture and the pass's
watch live in the existing workspace blocks, so the selection allocation keeps
its f008308 size (3,571 bytes on C3, 3,591 on 64-bit hosts) and external-reader
admission needs nothing more. Allocation requests do not establish runtime heap
peaks.

Policy 1 semantic counts still equal styled layout totals. Book `display:none`
rules are neutralized and body nesting is bounded, so CSS changes faces and
spacing, not the laid-out text. Canonical extracted-text anchors and durable
progress codecs are unchanged, though new faces can reflow a saved anchor onto
a different page.
