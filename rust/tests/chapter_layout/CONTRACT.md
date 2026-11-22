The shared chapter modules translate pinned FreeInk SDK `cd6f5b8` chapter layout,
plain-text page production, image prescan and producer-bounded XML orchestration.
The C++ source and all five authenticated producer patch files are preserved.
Native tests reuse the exact SDK closure already frozen for PageRenderer; no
reader logic is supplied by a C++ adapter behind Rust.

One `Backend: Font + Sink` owner lends metrics and consumes each completed page
synchronously. Offset-backed `PageView` borrows existing text and descriptors;
rendering performs preflight checks before frame publication. `XmlStream` accepts
a handler per feed, and the compatibility `XmlSaxSession` delegates to that same
parser/policy. Prescan and main parsing borrow producer policy sequentially.
Mutable font/source/storage loans must be disjoint and live for their documented
calls. These are caller-owned typed capacities, not equivalent native arena bytes.

Independent complete traces cover small layouts, BOM/plain text, operational
failures and XML boundaries. Differential traces extend styles, Arabic/bidi,
ligatures, focus, CJK compression, locators and image-cache policy. The exact
trace and callback order matter. A malformed response, compiler error, panic,
sanitizer diagnostic or surviving mutation never establishes detection.

Expat and libunibreak remain explicit native host backends. The allocation-free
RV32 probes exercise borrowed Rust bodies; they do not supply target XML/Unicode
backends, firmware startup or resource acceptance. Original MIT, Unicode and
native library notices remain with the source. The Unicode license text is a
separately recorded official notice; original pinned data headers are unchanged.
