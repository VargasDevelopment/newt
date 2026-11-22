# Bounded parity transport

One request, one ASCII line ending in exactly LF. A single space separates
nonempty tokens. No tabs, CR, leading/trailing whitespace, extra lines, plus
sign, leading zero, negative zero, uppercase hex or unconsumed token is allowed.
Exit 2 with empty stdout/stderr means transport rejection; any other failure,
sanitizer output or incomplete reply is fatal. Request limit is 65536 bytes.
Numbers are canonical signed decimal where a negative range is allowed.
Byte slices are lowercase even hex, or `-` for empty.

Token order:

- `PR1 mode width height stride format rotation fill arenaCapacity`
- `fontCount`, then each font: `maxCp missingCp rasterMissingCp advance bitmapAdvance kern
  ligatureCp glyphWidth glyphHeight xoff yoff pixels`
- `entryCount`, then each registration: `bankIndex flags`
- `runCount`, then each run: `x baselineY size style text`
- `imageCount`, then each image: `x y placementWidth placementHeight status
  extraDecoderBytes rowCount`, then each row: `rowY width gray`
- `queryCount`, then each query: `op cp right size style`

Modes: 0=text with firstMissingOut initialized to 999; 1=images; 2=combined
(original discards missing count); 3=query only. Formats 0/1/2 are Dithered,
Sharp, Gray8. Rotations 0/1/2/3 are None, Portrait, PortraitInverted, UpsideDown.
BookStatus numbers match the frozen enum (0..10).

Limits: panel1..64, stride1..64 (must cover panel row), fill0..255,
arena0..4096, fonts0..8, registrations0..8 (valid bank index), runs0..32,
images0..16, queries0..64. Glyph sizes0..8, pixels exactly width*height.
Measured advance/bitmapAdvance/kern/offsets/run/image coordinates -64..64, size0..64, style0..255.
Text0..64 bytes, rowCount0..64, placement sizes0..64, rowY0..65535,
callbackWidth0..64 with exactly that many samples. Codepoints0..2097151;
values may be non-scalars. Exclusion sentinels may use 2097151.

The synthetic backend covers cp<=maxCp except missingCp; rasterization also
returns none for rasterMissingCp. All covered glyphs use the supplied bitmap,
measured advance, bitmap advance, kern and ligature are independent supplied constants. Metrics return size+1 and size-1.
This is an oracle seam, never a font-engine claim. ImageDecoder reserves the
supplied extra bytes, then emits supplied rows and supplied status.

Queries: 0 advance, 1 kern, 2 ligature, 3 strict covers, 4 line height,
5 ascent, 6 selection (bankIndex*256+faceFlags, or -1), 7 styleCoverage.

Exactly three complete LF reply lines:

    R missing firstMissing status completeFrameHex
    T commaSeparatedTraceOrDash
    Q spaceSeparatedQueryAnswersOrDash

Trace font events end with `:bankIndex`: `hCP`, `rCP:size`, `aCP:size:style`,
`kLeft:right:size:style`, `gLeft:right:style`, `lSize`, `sSize`. Image events:
`dIndex:remainingArena`, `iIndex:y:width`, `eIndex:status`. All framebuffer bytes,
including stride padding, appear in the reply. A trace-only mismatch kills a
semantic mutant only after successful compilation, clean exit and complete
canonical protocol. Crashes and parser rejection never qualify.

Reply admission is implemented in tests/replies.py. Event integers are canonical
unsigned decimal: cp 0..2097151, size 0..64, style 0..255; bank/image IDs must be
within that request's object counts, rowY 0..65535, width 0..64, remaining arena
0..4096 and status 0..10. Each event must have exactly the documented arity.
Missing count cannot exceed request text bytes; firstMissing is 0..2097151.
Q has exactly queryCount answers (`-` only when zero). Advance/kern/metrics are
i16, ligature is 0..2097151, covers is 0/1, selection is -1 or a registered
bank/flags pair, and styleCoverage is 0..7. Canonical grammar/domain failure is
fatal before semantic mismatch can qualify as mutation detection. Grammar
admission does not itself claim that an otherwise well-formed trace is correct;
the independent complete expected reply checks that.
