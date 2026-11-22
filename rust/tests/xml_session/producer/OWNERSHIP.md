# Direct host unsafe-boundary analysis

`Expat` exclusively owns a NonNull parser handle between create and close/drop.
No core unsafe code was added. New index/configure calls accept only this live
handle; parser configuration and index inspection never receive Rust references.
Index is normalized to zero when Expat reports a negative value, as in C++.

Each feed creates a Rust stack Context holding the exclusive handler borrow and
a nested C stack callbacks record pointing to it. Expat userdata points to the C
record only during synchronous XML_Parse. All element/text callbacks use the
parser-as-handler argument to fetch this record, then enter the Rust trampolines.
The trampolines access the exclusive Rust Context sequentially. No callback can
retain an attribute/text view through the safe Handler API; the view's lifetime
is confined to the callback. C clears userdata, element handlers and text handler
before every normal XML_Parse return (success, parse error or aborted DTD).
Policy cancellation runs before/after feed, never while a callback owns Context.

The doctype callback retains no context and receives the live parser itself;
XML_StopParser uses Expat's supported synchronous callback operation. The external
callback receives the parser according to Expat's fixed external callback ABI,
uses no borrowed state and returns error. Those static C function pointers may
remain installed between feeds without retaining Rust stack storage. Close frees
only the owned parser and is idempotent. create_producer configures only after
successful create. Failed create leaves no parser or callback record.

Callback panics abort rather than unwind across extern C. Reentrancy is excluded
by exclusive session/backend/handler borrows and by not exposing parser handles.
The backend has no shared global state; scoped failing allocator callbacks return
null only for one explicit XML_ParserCreate_MM call. They acquire no heap storage.
The parser and C/Rust stack Contexts are never moved during synchronous XML_Parse.
Concurrent sessions have separate parser and Context values. None of this proves
target ABI compatibility or target execution, and sanitizer evidence is pending.
