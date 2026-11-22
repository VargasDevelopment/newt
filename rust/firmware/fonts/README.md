# Native font boundary

`native.rs` is the one Rust FFI backend for `newt_core::ft_font::Backend`.
The host proof (`host/src/bin/ft-font-host.rs`) and the RV32 borrowed probe
(`tests/ft_font/target/probe.rs`) include this same file with `#[path]`. It is
`no_std`, depends only on `newt_core`, and adds no crate, Cargo dependency,
build script, writable global, cache or resident buffer. Target selection
retains a constant allocator-callback table; its firmware cost is unmeasured.

`native/native.c` is the primitive FreeType adapter. Policy stays in
`core/src/ft_font.rs`. FreeType itself is the pinned FreeInkFont vendor copy
(FreeInk SDK `cd6f5b8`, `libs/font/FreeInkFont`), under FreeType's FTL, which
travels with that source. Nothing from FreeType is copied here.

## Allocation

The caller creates one `Library` and keeps it alive past every face.

- Host: `Library::host()` (libc) or `Library::with_callbacks` (the gate's
  fault-injecting allocator). Unchanged.
- Target (`target_os = "none"`): `unsafe Library::font_heap()` passes the
  static callbacks from `nf_font_heap()`. They call the pinned
  `fiFontMalloc/fiFontRealloc/fiFontFree` (FontAlloc.c), which is exactly what
  FtFont uses when no callbacks are configured. On the C3 there is no PSRAM, so
  these fall back to the internal 8-bit heap. `Library::host()` does not exist
  on target, and `nf_library_new(NULL)` returns error 6 under `ESP_PLATFORM`.
  A RISC-V or Xtensa compile without `ESP_PLATFORM` fails, so the target can
  never fall back to libc without a sign.
- The caller must make sure the SDK heap is initialized and that it runs in
  ordinary task context. Rust cannot witness this, so `font_heap` is `unsafe`.
- `native.c` refuses an SDK configuration that aborts on allocation failure,
  because FreeType reports out-of-memory and expects to recover.

## Stream

`BookStream` lends any `BookSource` (for example a bound `FileBookSource`) to
FreeType. The adapter makes one source read, preserving positive short results,
as pinned `FtFont.cpp::ftStreamIo` does. Zero, negative or excessive results
return zero. The native stream marks any result shorter than requested as
failed. `inspect` then reports `Unavailable`, and font loading fails.

Rust lifetimes enforce the order: the file and `FileBookSource` outlive
`BookStream`, which outlives `StreamContext`, which outlives `Native`/`FtFont`.
`Library` outlives all of them. Dropping `FtFont` closes the face
(`FT_Done_Face`), then the face record, before any borrow ends. Cancelling
through `FileBookSource` returns -1, which FreeType sees as a failed read.
Release the source only after the font is dropped.

## Default off

Nothing in the default target closure includes this module. Firmware selects it
explicitly. Until it does, the reader holds no FreeType library or heap. The
FreeType render pool (`FT_RENDER_POOL_SIZE` 4096) is on the caller's stack. The
render task's stack and heap peaks are unmeasured.

## Scope

Host evidence covers the host ABI only. The `const` and `_Static_assert` layout
checks run again in the RV32 Rust compile and the SDK C compile. Successful
compilation establishes those asserted layout facts under the recorded
configuration. Calling convention, final ABI/linkage, retention, stack/heap
measurement and hardware remain separate obligations. These compiles are pending.

MIT (root `LICENSE`).
