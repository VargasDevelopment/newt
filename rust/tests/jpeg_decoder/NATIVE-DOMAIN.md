Defined domain: pinned JD_FORMAT=2, JD_FASTDECODE=1, JD_USE_SCALE=1,
8-bit baseline sequential single interleaved scan; grayscale or YCbCr with Y
sampling 11,21,22 and Cb/Cr11. Four quant tables, DC/AC Huffman table IDs0/1,
scales0..3, restart intervals and cyclic RST0..7. Pinned native selects Huffman
class by component position, ignoring SOS selector choice after validating
00/11; Rust preserves this. Component identifiers are likewise positional.
Grayscale means Y directly, cast to u8: no RGB conversion or clamping occurs
in this SDK configuration. Chroma entropy is consumed even though no chroma
IDCT contributes to grayscale output. No alternate RGB565/RGB888 build claim.

Rust rejects native undefined arithmetic using Error::Arithmetic(9): i32
coefficient*dequantizer overflow and IDCT signed intermediate overflow. Native
sanitizer remains fully enabled, frozen source unmodified. Native unchecked
short SOF/DRI records, an SOS shorter than its count plus one table byte per
component, Huffman tables over 256 symbols (the fixed caller slot), non8-bit
precision and invalid table symbol lengths are not equality domains: safe Rust
checks lengths/bounds and rejects. Repeated DQT/DHT definitions overwrite
fixed caller slots; native repeatedly consumes its pool and can report MEM1.
Exact native pool exhaustion parity is pending. Input implementation must obey
requested slice bounds; invalid count rejects.

Like native (#316), Rust ignores the SOS spectral/approximation bytes and any
SOS bytes after them, and accepts over-subscribed Huffman counts. Native's
uint16 code words wrap only after every bit pattern already matches a shorter
code, so both select the same symbols. The DC predictor narrows as native's
does: the stored predictor wraps to i16, the current block keeps the full sum.
rust/core/tests/cover_tolerance.rs pins the frozen C++ cover session's raster
CRC for each of these cases.

Current safety definition is conservative: C int16 narrowing is implementation
specified; Rust uses explicit two's-complement casts for IDCT output and the
stored DC predictor, as the pinned compilers define them. Native C signed
right shift follows the authenticated
Clang/RV32 arithmetic-shift convention. The shortcut divides DC by256 with
truncation toward zero; IDCT shifts negative values arithmetically.

Progressive preview is a separate safe Rust translation of ImageRenderer's
SOF2/DC first-scan path, not TJpgDec and not a full progressive coefficient
buffer. It preserves restart acceptance (any RST0..7), chroma consumption,
DC quantization, successive low-bit scaling, endpoint sampling and consumer
stop success. Header and restart read failures can remain ParseError while
entropy and segment-marker IO failures become IoError, as in the original.
No blanket IO override is applied to this path.

The preview requires nonzero placement dimensions. Typed borrowed capacity
failures are OutOfMemory; this is not native Arena/NativePool byte-for-byte
resource admission. NativePool remains a separate resource domain. Four
Huffman tables are borrowed in the safe core; the ABI wrapper holds them on
its stack. No default firmware feature or resource policy is changed.

The frozen C++ has undefined behavior for out-of-range table selectors,
overflowing DC multiplication, and negative signed DC left shifts. Pinned
JD_FASTDECODE=1 builds no lookup table, so over-subscribed Huffman counts are
defined there. Such inputs are outside sanitizer-clean differential
equality; Rust bounds-checks/rejects the former and defines checked signed
multiplication for DC scaling. Generated parity fixtures use nonnegative
predictions (negative differences are exercised). Full malformed-input safety
proof and exhaustive C++ undefined-domain classification remain pending.

JPEG detection is the JPEG branch of frozen ImageProbe with its 26-byte
prefix, short-read loops and 128 KiB scan bound. Protocol A probes then reopens
a raw entry and selects baseline/preview. It is not the full image catalog
visitor/dispatcher: PNG dispatch, catalog lookup, placement from PageImage,
Zip decompression integration, firmware startup and physical rendering are
still absent. Protocol I exposes JPEG probe dimensions/flag independently.
Protocol P invokes the preview directly, R baseline rows, D
baseline block decoding. A's dimensions come from the request placement.
