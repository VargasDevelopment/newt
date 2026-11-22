This family translates PNGLE and the PNG/RowScaler paths of `ImageRenderer.cpp`
from SDK `cd6f5b831c8fad899723b6d406586f00c575f021`, using Newt reference
`bc9ffda52b85f277e46c63718ac88d62fba5d368`. `PROVENANCE.json` authenticates the
frozen sources and notices. The Rust runtime uses the shared Rust inflater and
ZIP reader; it does not call the native PNG decoder.

`pngle::Decoder::feed` preserves incremental chunk parsing, CRC policy, legal
color/depth combinations, PNG filters, palette/transparency, gamma policy and
Adam7 callbacks. Decoder state and every large buffer are caller-owned.
`image_renderer::render_png` preserves the original grayscale conversion,
alpha-on-white blend, row callbacks, stop/error ordering and box scaler. The
image wrapper's interlace rejection is separate from the underlying decoder's
Adam7 support. Incomplete-input behavior follows the pinned implementation;
this port does not silently turn its permissive wrapper into a PNG validator.

Buffer capacity is an explicit Rust caller contract. It is not evidence of
equivalent native arena charging, available X3 heap or safe selected capacities.
Callbacks return a distinct `CallbackError`; they cannot impersonate the
decoder's retryable capacity result. Callback failures stay failed across later
buffer loans. No allocator, raw pointer, `Rc` or `RefCell` exists in these core
modules. Target `Math::pow` implementation and resource acceptance remain pending.

The pinned C decoder has signed-shift undefined behavior in its big-endian
integer load. Every ordinary complete PNG can reach it through high-bit CRC
bytes. The oracle compiles a separate output copy with exactly four explicit
`uint32_t` byte promotions, records the old/new source hashes and keeps ASan and
UBSan enabled. Frozen/vendor sources are untouched. Results are explicitly
normalized-native comparisons, not pristine-native full-corpus evidence.
[NATIVE-DOMAIN.md](NATIVE-DOMAIN.md) also records the uppercase `BKGD` spelling,
the unsafe native RGB background branch and dangling gamma-table state.

Independent expectations start from authored samples before filtering or zlib
packing, plus literal read/callback/row traces; neither executable supplies the
expected pixels. The family separately runs the heterogeneous normalized-native
corpus in both inflater modes. A mutation must compile freshly on the actual
shared core and transport, exit cleanly, produce an admitted complete response
with empty stderr, and have a semantic witness in both modes. Build failures,
diagnostics, panics, malformed output and surviving edits are gate failures.

Borrowed target exports retain actual feed and decoded-output bodies through
optimized RV32 static-library LTO. This proves emitted code only. JPEG decoding,
the native font backend, linked firmware, flash/RAM/stack limits, battery/power
lifecycle and device performance remain separate requirements.
