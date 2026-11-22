# Resource ownership invariants

These invariants govern the Rust firmware, shared core and Salamander device
layer.

| Resource | Owner | Representation | Invariant |
| --- | --- | --- | --- |
| Immutable book bytes and source handle | `native-storage/src/source_file.rs`, `native-storage/src/sd_source.rs`, `firmware/reader_selected_source.rs`, `firmware/reader_image_source_loan.rs`, `core/src/zip_window/source.rs` | Owned source adapter and borrowed immutable reader session, or stable numeric offsets into an owner-held source | No read/view outlives the source; source revision changes invalidate derived receipts and caches. |
| Reader arenas | `core/src/zip_window/arena.rs`, `salamander/sdk-heap/src/aligned.rs`, `firmware/reader_book_memory.rs`, `firmware/reader_image_memory.rs`, `phased-reader/src/resources.rs` | Caller-owned aligned blocks; arena references bounded to a session | Fallible admission precedes allocation; fixed capacity is not replaced with unbounded collections. |
| Display/parser staging loan | `salamander/board/lib.rs`, `phased-reader/src/phase_storage.rs`, `firmware/reader_book_resources.rs` | Exclusive scoped lease of mutable staging bytes | Parsing and drawing cannot hold the same mutable bytes together; all XML state is destroyed before the display gets its buffer back. |
| Published logical frame | `salamander/board/app_frame.rs`, `core/src/frame.rs`, `firmware/app_runtime/display.rs`, `firmware/app_runtime/publication.rs` | Application-owned frame; immutable published generation and separate staging borrow | A failed page build never publishes partial pixels or advances the displayed generation. Rotation/selector transforms use distinct buffers. |
| Callback text and section labels | `core/src/chapter_layout/types.rs`, `phased-sink/src/lib.rs`, `reader-navigation/src/lib.rs` | Callback-scoped borrowed slices, copied only into existing bounded owner storage | Sink text is consumed during the callback and cannot escape into a longer-lived UI state. |
| Durable store/backend | `core/src/durable.rs`, `native-storage/src/reading_state.rs`, `firmware/nvs/nvs.rs`, `salamander/nvs_guard/lib.rs` | Exclusive backend borrow, explicit commit capability, local fixed scratch and scope-closed transaction | One backend transaction at a time; only barrier acknowledgement plus exact readback publishes success. |
| In-flight persistence | `core/src/progress_store.rs`, `core/src/external_progress_store.rs`, `core/src/sleep_intent.rs`, `firmware/reader_checkpoint.rs`, `firmware/reader_pending_recovery.rs` | Explicit pending/confirmed result and protected-peer identity | A lost response is not definitely unsaved; retries cannot overwrite the last acknowledged peer. |
| Live preference | `device-ui/src/home_clock.rs`, `firmware/home_startup.rs`, `firmware/reader_preferences.rs`, `core/src/preferences_store.rs` | Separate intended and published values with durable restore | Cold restore discards RAM. Explicit fallback must replace an older opt-in; failure retains the previous runtime choice. |
| Retained sleep state | `salamander/board/retained.rs`, `firmware/retained_runtime.rs`, `firmware/retained_runtime/record.rs`, `firmware/app_power.rs`, `firmware/reader_sleep.rs` | Explicit validated wire/retained representation behind Salamander's board adapter | RTC is a validated warm fallback, never the sole persistent preference authority. Native Rust layout is not a retained schema. |
| Input/background work | `salamander/input/lib.rs`, `salamander/input-owner/`, `core/src/reader_scheduler.rs`, `firmware/app_runtime/run_loop.rs`, `firmware/reader_producer.rs` | Owned state machines receiving input/work events; scoped admission capability | Callbacks cannot reenter or mutate a live reader; once a save enters durable work input cannot cancel it. |
| Reading versus update memory | `firmware/wifi_memory.rs`, `tools/wifi-sections.py`, `firmware/update_mode.rs`, `firmware/update_https.rs` | Separate boot-mode ownership, with an audited linker adapter | Reading never starts Wi-Fi. Reclaimed static memory cannot simultaneously belong to radio state and the reader heap; `tools/verify-wifi-link.py` verifies the linked ELF. |
| Covers and optional caches | `firmware/cover_memory.rs`, `firmware/cover_job.rs`, `firmware/library_covers.rs`, `firmware/reader_cover_idle.rs`, `firmware/reader_cover_producer.rs` | Scoped owned jobs and caller-supplied bounded workspaces | An unused feature retains no reader memory by default; errors unwind loans and cannot poison authoritative progress. |

Do not solve self-referential arena lifetimes by automatically introducing
`Rc`, `Arc`, `RefCell`, leaked allocations or raw pointers. Prefer stable
offsets/indices for nodes inside a caller-owned arena and short-lived borrows
when interpreting them. An owning container plus borrowed views can replace a
self-reference when views need not survive a method call. Where a source
requires address stability across callbacks, record that exact contract and
review the proposed pinned owner before implementation.

Resource budgets live in `code-health-baseline.json` and are measured against
clean firmware builds (`python3 tools/firmware-build.py --clean --health-report`,
see [docs/CODE_HEALTH.md](../docs/CODE_HEALTH.md)). Host Rust type sizes, or
`target-probe`'s synthetic stack storage, do not establish X3 resource
consumption or justify changing limits.
