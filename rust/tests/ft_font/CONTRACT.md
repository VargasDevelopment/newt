# FtFont policy and native boundary

Prepared source candidate; every new compile, test, gate and target check is
UNEXECUTED. Historical ft-feedback-01 results apply to the earlier staged
wrapper, not this integration.

Authority is pinned FreeInk cd6f5b831c8fad899723b6d406586f00c575f021,
Newt reference bc9ffda52b85f277e46c63718ac88d62fba5d368. The exact
original wrapper, headers and licenses are under original/. The complete
FreeType C closure and unchanged original Gpos/Gsub/FtFont sources are in
../page_renderer/frozen/, authenticated by that packet's PROVENANCE.json.
newt-pin/ retains immutable platform route configuration and both patches.
No vendor bytes are changed or normalized by this gate.

The source references below are lines in original/libs/font/FreeInkFont/src/FtFont.cpp:

- configureMemory (238), inspectMemory/Stream (249/266), deinit (296),
  init/initStream (362/396): the Rust backend owns faces; a Library owns the
  native allocator and outlives every face. Memory bytes and stream contexts
  remain borrowed. Rust deinit/Drop is idempotent. Explicit library creation
  replaces the original process static; its failure is observable immediately.
- finishInit/applyVariation (449/464): bounded 16-axis coordinates, wght/ital/slnt
  clamping and synthetic weight/slant fallback remain safe Rust policy.
- setRenderOptions/applyGlobalProperties (505/535): capabilities determine
  support, requested settings are retained, and shared native properties are
  reapplied before each load. Both disabled/enabled optional build flags are
  exercised by separate native objects and fresh actual core/host builds.
- expandMonoCoverage/ensureSize26_6 (554/576), metricsGlyph26_6 (646),
  rasterizeGlyph26_6 (897): sizes, error stages, linear advances, synthetic
  style, bounds and packed pixels are policy in core/src/ft_font.rs. The
  mutable bitmap borrow prevents another mutable font operation while pixels
  are live. Negative/padded grayscale and mono rows use caller scratch.
- kerningGlyphs26_6 (689), ensureGsub/GposLoaded (746/787), ligatureGlyphId
  (822): native legacy kerning precedes the canonical crate::font_layout raw
  GPOS reader. GSUB likewise calls that canonical reader. sfnt_table locates
  bounded borrowed tables (first TTC face); it is not a second GPOS/GSUB parser.
  Native scaling retains FT_MulFix. Stream budgets/release rules are Rust
  policy; resident tables borrow source bytes and do not consume stream cache.

core/src/ft_font.rs forbids unsafe code and uses crate::page_renderer's actual
Font/RasterFont/GlyphBitmap traits. No wrapper crate, duplicate layout parser,
Rust allocation, hidden C++ policy backend or target FreeType implementation is
introduced. firmware/fonts/native.rs and native/native.c contain the explicit
unsafe C boundary, shared by this host proof and the unaccepted RV32 probe.
Rust links only adapter + vendor FreeType C objects; FtFont.cpp/Gpos.cpp/
Gsub.cpp are linked solely into the separate oracle process.
The callback zeroes C output storage before constructing a safe mutable slice.
The retained native regression reads the slice before writing and checks guards.
Callbacks must obey allocator validity/lifetime rules, be synchronous, and
never unwind or reenter; Library is not Sync. Rust does not establish arbitrary
third-party native C memory safety.

Intentional ownership changes: GSUB/GPOS/mono scratch is caller-owned and
bounded; exhaustion disables streamed tables or reports BitmapBuffer. Library
and face bookkeeping use explicit allocator blocks. Failed size initialization
closes wrappers immediately. inspect closes temporary faces. Complete policy
correspondence and remaining heterogeneous-native gaps are recorded in
STAGED-HISTORY.md; its old preparation commands are superseded by this gate.

The independent synthetic TTF has UPEM1000 and a rectangle from (100,0) to
(500,700), with hmtx bearing100 and advance600. At 10px it must produce
4x7 full coverage, xoff1/yoff-7/advance6, linear advance384, line metrics
512/-128/768, and GPOS kern-64. fixture.py generates this from explicit table
bytes using the canonical font-layout fixture helper in the frozen snapshot.
It never learns expected pixels from an engine. All nine cases, both flag
configurations, all seven mutation witnesses, and 13 malformed responses are
retained. Five primitive policy tests and three native host tests remain distinct.

Runtime evidence requires canonical Run.execute: zero exit and empty stderr,
then command-specific FTFONT1 grammar and result-dependent arity/ranges/pixels.
Fresh mutants alter actual core policy, compile core and transport with
-Dwarnings, and must emit a complete admitted semantic difference. Compiler
failure, diagnostics, crash, malformed output or rejection is never a kill.
Canonical, each mutant, and derived native source trees are sealed before each
compiler invocation and checked in finally. Mutant expected hashes are derived
from the single intended substitution before writing, never learned afterward.
The full compiled source closure and approved tool hashes are recorded.

RV32 symbols retain runtime borrowed raster trait calls and sfnt/options policy.
The generic full-wrapper probe is supplied but requires a real future target
backend to instantiate it. These probes are not native target backend, linked
firmware, resource-budget, allocator-routing, physical reading or device evidence.
Host scratch is 1MiB per slice and is not a target memory budget. Variable fonts,
shared multi-face property interleaving, TTC, more corrupt/native short-read and
allocation-failure sweeps, allocator accounting/sanitizers, target native routing
and hardware checks remain pending as listed in STAGED-HISTORY.md.
