# FTFONT1 host protocol

Both programs read a complete UTF-8 request before any library calls/output.
Exactly `FTFONT1\n`, one nonempty lowercase hexadecimal font line, then 1–4096
commands, each LF terminated, with single ASCII spaces and canonical decimal
integers. CR, missing final LF, unknown/extra tokens, leading zeroes, signed zero,
noncanonical signs, invalid UTF-8, oversized requests and out-of-range fields
exit 2 with empty stdout/stderr. A crash or sanitizer diagnostic is a failure.
Maximum input 4 MiB, maximum decoded font 1 MiB. Input is borrowed by both engines.
The host owns 1 MiB per Rust scratch slice; this is host test capacity, not a
firmware allocation recommendation.

Commands:

| Command | Arguments | Reply |
|---|---|---|
| INIT | stream(0/1), px(u16), weight(i32), italic(0/1), maximum read size(0 unlimited, <=1MiB) | INIT ok |
| OPTIONS | hint(0 default/1 none/2 auto/3 light/4 native), version(u8), mono(0/1), darkening(0/1), embolden(i32), slant(i32) | OPTIONS supported |
| METRIC / GMETRIC | codepoint / glyph, size26_6 (u32) | METRIC 0, or METRIC 1 advance26_6 left top width height |
| RASTER / GRASTER | codepoint / glyph, size26_6 | RASTER 0, or RASTER 1 width height xoff yoff advance packed-coverage-hex |
| LINE | size26_6 | LINE 0, or LINE 1 ascender descender height |
| KERN / GKERN | left/right codepoints or glyphs, size26_6 | KERN signed26_6 |
| LIGA | left/right codepoints | LIGA Unicode-result |
| LIGAG | two codepoints, third codepoint (0 selects two inputs) | LIGAG glyph-result |
| BUDGET | GSUB/GPOS maximum streamed bytes (<=1MiB each) | BUDGET |
| RELEASE | 0 GSUB / 1 GPOS | RELEASE |
| INSPECT | 0 memory / 1 stream, family capacity (0..4096) | INSPECT status familyLength weight italic full-zero-initialized-buffer-hex |
| ALLOC | successful native allocation attempts before failure (u32::MAX disables) | ALLOC |
| STATUS | none | STATUS ready initStage initRaw glyphStage glyphRaw |
| DEINIT | none | DEINIT |

Numeric fields otherwise are u32. Empty coverage/buffers encode as `-`. Output
starts `FTFONT1`, contains one reply per command and ends `END`, all LF terminated.
Stages use the original FtFont enum discriminants. Inspect statuses are 0/-1/-2.
Each runtime must pass canonical Run.execute (exit 0, empty stderr), then complete
protocol admission, before comparing either parity or independent expectations.

ALLOC counts are intentionally not exposed: the native adapter allocates an
explicit face/stream handle while original FtFont uses different wrapper blocks.
All native allocations use the selected callback family. Tests compare status
classes and recovery, not allocator-internal block counts. The host buffers and
protocol containers are host transport allocations outside that allocator.
