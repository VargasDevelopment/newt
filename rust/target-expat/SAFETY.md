# Native boundary contract

This authored leaf is `no_std`; its only Rust dependency is actual root
`newt-core`. Core is unmodified and remains safe. It links the byte-identical
existing C bridge and pinned FreeInk Expat, not a C++ reader or parser imitation.
There is no Rust allocator, `alloc`, `Vec`, `Box`, bindgen, new dependency,
retained handler, or self-referential object in production.

The manual declarations in `src/lib.rs` match `native/expat_bridge.c`:

| C | Rust |
| --- | --- |
| `void *` parser/context | `*mut core::ffi::c_void` |
| `const char *` bytes/name | `*const core::ffi::c_char` |
| `const char **` attributes | `*const *const c_char` |
| `int` length/final/result | `core::ffi::c_int` |
| `int64_t` byte index | `i64` |
| `start_fn`, `end_fn`, `text_fn` | matching `unsafe extern "C" fn` pointers |

Do not compile Expat with `XML_UNICODE` or `XML_UNICODE_WCHAR_T`: the pinned
configuration exposes byte `XML_Char`, not UTF-16. RV32 uses C calling convention
and 32-bit `int`. `XMLCALL` on the pinned supported ELF targets is the ordinary
C convention. Bridge casts `XML_Index` to `int64_t`, so the Rust side does not
guess the native `long` width or `XML_LARGE_SIZE` setting. Compile bridge and
Expat against the same pinned configuration/headers.

`Expat` owns one optional non-null parser. `create` closes any previous parser;
failed creation leaves none. No Clone, Send or Sync implementation is added.
`&mut self` plus private parser ownership prevent safe callers from reentering
or concurrently driving that parser. `close` takes the pointer before freeing;
repeated close and subsequent Drop are harmless. No parser allocation happens
on `default()` or while the backend is unused.

Each feed creates a stack `Context<'_>` holding the exclusive handler borrow.
Its pointer is cast to `void *`, not to a fake `'static` reference. The callback
casts it back only during that same call; this is lifetime erasure at the C
boundary, not a self-referential lifetime extension. The bridge creates its own
stack callback record, installs parser-as-handler and userdata, and calls
`XML_Parse` synchronously. The native start/end/text trampolines recover the
bridge's record via `XML_GetUserData`, then invoke Rust with the original
context. Every feed return, including XML error/abort, first clears userdata and
all three Rust-facing handlers. Producer doctype/external callbacks can remain
installed because they contain no Rust borrow and use only the parser.

Expat guarantees names/attribute values are terminated strings and its
attribute array is terminated, with complete name/value pairs. `NativeAttributes`
is private and exists only on the callback stack. Its returned slices are tied
to the current borrow, and Handler cannot retain that view through safe Rust.
Lookup uses the original base even after iteration. Text is a non-null native
buffer with a nonnegative `int` length; it is length-delimited, not a C string.
Input and all callback views remain valid until the synchronous native call
returns. A handler must not unwind through C; Rust's non-unwind extern C boundary
aborts on panic, and target integration must retain the existing panic=abort
policy. No C longjmp or asynchronous callback is supported.

The adapter deliberately matches the known-working host semantics:

* `XML_STATUS_ERROR` is false; native callbacks are not suppressed on Handler's
  `stop_parse`. Core checks that flag between one-shot feeds; stepped feeds ignore
  it. Same-chunk parse failure wins over a newly requested stop.
* Producer creation installs the bridge's non-resumable internal-subset abort
  (`XML_StopParser(..., XML_FALSE)`) and external-reference error handler. No
  replacement DTD policy or custom entity parsing is added.
* Byte index is exactly `XML_GetCurrentByteIndex`, sampled by Core before feed;
  no live parser gives -1. Oversized Rust input lengths fail before FFI.
* Raw `feed` failure retains the native parser until `close`/Drop, just like the
  host adapter; it does not destroy diagnostic byte-index state. Core one-shot
  parsing always closes after errors. A stepped session retains resources until
  explicit close or Drop, also exactly as Core specifies. Dropping the backend
  frees it even after raw feed failure. Do not infer immediate native free from
  a stepped ParseError alone.

Expat's existing `malloc/realloc/free` allocations are inside this explicitly
allowed native boundary. They are not allocation-free Core or Rust allocations.
The failing-creation switch invokes the existing per-call allocator suite, never
a process-global fault mode. It neither exercises nor proves every subsequent
native allocation-failure path. Device peak heap, allocation failure/fragmentation,
stack, static RAM, flash and RTC accounting remain pending parent validation and
owner memory acceptance. There is no new persistent cache or idle heap allocation.
