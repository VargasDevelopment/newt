# Source contract and ownership

Authority: frozen SDK XmlSax.cpp/h, not the working SDK. `PROVENANCE.json` gives
full-source hashes. The upstream file comments and licensing remain intact.

| Operation | Baseline observable ordering |
| --- | --- |
| open | Close old parser; assign handler; reset progress/finished; reader.open; bind parse buffer; reset requested available filter even if buffer missing; resource OOM; parser create OOM; publish open |
| feed | Write previous finished into atEnd; reject closed/finished; pull filtered/raw Rust input; I/O error before parse; synchronous parser feed with final iff n=0; parse error before progress; add filtered n; n=0 sets finished and atEnd |
| close | Free live backend once; clear buffer/filter/handler and finished; retain progress |
| one shot | Open before inspecting handler stop; check stop only between chunks; propagate failure; close on all returns |

Failed parse can have emitted callbacks and advanced ZIP/filter state, while
bytesConsumed stays unchanged. Errors do not automatically close the parser.
Repeated feeds after errors are backend-dependent exactly as upstream; a generic
backend is not required to recover from parse errors. Successful final feed keeps
is_open true until close. Stop never aborts a running feed or changes its result.

The Rust Reader retains source/entry/optional decoder borrows, not native reader
pointers. Resources hold an exact 2048-byte parse array and optional initialized
EntityFilter. InflateScratch's State/dictionary/input are exclusively caller owned.
Backend storage is caller owned; native host Expat additionally owns its parser
heap. Core never constructs decoder/filter/parse buffers on its own stack.

Typed resources are a different domain from C++ byte-arena capacity, alignment,
allocation failures and mark/release. None means unavailable at the corresponding
admission point; it does not model exact arena high water or failed allocation
size. Reader-open failure precedes parse-buffer/filter availability. One shot
releases borrows on return instead of mutating an arena mark. No shared arena
aliasing, self-referential storage, global buffers or heap copies bridge lifetimes.

Conservative lifetime limitation: each open argument is borrowed for the entire
session lifetime, including after close. Reopen with a distinct handler/resource
set is supported and tested in source; reclaim/reuse a previous set by dropping
and recreating the session. This forbids some C++ call patterns, rather than
claiming that close permits Rust borrow reuse. Source/entry remain live through
the session. Handler/backend cannot access each other through retained references.

Host FFI is confined to host/expat.rs plus expat_bridge.c. A feed installs a stack
Context holding the exclusive handler, invokes Expat synchronously and removes
all callback pointers/user data before returning. Names/attributes/text are Expat
borrowed callback views; safe trait signatures prevent retaining them. Parser
is uniquely owned, never shared or reentrant. NativeAttributes can only be
constructed by this adapter. Callback panic aborts across extern C (no unwinding
through C); production handler panic behavior is not an OOM strategy. Expat's
allocator is outside the allocation-free core claim.

# Strict host wire

UTF-8 ASCII requests, one record per LF, mandatory final LF, no CR, blank record,
extra whitespace/token, uppercase hex or incomplete record. Malformed transport
exits 2. A request is:

`X hex max fail filter stop actions`

hex is lowercase even-length bytes or `-` for empty (max decoded 1 MiB); max is a
positive read cap <=1 MiB; fail is zero-based read call index or -1; filter is
0/1; stop is 0 (never), 1 (initial), 2 (first text callback). actions is `p` for
one shot, or a comma list of `f` (feed) and `c` (close). Stepped mode opens once
implicitly. Reopen is in direct lifecycle tests, not this protocol.

Output records: `R offset requested returned`, `S hex-name [hex-name hex-value]*`,
`E hex-name`, `T hex-text`, `O status count open`, `F status count atEnd open`,
`C count`, `P status`; each request ends with exactly `END\n`. Booleans are 0/1.
Callbacks and source operations appear in execution order, before the operation's
result. Counts exclude unsuccessful feeds. Both transports reject partial LF
input before running any operation. For malformed later records, preceding valid
records may already have output; consumers must reject the nonzero exit.

The C++ oracle uses full original session/reader/filter plus original Expat. The
Rust host uses Rust session/reader/filter plus only original Expat. These are
separate executables. Independent expected records in host_vectors.py are checked
against each, rather than considering agreement between two ports sufficient.
