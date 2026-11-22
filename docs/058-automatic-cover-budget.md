# Automatic covers: measured decoder budget and the PNG decline

> Port status: ported from draft PR #20 (source `fd678dd`, draft `4d42cc8`)
> onto `v0.1.0-alpha.0` (`f008308`) as isolated, host-only evidence. The probe
> now runs in the host gate on the Linux host with the project-local LLVM
> toolchain. Its baseline-JPEG route cases (direct, stored ZIP, deflated ZIP,
> extraction and staged decode) must be arena-only and sanitizer-clean; its PNG
> cases are retained as asserted negative evidence for why the producer
> declines PNG on the unpatched SDK. The decoder route chosen for the ported
> producer is recorded in `059-automatic-cover-port.md`. The historical
> narrative below is the original handoff, updated where the port's
> measurement method changed.

This isolated experiment started at the unchanged reading baseline
`7d0672a2d5d02c1e9358718e8970291bf79a2e6a`, on branch
`perf/automatic-library-covers`. There are no changes to firmware, shared
renderer/API, reader/page cache, lifecycle hooks, toolchain hooks, pinned SDK,
UI pixels or hardware. This is a measurement record, not an implementation or
device-readiness claim.

## Exact production slice

The host probe compiles the clean pinned FreeInk SDK
`cd6f5b831c8fad899723b6d406586f00c575f021` without modifying it and without
extra preprocessor definitions beyond `FREEINK_BOOK_SMALL=1`, with ASan and
UBSan. It invokes the real `ZipCatalog`, `ZipEntryReader`,
`ImageRenderer::render` and `ditherRowOrdered` against authored PNG/JPEG bytes,
stored/deflated EPUB members carrying the JPEG, and a staged extracted image.
The fixed destination is the existing 144 by 188, 3,384-byte 1bpp raster.
Fixture metadata is generated; full EPUB metadata discovery is costed by the
component suites in `059-automatic-cover-port.md`. No user artwork or books are
read.

The original handoff intercepted pngle's `calloc/free` with preprocessor
redirection. The port instead observes the process allocator through the
sanitizer runtime's malloc/free hooks around each extraction/render call, so
the SDK compiles unchanged and every libc allocation is seen, not only
pngle's. This observes requested host payload; it is not an approved production
allocator, an allocation denial mechanism, or an ESP heap measurement. Decoder
scratch high water is usage, not reserved capacity. The probe keeps a separate
48 KiB fixture metadata array and raster on its host stack; those reservations
are not hidden inside the decoder figures.

| Authored case | Reserved scratch | Scratch high water | libc heap requested | Output raster |
| --- | ---: | ---: | ---: | ---: |
| Baseline 640 by 960 JPEG, direct or staged | 32,768 B | 12,784 B | 0 B | 3,384 B |
| Stored ZIP member carrying that JPEG | 32,768 B | 12,784 B | 0 B | 3,384 B |
| Direct deflated ZIP member carrying that JPEG | 65,536 B | 55,984 B | 0 B | 3,384 B |
| ZIP extraction alone | 49,152 B | 43,200 B | 0 B | none needed by extraction |
| 640 by 960 RGBA PNG (negative evidence) | 8,192 B | 5,744 B | 43,904 B | 3,384 B |
| 4 by 4 gray PNG (negative evidence) | 8,192 B | 5,248 B | 41,482 B | 3,384 B |

A 16 KiB arena still rejects direct deflated decoding before image decode
(SDK status 7, out of memory, no rows). Staging is therefore required for
deflated members so two inflate states never coexist with the decoder; stored
image members bypass staging. The JPEG measurement is one baseline fixture at
one scale; dimensions and downscale choice change its band cost. The producer
sizes that band from the validated frame header (see `059`).

The approved existing RISC-V toolchain separately compiles the native C3 ABI:
pngle state **41,264 B**, ZIP `tinfl_decompressor` **8,364 B**, JPEG `JDEC`
**128 B**. Host pngle state is 41,336 B. Native measurements are compile-only
sizes, not physical heap, stack, battery or latency measurements. The existing
PNG renderer never enables display gamma; the 16-bit/gAMA fixture does not
allocate a 64 KiB gamma table.

Extraction and decode run as separate calls and produce all **3,384 raster
bytes identical** to direct and stored decoding (also recorded CRCs). This
establishes phase separability and pixel parity for this authored case. The
integrated job's release ordering, publication and lifecycle are tested
separately in the component suites.

## Confirmed SDK behaviors

* Unpatched pngle obtains its ~41 KiB state from libc `calloc` (pinned
  `ImageRenderer.cpp` `renderPng` calls `pngle_new`), outside any caller arena,
  so a producer charge cannot bound or deny it. The baseline JPEG route makes
  no libc heap call at all.
* Valid authored PNGs trigger UBSan at pinned `pngle.c:144`: a byte promoted to
  signed `int` is left-shifted by 24. The probe asserts these diagnostics occur
  only in PNG cases and only at pngle sources; JPEG route cases must be clean.
* PNG truncated inside IDAT returns SDK `Ok` with **zero rows**. A PNG lacking
  its IEND chunk returns `Ok` with all **188 rows**. Counting rows alone is not
  a complete-file validity check. A corrupted final chunk CRC is rejected.
* Returning false after two row callbacks releases allocations but returns SDK
  `Ok` for both PNG and JPEG. A generator needs an explicit cancelled flag that
  prevents publication. This does not prove input/sleep cancellation latency,
  idle scheduling or prompt release under firmware conditions.

These are the reasons the ported producer declines PNG on the unpatched SDK.
The draft's SDK patch (`patches/freeink-cover-decoder.*`) addresses the heap,
sanitizer and completeness findings for PNG; it stays tracked and host-tested
but unlinked, pending an owner decision. Existing third-party licenses and
notices remain untouched.

NLC2 remains 3,412 bytes; size/time and payload CRC do not establish EPUB
content identity. Nothing here authorizes reading or replaces the reader's full
source identity/admission. No whole-library scan or retained catalog pointers
were introduced.

## Reproduction and evidence

Run `python3 -B tests/cover_decode_budget_probe.py` (part of
`tools/check.py`). It exits 0 only when the seven JPEG route cases are
arena-only and sanitizer-clean, the four complete JPEG decodes are
byte-identical, and the seven PNG cases still reproduce their libc heap demand
and pngle-only sanitizer diagnostics. All case observations
and source/fixture hashes plus tool versions are written to the ignored
`.build/cover-decode-budget/host-probe.json`. PNG, JPEG and ZIP bytes are
authored and deterministic. The original handoff converted the PNG with the
macOS `sips` codec; the port writes the same RGB pattern through the stdlib
baseline 4:2:0 encoder in `tests/cover_jpeg_fixture.py`, so the JPEG needs no
platform codec and is byte-stable across hosts.

Run `python3 -B tests/cover_decode_native_sizes.py --compiler <approved-existing-riscv32-esp-elf-gcc>`.
It verifies clean pinned SDK bytes and writes native sizes, exact invocation and
hashes to the ignored `.build/cover-decode-budget/native/native-sizes.json`; it
installs nothing. No four-variant firmware candidate or physical tests are
claimed: the producer is not linked into firmware. Parent retains all hardware
and promotion ownership.
