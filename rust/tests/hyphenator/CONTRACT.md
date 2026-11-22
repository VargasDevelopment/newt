# Borrowed FIBH matcher contract

The exact SDK pin is `cd6f5b831c8fad899723b6d406586f00c575f021`.
Complete original Hyphenator.cpp/.h and MIT/NOTICE bytes are bound in
`provenance.json`. No language datasets or dependencies are acquired.

The Rust matcher preserves the original sorted-range search, maximum weight
merge, odd break selection, boundary dots, original case-fold ranges, UTF-8 byte
offsets, character minimums 2/3, 62-byte word bound and capacity-limited increasing
results. Pattern bytes are borrowed for the matcher's lifetime. Initialization
preserves the original header-only success and readiness distinction: an intact
header can succeed even if the following records are corrupt.

Record bounds, nonempty keys and strict ordering are validated once when the
immutable bytes are adopted. The cached validity gates search without adding a
full pattern-table scan to every word. Invalid records and unsupported words
return zero without changing the output. Empty/default/failed initialization
has vacuously valid records but is not ready. This safe Rust rejection policy
outside the supported pattern/word domain is distinct from original-C++ parity;
not every excluded C++ input is undefined.

## Supported native oracle domain

* Header version one, complete uint32 little-endian offset table; native
  `12 + count*4` arithmetic must not overflow uint32. Transport copies all table
  data to uint32-aligned storage retained through every init/search. C++ input
  lengths must fit uint32; nonnull empty init uses valid aligned storage.
* Every offset points within the blob to a complete nonempty key (one-byte
  length), key bytes and exactly key-length-plus-one readable value bytes.
  Keys are strictly increasing lexicographically by unsigned bytes. No duplicate
  keys. Weights may be any byte; real compiled patterns normally use digits.
  `maxPatLen` may be zero or smaller than some stored keys: native ignores those
  longer matches. Reserved header byte is ignored, matching native.
* Words are canonical valid UTF-8 containing only BMP scalars (no surrogate,
  overlong, invalid continuation, truncated sequence or four-byte scalar).
  Tests use synthetic letter runs; valid BMP punctuation/NUL is also initialized
  by native's decoder/encoder, but is not a promise of meaningful typography.
  Empty/short/over-62-byte words return early. All folded BMP scalars preserve
  encoded byte width. Native's four-byte decoder plus three-byte-only encoder
  can leave uninitialized bytes; even valid non-BMP words are excluded.
* Output storage is readable/writable for cap bytes, cap 0..255, and does not
  alias words or tables. The transports keep sentinel tails and verify only
  the returned prefix changed. Rust borrowing enforces nonaliasing.

Within this domain the algorithm stays byte-wise: boundary dots, original
Latin/Greek/Cyrillic folds, per-start sorted-range binary narrowing, shorter-key
ordering, exact-key max-merge, odd weights and character-based 2/3 minima.
Return positions are original UTF-8 byte offsets, increasing and capped.
No trie, language-specific shortcut or pattern expansion was introduced.


The mandatory family executes every independently expected case. Native-domain
cases must pass both transports and match complete raw-LF responses; separate
Rust-only safety cases are labeled and are not counted as C++ comparisons.
Every semantic mutant compiles the actual core and transport freshly, exits
successfully, emits a complete valid protocol, and differs on a native witness.
Malformed requests, compiler failures, panics, timeouts and invalid replies
never count as mutation kills. Both adapters verify untouched output tails.

The RV32 export borrows runtime state/data/word/output and supports reinitializing
or reusing that state. Nonempty code retention is neither target execution nor
firmware, stack/RAM acceptance, complete line layout or measured performance.
