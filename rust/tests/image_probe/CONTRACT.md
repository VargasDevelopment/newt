# Behavioral and resource contract

Authority is the complete frozen original source at the recorded pin. This
port intentionally does not add JPEG/PNG standards validation. Output resets
before open. Open failures take precedence over missing skip storage. An exact
26-byte read is mandatory even if a JPEG SOF or PNG dimensions appear sooner;
any failure of that read returns Ok/Unknown. PNG requires all eight signature
bytes and IHDR at byte 12, with both dimensions in 1..65535; the IHDR chunk
length and remaining image structure are not validated.

JPEG replays the initial 24 bytes after SOI. next() consumes replay first, then
performs an exact physical read of the remainder. skipN() consumes replay first,
then reads physical bytes in chunks of at most 1024. Exact-read consumed counts
advance only on complete reads; skips advance by actual successful read counts.
The loop checks physical consumed <128KiB before each marker, so a marker begun
below that boundary may finish above it. SOI, RST0..7 and TEM are standalone;
EOI/SOS stop. Other markers have a length >=2. C0..CF except C4/C8/CC are SOF.
Five SOF bytes are read regardless of the declared segment length. Zero JPEG
dimensions are retained, and only C2 sets progressive. Failed reads after open
are handled as Ok/Unknown, never promoted to an I/O status.

## Storage domains

The accepted `zip_container::Reader`, `Entry`, `InflateScratch`, and
`zip_window::source::BookSource` are reused directly. The core is no_std,
allocation-free and forbids unsafe code. It follows accepted CSS add_sheet
conventions: decoder scratch is Option<InflateScratch>, and staging is a separate
caller-owned mutable slice. No decoder/state/dictionary initialization happens
inside probe(). Source, entry, decoder state/dictionary/input, staging and output
are all runtime borrows in the target function.

Original C++ borrows a BookArena (the FontArena alias), marks it, opens the ZIP
reader, attempts a 1024-byte allocation even if open failed, and releases the
mark on every return. Stored/raw entries need 1024 arena bytes. Deflated local
entries allocate native sizeof(tinfl_decompressor) aligned to uint64_t, then
32768 bytes aligned to 16, then 2048 bytes aligned to 16, then 1024 bytes aligned
to 1. The oracle starts from a 3-byte arena prefix to exercise alignment and
mark restoration, deriving native decoder capacity using the original Arena.
A missing typed decoder maps to a C arena which cannot admit any decoder
allocation; missing staging maps to the exact decoder end plus staging capacity.
This preserves open/OOM precedence and the 1024-byte admission threshold without
claiming native C and Rust object sizes are equal. Partial individual decoder
allocation failures remain the accepted ZIP family's responsibility; this API
models complete typed decoder availability or absence.

Rust needs size_of::<inflate::State>() plus a 32768-byte dictionary and a
2048-byte compressed input buffer for deflate, separately owned and initialized
by the caller. Staging must provide >=1024 bytes even for a PNG, raw entry, or
short input; additional staging bytes are unused. Missing decoder on a deflated
local entry is OutOfMemory after successful local-header validation; raw invalid
methods and unsupported local methods retain their own statuses. A staging
shortage returns OutOfMemory only after successful open. These checks cannot be
removed merely because Rust can keep small headers on its stack.

Automatic Rust locals include the 26-byte header, 2-byte marker and 5-byte SOF,
stream/replay/Reader metadata and scalar locals, subject to actual compiler
layout. C++ has a 26-byte header, 8-byte seg, 5-byte SOF and native metadata.
No byte/stack/heap/ABI equality is claimed. Root must measure target artifacts;
no embedded memory/resource or performance acceptance is supplied here.

SCRATCH 1 means the C arena used offset returned to its entry mark; in Rust it
means no caller storage allocation/lifetime is retained (the API never receives
an arena). It does not mean buffer bytes or decoder state are unchanged.
Operation output records status, complete ImageInfo, source READ offsets,
requested sizes and returns, and END read count. Strict LF validation rejects
truncation, extra rows, wrong operation order/widths and traces inconsistent with
the declared chunk/fault source. Identical output is compared to the actual
original C++ oracle; mandatory independent expected rows are checked in both.

Case IDs and SHA-256 fingerprints include full requests and independent expected
rows; changed or replaced mandatory cases fail admission. Mutation controls use
unique anchors, fresh actual-core/host compilation, complete clean-exit replies
and named witnesses; crashes, build errors, protocol errors or sanitizer failures
are failures, never semantic kills. The baseline and each witness must differ
as valid responses. All execution proof is pending root verification.
