# JPEG actual-core candidate (source only)

All new work is confined to this ignored owned directory. The candidate began
as an exact copy of `.build/rust/combined-core-seed-01/rust/`, verified against
its frozen `source-sha256.json`. No live Root sources were copied afterward.
`seed-source-sha256.json` records that starting point. Existing seed files,
including its canonical Run/cargo_context, remain unchanged except the two
module registration files listed in INTEGRATION.md.

No candidate compiler, test, gate, packet module, linker or device operation
was executed. Python was used only for copying, hashing, JSON and AST parsing.
Donor evidence is historical: `.build/rust/jpeg-progressive-02/receipt.json`
reports 189 passing cases, 12 clean semantic controls and freestanding RV32
retention. That evidence is not evidence that this adaptation builds or passes.

The actual `newt-core` owns `jpeg/{mod,tables,renderer,progressive,detection,
runtime_probe}.rs`. It is safe, allocation-free and no_std, with caller-borrowed
workspace and the existing core image_renderer::RowScaler/page_renderer types.
There is no nrust dependency, nested core, Rust-to-C decoder call, allocator,
new package, lockfile edit or invented general image dispatcher.

`D` preserves the baseline decoder domain; `R` preserves the baseline renderer;
`P` preserves the pinned first-scan progressive DC preview (not full progressive
reconstruction); `I` probes JPEG metadata. `A` is only a JPEG probe/reopen wrapper
with original baseline/progressive dispatch. The existing core image_probe API
is separate. No whole-book or all-format acceptance is claimed.

The corpus and independent expectations are copied without changes, including
all baseline higher-AC, nonuniform quantizer, distinct-MCU, EOF and vertical
DC-band stop=1 cases. No algorithm or mutant was omitted. The band-flush control
must fail the independently authored distinct vertical DC-band expectation.
The native oracle transport and pinned SDK bytes remain exact, including pngle:
JPEG requires no PNG CRC normalization and no vendor file is rewritten.
NativePool accounting, genuine native-target backends/resources, whole firmware
and hardware remain pending. USB, battery and cold-power acceptance are separate.
