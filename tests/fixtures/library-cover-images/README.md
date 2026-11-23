These four original CC0 UI cover PNGs come from `tools/library-cover-fixture.py`.
Their exact bytes are committed so Linux fixture preparation does not depend on
the runtime zlib compressor. `tools/library-cover-fixture.py` locks their source
and 3384-byte ImageIO payload hashes; the Mac cover suite checks the unchanged
generator and actual ImageIO output against them. Changes require explicit review;
no test or gate updates this corpus or the reviewed UI manifest.

Scope is four 144 × 188 grayscale PNGs containing only tones 0 and 245, without
resizing or color/orientation transforms. This corpus does not establish general
ImageIO, JPEG, arbitrary PNG, color-management or interpolation parity.
