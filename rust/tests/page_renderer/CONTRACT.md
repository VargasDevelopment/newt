# NRUST-41 contract (source only)

This packet ports the entire PageRenderer.cpp behavior and actual FontChain
methods from SDK cd6f5b831c8fad899723b6d406586f00c575f021. The exact original
PageRenderer.cpp/h, Font.h, TtfFont.h/cpp, their original supporting book/font
sources and notices are frozen under frozen/. They were read with local git
show, not copied from the checkout. provenance.json records every frozen file.
Newt reference: bc9ffda52b85f277e46c63718ac88d62fba5d368.

TtfFont's real font parsing, caches, stb rasterization, FreeType integration,
PNG/JPEG decoding, ZIP/source access and layout are not implemented here.
Font, RasterFont and ImageDecoder are explicit supplied interfaces. Synthetic
backends exist only in host/oracle tests and the explicit borrowed target probe. Nothing is described as a real font
engine or compressed-image decoder. No new crate, font or image asset is used.

FontChain owns eight registrations over a borrowed unique-object font bank.
The same bank index can occur more than once with different registered styles;
identity comparisons use that index, never the registration position or style.
Safe mutable references prevent registering one object in two bank slots.
Selection probes every registration, preserves first-on-tie ordering, masks
only bold/italic, falls back to face zero even for missing codepoints, and
computes styleCoverage with bit 4 for regular. Metrics use the first face.
Kern and ligature checks compare objects, and ligature output is reselected.
Font::covers is required explicitly: RasterFont implementors must delegate it
to has_glyph; metrics-only fakes may explicitly return true. This removes the
silent inherited-true failure without introducing a second coverage abstraction.
A sparse borrowed raster adapter is queried through dyn Font in host safety
source. This is an explicit implementation obligation, not an automatically
enforced equality between two arbitrary user implementations. Returned GlyphBitmap
borrows the font; all pixel consumption finishes before advance or any other
mutable font call. Glyph.advance is preserved data, not pen movement authority.

Page/run/image/link fields follow ChapterLayout.h. Slices replace pointers and
counts; run text is bytes, not str, deliberately accepting malformed UTF-8.
Href, target and fragment omit C terminators; the decoder adapter must map those
borrowed bytes to its source/catalog. Page and backend data stay caller owned.
The renderer does no allocation and forbids unsafe Rust. Core file is a module
for the existing no_std crate; it is not a replacement workspace or renderer.

The private decodeUtf8 is reproduced independently from accepted UTF-8 helpers:
leaders >= F0 request three continuations (even F8..FF); truncated/interrupted
sequences return their accumulated value; continuations alone are bytes; overlong
encodings, zero and non-scalars are retained. No ligature substitution happens
in render_text: layout must already have substituted the text.

Pen order is kern, select, rasterize, missing bookkeeping, coverage strikes,
select/advance, previous-codepoint update. A zero previous codepoint suppresses
kerning. Missing space and NBSP do not count. firstMissingOut stays unchanged
when none is missing. Combined render discards missing count, as C++ does.
Synthetic bold tests selected face flags, adds one coverage strike at x+1 and
does not change advance. Underlines depend on final pen and size >=24/28.

Frame formats preserve MSB-first white bits and padded row stride. Logical
bounds use the original lw==height equality branch, including square panels.
Bayer indexing is in logical coordinates before rotation. Coverage zero is
transparent; Sharp is >=120; Dither has floor40 and solid140 with integer boosted
coverage; Gray8 text keeps min(existing,255-coverage). Frame contents are never
cleared by render calls.

Images use the real row composition and Floyd–Steinberg diffusion, not Bayer.
Both mono formats use identical image handling: black-only writes, <128 decision,
clamp before residual, and signed division truncating toward zero. Carry updates
cast to i16 as C++ does; row transfer covers placementWidth+2 even for a partial
callback. Mono callback width clamps to placement width. Gray8 uses the callback
width without this clamp and overwrites luminance (including white erasure).
Clipping does not suppress diffusion computation. Each image attempt zeroes both
error rows; each decoder return restores byte-arena mark. Last non-Ok,
non-Unsupported status wins; scratch admission OOM returns immediately, whereas
decoder-returned OOM does not stop later attempts. Combined draws text first.

Scratch has two independent admissions: real caller-owned typed i16 error rows,
and original C++ byte-arena accounting (two separately aligned arrays, then
remaining decode allocation space). A typed-capacity failure returns OOM before
calling the decoder; it is not claimed as C++ parity coverage. ByteArena exposes
checked borrowed byte allocations, marks and remaining bytes to decoder seams.
It cannot return typed i16 from byte memory without unsafe, so typed rows are
supplied separately. Peak physical memory therefore includes those typed rows
in addition to the simulated C++ byte reservations. The caller must account for
both; this packet makes no memory/performance acceptance claim.

Exact C++ comparison domain: valid enum variants, positive i16 panel dimensions
and stride with sufficient complete backing frame, consistent u16 counts and
run byte lengths, valid NUL-terminated C++ href strings, valid font objects and
bitmap descriptors with width*height readable samples, allocated/aligned arena
storage, bounded callback row slices, and no overflowing i32 text coordinates.
Decoder contract is already-scaled grayscale rows top-to-bottom; placement and
callback width may differ, and rows may be partial. Out-of-frame pixels are
supported, not errors. Zero-size glyphs/images are supported. No scalar-valid
UTF-8 requirement is imposed. Callback y is u16 and need not be below placement
height for safe composition (the panel performs clipping).

Rust rejects negative/zero frame descriptors, short backing buffers, oversized
page counts/text, short glyph/row slices, invalid bank indices and text coordinate
overflow using typed errors. Errors may follow already-rendered pixels/font
calls; this API is not transactional. Unrepresentable enums/pointers are excluded
by types. These boundaries exceed the comparison domain; excluded C++ inputs are
not all claimed to have undefined behavior. Typed row capacity is Rust admission,
not an assertion about C++ invalidity. Tested safety counts must remain separate
from parity cases.


Reviewed packet additions (all unexecuted): tests/corpus.py has a 3-format by
4-rotation asymmetric text matrix and a separate 12-case image matrix, including logical Bayer fringe samples that
change under panel-coordinate indexing. The values are hand-derived complete
panel bytes with padding, never captured port output. Clipped image [100,100]
lets the offscreen residual turn the visible sample white. A partial width-1
row [100] carries residual into width-2 [100,150] on the next callback. Repeated
registrations use a missing left operand (first-registration fallback) and a
covered right/ligature operand (bold registration), all on the same object.
Synthetic measured advance and bitmap advance are independently supplied (2
and 7 by default); the advance call trace remains part of each expected reply.

Response admission in tests/replies.py parses every trace event signature and
integer domain, rejects unknown events and incomplete/canonical failures, and
requires precisely one typed answer per requested query before comparison.
Malformed-response fixtures run in-process under the existing owned Run scope;
no new subprocess entry point is added.

The shared target/borrowed_caller.rs caller operates on runtime-borrowed text,
glyph pixels, decoder rows, complete frame, i16 error rows and byte arena. Its
local arrays contain descriptors/references only. A nonzero byte-arena mark is
reported before/after rendering, along with missing-glyph/row/coverage outputs.
The host test supplies nonempty data and specifies the entire resulting frame.
Target compilation/symbol retention is compile evidence only; it cannot prove
runtime output or device behavior. Synthetic adapters are not real rasterizers
or compressed-image decoders.
