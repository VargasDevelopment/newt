# NRUST-44 source-only handoff

No compiler, test, gate, firmware build, device, acquisition, commit or planner
operation was run. Only Rustfmt parsing/formatting and Python AST parsing were
performed. Every new file is under this ignored staging directory. This is an
implementation candidate, not validated port or firmware/readiness evidence.

`ft_font.rs` is a safe, allocation-free wrapper module using the accepted
`newt_core::page_renderer::{Font,RasterFont,GlyphBitmap}` and `font_layout` table
readers. `lib.rs` is the no_std crate root. `native.c` and `native.rs` form the
actual FreeType adapter. `host.rs` and `oracle.cpp` implement PROTOCOL.md. The
oracle links the authenticated original FtFont.cpp, Gpos.cpp, Gsub.cpp and
FreeType. The Rust binary links only the new Rust wrapper, native.c and FreeType
C. Never include original FtFont/Gpos/Gsub C++ objects in the Rust link.

The source binding is SDK cd6f5b831c8fad899723b6d406586f00c575f021 with translation
reference bc9ffda52b85f277e46c63718ac88d62fba5d368. `inputs-start.json` authenticates
the complete existing 408-file closure. `original/` preserves the wrapper and
license/notice sources. `newt-pin/` materializes the immutable route configuration
and patch pair. Both pinned regional/producer patches leave the font wrapper,
FreeType configuration and source unchanged. Newt's one pinned firmware route
uses the composed regional-dev-usb SDK; do not follow moving main or route the
target directly to a different vendor tree. None of the three optional hint/mono
flags is added by the pinned platformio.ini; recipe.py also tests all enabled.

## Ownership and API correspondence

| Original wrapper API | Rust surface / authority |
|---|---|
| configureMemory / ensureLib | Explicit Library::with_callbacks creates one shared native library and freezes allocator; Library::host chooses host libc. Callback context outlives library and faces. No global Rust cache or implicit library. |
| init / initStream | FtFont::init(Some(bytes),...) / init(None,...). Native::stream borrows stable caller StreamContext containing a borrowed ReadAt. Source is never closed or copied. |
| destructor / deinit | FtFont Drop/deinit closes backend face. Native Drop is idempotent. Native Library lives beyond all faces. |
| ready / lastInitFailure/Error / lastGlyphFailure/Error | ready() and explicit recorded status fields |
| inspectMemory / inspectStream | inspect(backend, Some(bytes)/None, family_slice), returning status and FaceInfo; backend closes temporary face; scalable/Unicode validation and family truncation preserved |
| applyVariation / synthetic styles | safe Rust bounded 16-axis coordinates, wght/ital/slnt and synthetic fallback; native FT MM allocations freed with same library |
| setRenderOptions / flags / shared properties | Rust options/capability report and flags; darkening and interpreter reapplied immediately before every load; unsupported requests remain stored |
| glyphId / hasGlyph | glyph_id / has_glyph and accepted trait covers |
| metrics26_6 / metricsGlyph26_6 | metrics26_6 / metrics_glyph26_6, fallible Option replaces output-reset plus bool |
| lineMetrics26_6 / integer metrics | line_metrics26_6 and accepted Font methods |
| kerning26_6 / kerningGlyphs26_6 | kerning26_6 / kerning_glyphs26_6: native legacy first, borrowed/raw Rust GPOS fallback, native FT_MulFix scaling |
| ligature / ligatureGlyphId | ligature_codepoint / ligature_glyph_id and accepted Font method |
| budgets / release | set_gsub_byte_budget / set_gpos_byte_budget / release_ligature_table / release_kerning_table |
| rasterize variants / mono expansion | accepted RasterFont, rasterize26_6 / rasterize_glyph26_6; coverage borrows font until next mutable call |

Intentional ownership adaptations: GSUB/GPOS/mono Rust buffers are caller scratch,
not native-owned implicit growing allocations. Insufficient scratch disables a
streamed table or reports BitmapBuffer, without allocating. Resident tables stay
borrowed regardless of stream budget. Native face and stream bookkeeping is one
caller-allocator block rather than the original two stream blocks. Explicit
Library creation exposes library allocation failure before any FtFont exists;
it replaces process-static initialization, not FreeType implementation. A failed
SetSize immediately closes native source wrappers rather than retaining them
until deinit. Negative/padded grayscale rows are normalized just like mono into
caller scratch to satisfy accepted tightly packed GlyphBitmap. ReadAt cannot
outlive or move its context while borrowed. No Rust Rc/RefCell, font-byte heap
copy or hidden C++ wrapper is used. Library is shared on one thread and is !Sync;
backend calls must remain synchronous, and callbacks must not unwind/reenter.

## Prepared validation recipe (do not overlap the Lead's current LOCAL gate)

Root owns the coordinated flock, shared toolchain lease and canonical Run.
After acquiring its agreed existing root lock, pass that descriptor/path through
NEWT_FT_LOCK_FD and NEWT_FT_LOCK_PATH. Hold
`tools/rust-toolchain.py:lease(shared=True)` across verification and the full
recipe. Install canonical signal handlers, verify_tools, create a new output
folder below .build/rust, instantiate canonical Run(output,tools,deadline), and
call recipe.check(root, runner, output). No package manifest/tool installation is
needed. The recipe freezes sources before builds, runs Cargo offline/frozen/
locked/jobs=2 and tests with 2 threads, uses Run.run for compiler work and
Run.execute for every runtime, then snapshots source/tool identities again.
Canonical Run owns process groups and INT/TERM cleanup. Root must keep the
coordinated lock and shared toolchain lease until cleanup and final verification
finish. The recipe has no standalone main and deliberately cannot claim it
acquired the outer lease on root's behalf.

`fixture.py` builds an original synthetic TTF with Python stdlib, using explicit
rectangle coordinates/UPEM, cmap, GSUB and GPOS. No book, SD card, font download or
unlicensed asset is needed. Its independent expected 10px rectangle is width4,
height7, xoff1, yoff-7, advance6, 28 full-coverage bytes; linear advance384,
line metrics512/-128/768 and GPOS kern-64. `core_tests.rs` uses a clearly labeled
primitive model for negative pitch, mono scratch failure, axes, flags, native
stage errors and bounds, never as the real host rasterizer. Callback library
allocation rejection is a separate native test. Mutations must compile fresh,
pass clean complete transport admission and differ at the named witness. No
compiler failure/crash/diagnostic can kill a mutation. `rv32_probe.rs` contains
borrowed runtime raster loops and full wrapper operations over runtime input;
the recipe compiles the non-generic trait-call probe for RV32. The generic full
wrapper body must also be instantiated with the actual future target backend.

## Remaining gaps the Lead must close

1. Compile both transports and unit tests; run the prepared cases and all fresh
   mutation controls. Resolve any compiler/parity/fixture/recipe defects before
   integration. The recipe itself has only been AST parsed.
2. Add native variable-font coverage (or a spec-built fvar/gvar fixture), shared
   multi-face property interleaving, TTC and corrupt-table/font cases, native
   short-read mid-face/table and allocation-failure sweeps. The bounded-axis and
   rare backend errors currently have primitive unit coverage, not native
   heterogeneous witnesses. Add allocator accounting/leak assertions/sanitizers.
3. Exercise font reinit with different font bytes (not only the same fixture),
   family weight/italic metadata variants, non-Unicode/non-scalable rejection,
   chained ff/ffi/ffl and glyphs with no Unicode ligature mapping. The wrapper
   implements these paths but present native fixtures do not establish them all.
4. Wire into the actual root core/host/probe modules, replacing external
   `newt_core::` with `crate::` inside the core as appropriate. Add manifests,
   gate registration, exact bindings and controls there only after review.
5. Implement/review the target backend and original platform allocator routing,
   target library initialization/serialization and source ownership. Measure
   scratch/cache capacities, native allocations, flash/RAM/stack/RTC under the
   resource policy. Host 1MiB scratch buffers are not target budgets.
6. Run required root/local/firmware gates when authorized and available; separate
   hardware evidence from preview/compile evidence. A staged library compile or
   RV32 borrowed probe cannot establish target FreeType, firmware readiness or
   physical reading behavior.

The full wrapper implementation is supplied. FreeType remains licensed native C,
not a claimed safe Rust FreeType port. Target backend and memory acceptance are
explicitly pending.
