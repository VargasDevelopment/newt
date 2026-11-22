# Source and licensing

All new packet source is original Newt work under the repository MIT LICENSE
SPDX tags identify source files. No vendor
implementation, third-party crate, acquired tool or SDK file is copied or changed.
Preserve repository LICENSE and vendor notices on integration/distribution.

Read-only policy inputs consulted:
- `include/newt/aligned_storage.h`: four-byte C3 malloc alignment, explicit
  aligned payload calculation and memcpy-stored raw allocation pointer.
- `src/xml_budget.cpp`: libc malloc/realloc/free policy, checked overhead and
  failed-realloc ownership. Its accounting is specific to XML and is not reused.
- `firmware/cover_native_heap.h`: pinned IDF 5.5.2/light-poisoning assumptions,
  non-aborting heap, capability-specific Cover allocator, vendor overhead.
  This proposal preserves general libc selection instead of borrowing Cover's
  separate capability/reserve/accounting policy.
- `.build/rust/native-io-preparation/{README.md,recipe.py}` and
  `.build/rust/native-io-target-preparation/run.py`: existing Run integration,
  actual C3 compiler/header/config closure and separation of object/final links.
- `.build/rust/native-io-target-02/evidence.json`: prior native-I/O object-only
  evidence, not evidence for this packet. It records the SDK flags/config and
  compiler fingerprints; Root must verify current inputs under its existing
  source/tool mechanism, never bless a changed SDK with a new expected digest.
- `rust/Cargo.toml`: normal pinned release profile. No profile exception added.

Approved existing SDK input base:
`.toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3/`.
Headers consulted: `include/heap/include/{esp_heap_caps.h,multi_heap.h}`;
`dio_qspi/include/sdkconfig.h`; `flags/{c_flags,includes}`. esp_heap_caps.h
specifies libc-equivalent allocation/reallocation and free ownership, native
aligned allocation, and MALLOC_CAP_DEFAULT. multi_heap.h specifies the existing
recursive lock association (the SDK must supply it). Actual allocator code is
in the existing SDK archives, not these declarations; final resolution and task
behavior require their own evidence. Config currently selects C3, unicore and
light poisoning without abort-on-failure. No handwritten vendor ABI constants
or FreeRTOS implementation are needed: this bridge calls libc, which already
owns that integration. Real sdkconfig is included and ABI widths asserted.

Existing C3 compiler/newlib header base:
`.toolchain/platformio/packages/toolchain-riscv32-esp/` (recorded GCC 14.2.0 in
native-I/O provenance). Standard allocation signatures come from its actual
stdlib.h, not handwritten Rust declarations of vendor heap internals. Rust
crosses only our four C functions. Root should preserve actual dependency-file
headers, compiler/config hashes and final SDK archive provenance with its Run
output. The frozen SDK input reference recorded by native I/O is
`cd6f5b831c8fad899723b6d406586f00c575f021`; its C++ policy source reference is
`bc9ffda52b85f277e46c63718ac88d62fba5d368`. These references do not certify
current local SDK archive bytes or this unbuilt proposal.

## Canonical materialization

Canonical mapping: lib.rs -> src/lib.rs; native_alloc.{c,h} -> native/;
host_test.rs and host_checks.cpp -> host/; probe.rs -> probe/lib.rs. All six bodies are
unchanged from the reviewed target-allocator-preparation packet. The existing
target-allocator-focus-01/evidence.json is earlier packet evidence; no
canonical Cargo build or final link has been performed by this preparation.
The recipe now selects the actual workspace package for host/RV32 release
builds and canonical native/test paths; existing SDK inputs remain unchanged.
