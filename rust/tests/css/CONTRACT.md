# CSS supported caller and resource contract

## Scope and API

src/lib.rs translates the actual 100-line Css.h / 427-line Css.cpp at SDK
cd6f5b831c8fad899723b6d406586f00c575f021. frozen files were obtained via
`git -C vendor/freeink-sdk show COMMIT:path`, not the patched live tree.
provenance.json records exact SHA-256 values; LICENSE and frozen/NOTICE retain
original notices. BookCatalog.cpp is frozen as persistence evidence only.

The crate is no_std, forbids unsafe code and allocates nothing. It depends by
path on the sibling ZIP integration newt-core. Do not copy its inflater or
reader into this crate. The core path intentionally remains a parent-controlled
integration boundary while that work is undergoing its full controls.

CssDecl, TextAlign, CssRule, CssStylesheet, CssStylesheetBuilder::begin /
add_text / add_sheet / finish / skipped_sheets, parse_inline_style, cascade_for
cover the original public functionality. rule_count() is the borrowed slice
length; builder output is at most 512. Defaults differ intentionally between a
default empty stylesheet (hash 0) and a fresh builder (FNV seed), as in C.
All 12 declaration fields are retained with original scalar types (including
bool, i8/i16/u16 and TextAlign repr(u8)); rule hashes are u32.

## Supported caller and arithmetic domains

Raw CSS is a bounded byte slice (C uint32 length domain); it is not UTF-8 text.
Inline, element and class APIs accept None for null; otherwise the slice is a
bounded C string view, ending at its first NUL or at the slice boundary (the
latter models an implicit accessible trailing NUL). add_text includes embedded
NUL bytes and hashes them. Source read counts must obey BookSource's bounds.
Caller-constructed sheets must contain valid typed declarations and no more
than uint16::MAX rows. The builder itself always limits output to 512 rows.

A sheet borrowed from finish cannot coexist with a mutable builder borrow.
This deliberately excludes C's legal mutation of rule storage after publishing
a pointer-based sheet. This is a Rust supported-caller restriction, not an
assertion that such C use is undefined. Re-begin requires a new accessible
mutable slice and resets hash, count and skipped count, even on failure.

Unsigned FNV multiplication and u16 skipped increments wrap. Signed decimal
accumulation and percentage arithmetic use checked i32 operations: if an
operation would overflow, the numeric declaration is ignored. Original signed
arithmetic overflow is outside the original defined arithmetic domain; no C
behavior or C equivalence is claimed there. Huge numeric vectors stay Rust-only.
Original truncation is preserved (three consumed fractional digits maximum,
integer percent truncation, px /16, pt /12, negative supported lengths become
zero, size clamp 400 and margins/indent clamp 1000). Bare zero is special;
positive unitless numbers and nonexact units are invalid. Weight uses only the
first byte 1..9. Font-size keywords deliberately retain a stale root-relative
flag; a successful numeric size updates it. Zero/invalid sizes leave it alone.

Space is exactly 09/0a/0c/0d/20, excluding vertical tab and non-ASCII spaces.
Comments are skipped only between top-level rules, not inside declarations or
selectors. At-rules are skipped by literal brace depth, without quote parsing.
Selectors use the rightmost token separated by those spaces or > + ~, without
implementing a web selector grammar. '*' is stored as zero hashes and never
matches. Unsupported selector tails are rejected; declarations remain tolerant.
Rules apply in element, class, element+class specificity passes and forward
source order; only the first eight class tokens are hashed; inline overlays last.
The content hash sees all add_text bytes even ignored syntax and capped rules.

## Resource mapping and cleanup

C begin reserves 512*sizeof(CssRule) arena bytes with alignof(CssRule); Rust
begin requires 512 preinitialized typed slots and is inert when fewer are
supplied. Supplying 511 slots is not an ABI byte comparison. No unsafe byte
arena casts, MaybeUninit conversion, raw serialization, 64KB stack text array,
or inflater-state construction is hidden inside any runtime function.

C addSheet first skips uncompressed size >65536 (including failed begin), then
opens ZIP reader, then reserves uncompressed_size+1 text bytes. Rust follows
that exact status order using the shared Reader and a borrowed text buffer of
that minimum length. The extra byte is capacity only, just as C never writes a
terminator. Reader errors become IoError, irrespective of partial output;
there is no hash/rule publication on error. A successful early deflate EOF
parses only bytes actually returned, exactly as C. Failed begin still performs
source I/O and may return errors. Oversized sheets perform no source I/O and
wrap the skipped counter. Temporary Reader and all its resource borrows end
on every return, leaving caller resources available for reuse. Scratch bytes
and decoder contents may change, as they do in C; the API does not clear them.

C scratch is one bump arena; Rust has disjoint caller-owned typed state,
32768-byte dictionary, 2048-byte input, and text domains. Decode omission models
C decoder OOM, not a claim of equal byte budgets. Host oracle maps these to C
capacities by simulating the three actual decoder allocation sizes/alignments,
then adding the text capacity. A 3-byte existing allocation verifies that every
C addSheet exit restores its mark. Rust SCRATCH 1 denotes released borrows,
not a byte-arena high-water measurement. The transport covers both text and
decoder exhaustion and precedence against bad headers/methods/source I/O.
Arbitrary fragmented C arena capacities and partial decoder allocations have
no one-to-one typed Rust resource representation and are not claimed equal.
The shared ZIP controls remain separate evidence; CSS results cannot replace them.

## BookCatalog persistence boundary

The pinned catalog writes sheet.ruleCount*sizeof(CssRule) bytes directly and
loads rows with allocArray<CssRule>; see frozen BookCatalog.cpp around
1109-1133 / 1280-1292. Therefore dropping fields or flattening sentinels loses
stored data. This Rust stage retains every semantic field but does not derive
raw POD serialization or assert Rust/C layout equivalence. `tests/oracle.cpp
--abi` emits C sizeof/alignof plus offset/width of every declaration and rule
field. Parent must collect that on each supported catalog-producing ABI,
including target compiler, and establish byte order, enum/bool encoding,
padding initialization/history and cache version policy before any compatible
field-wise codec. Host layout alone is not target persistence evidence.
No raw Rust memory is written to or read from a catalog in this stage.


Integration: the implementation is now newt-core::css. Probe bodies are in
the target-probe crate, and host transport/core tests use the same engine.
