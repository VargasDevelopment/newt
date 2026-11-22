# Provenance and licensing

`original/include/newt/book_semantic_index.h` and
`original/src/book_semantic_index.cpp` were copied byte-for-byte by read-only
`git show bc9ffda52b85f277e46c63718ac88d62fba5d368:<path>` from Newt.
`original/include/newt/package_loader.h`, `original/include/newt/epub_admission.h`
and `original/LICENSE` were copied the same way at the same revision. Newt's
MIT terms are retained in the root `LICENSE`.

The original source attributes extraction parity to FreeInk ChapterLayout.cpp,
MIT licensed, commit cd6f5b831c8fad899723b6d406586f00c575f021. Newt's pinned
vendor/freeink-sdk gitlink points to that revision. `licenses/FreeInk-LICENSE`
and `licenses/FreeInk-NOTICE` were copied byte-for-byte with read-only git show
from that submodule revision. Preserve these files on adoption/distribution.

The Rust counter/index is a translation of those Newt files under their MIT
terms and the retained upstream extraction attribution. Packet adapters, tests,
probe bodies and documents use the same Newt MIT terms. No upstream code was
fetched, installed or modified. Parent removed the two proposed dependency-header substitutes. The native
recipe must use actual pinned Newt and SDK headers; original behavior comes
from the unchanged pinned source. Source copying is provenance evidence,
not a parity or correctness result.
