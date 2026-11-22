# XML input foundation contract

Translation pin: FreeInk SDK `cd6f5b831c8fad899723b6d406586f00c575f021`.
`PROVENANCE.json` binds the complete original source/header closure and MIT
license used by the C++ transport. This foundation translates `EntityFilter`,
`XmlUtil` utilities and only `dirName`/`resolveHref` from `PackageParsers`.
It does not parse XML syntax or construct an EPUB package.

`EntityFilter` preserves the original 47 named replacements, case-sensitive
lookup, five XML built-ins and numeric pass-through, unknown-name replacement,
bare-ampersand repair, ten-byte entity-name bound, carry and pending bytes,
512-byte source pulls, zero-capacity no-op, partial destination writes on an
error, EOF repair, and reset without source rewind. The actual accepted Rust
ZIP reader implements its `BytePull` interface. No C inflater or filter is
called from the Rust core.

Qualified-name helpers use the last colon. Attribute lookup takes the first
matching pair. Tokens split only on ASCII space, tab, CR and LF. Text capture
keeps at most 255 bytes; trim inserts a terminator without moving its prefix or
changing its length. Encryption classification is sticky and exempts exactly
the two original font-obfuscation algorithms. These are upstream semantics,
not a general XML standard conformance claim.

Path resolution decodes percent bytes before normalization, splits only literal
fragments, joins the base without pre-normalizing it, and caps working strings
at 511 data bytes plus NUL. Fragment allocation precedes path allocation, so
partial arena use and allocation failure diagnostics are observable. Fragment
slots are cleared even on early failure. Failed `dirName` leaves its destination
unchanged.

The comparison adapter supplies utility callbacks directly and executes the
original entity filter with the original ZIP reader. Full original package
source is linked with unused parser functions discarded; no replacement path
algorithm or fake XML implementation is used in that oracle. Independent
literal expectations cover entity output and complete selected operation traces.
Complete raw-LF replies include destination tails, source pulls, reader totals,
text capture storage and arena diagnostics. Malformed requests are a separate
transport check, never a behavioral mutation witness.

The target probes operate on runtime borrowed filter/capture/scan state,
source trait, input strings, attributes, output slices and arena. Exact nonempty
RV32 exports establish code retention only. They establish neither firmware
linking, target execution nor stack/RAM acceptance. The core allocates no heap;
caller-provided storage and local bounded arrays still consume memory.

## Supported Rust caller domain and limits

Input is arbitrary bytes, including invalid UTF-8. Slice ends supply implicit C
terminators, and embedded NUL stops C-string scans. Full raw chapter byte streams
are length-based; they do not stop at NUL. Percent-decoding can produce NUL: decode
continues to scan the whole href (including overflow/fragment detection), then
joining sees only the prefix before NUL. A literal # splits; decoded %23 does not.
Fragments remain percent-encoded. Base directories are copied verbatim rather
than pre-normalized. Paths cap at 511 data bytes plus a terminator.

Attributes are complete non-null name/value pairs with implicit terminators;
Rust represents their sentinel-terminated C++ array as a slice. attr returns an
input-borrowed C-string prefix. A valid capture has len<=255, begin before storage
inspection, and no external forged field invariants. add_event preserves signed
nonpositive no-ops; positive text_len must fit text. add uses its slice length,
within the C++ int length domain for equivalence. Only initialized bytes through
len are compared. Default Rust zeroes otherwise indeterminate C++ storage; no
claim is made about C++ uninitialized tail bytes.

Sources return 0..dst.len() or a negative value; filter destinations fit i32.
Over-reporting sources and larger destinations are safely rejected, outside the
C++ defined comparison domain. Supported sources eventually return data, EOF or
error; no fairness/performance claim is made. Rust exclusive borrows exclude
source/destination/filter/arena overlap, callback-reentrant mutation, dangling
input/output pointers and views held across arena reset/release. Some disjoint
and valid C++ lifetimes/alias arrangements cannot be expressed by this API;
those are excluded Rust caller uses, not claims of C++ undefined behavior.
C++ invalid pointer, incomplete attribute pair, uninitialized read and invalid
memcpy-overlap cases are outside the defined C++ oracle domain too. ByteRange
handles obey the accepted arena's scope rules: retain no result across reset,
release or reinitialization, and do not transplant handles between arenas.

No DRM policy change: EncryptionScan is only the upstream generic sticky
classification utility, not an encryption implementation or a new access path.
No full XML, Expat integration, package handlers, parser session or firmware is
included in the Rust scope.
