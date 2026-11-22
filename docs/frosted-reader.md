# Frosted reader: first geometry and chrome slice

`spike/frosted-reader` implements the selected round5 reading layout through the existing shared C++ renderer and official FreeInkBook. It is an honest **monochrome fallback with provisional bitmap typography**, not a pixel match for the Lexend/AA/four-level reference. [Verification receipt](../artifacts/frosted-reader-verification.json) separates source, exact binary, host previews and untested hardware.

Reader settings support landscape CCW (792 × 528, the personal reading direction) or portrait (528 × 792), plus the selected three-pixel slim bar, on by default and configurable. They do not change the portrait system shell or its typography. Both interpretations use the same 52,272-byte packed frame. Bounds come from the selected reference: 5-pixel margins, body bottom 48 pixels above the bottom, bar at height−42, footer at height−25. These design pixels are not an interpretation/migration of the owner's original unitless margin/word-spacing report.

The reader requests SDK justification using the original 24-pixel bitmap metrics. The real original EPUB now has **5 + 5 landscape pages**; portrait is independently reflowed. Persistent UI title and button rails are removed. A book's own chapter heading remains book content. The footer contains known/unknown battery, actual current-chapter page x/total and whole-book percentage. Toggle the bar without changing prose, footer or pagination.

Whole-book percentage means **the displayed page's first extracted character divided by total extracted book characters**. Each of at most 16 spines is laid out once on open into the existing arenas to cache its character length. Later paint/turn operations reflow only the current chapter. Opening now lays out all bounded chapters before showing the first page; C3 opening latency is unmeasured and belongs in the next hardware observations. First page is 0%; the book's last page reads 100% (#105), so the end of a book is never shown as 99%. Every other page is position, not a claim that all visible text was read. Cache metadata is bounded; immutable source is required until close/reopen. Failed operations preserve the last published position and keep the app unavailable until explicit reopen.

Host tests cover both body bounds and frame guards, optional-bar isolation, known/unknown/extreme footer values, unchanged portrait system pixels, semantic anchor resume after reflow and sleep/reopen, one-time indexing, failure cleanup and existing package/NVS behavior. C3 conversion uses pinned FreeInk's native LandscapeCounterClockwise transform for reading and its original portrait transform for system screens. A geometry transition requests a full refresh; the general ten-paint policy otherwise remains unchanged.

```sh
python3 tools/check.py
python3 tools/preview.py --screen epub --telemetry sample --reader-orientation landscape-ccw --reader-progress-bar off
python3 tools/preview.py --screen epub --telemetry sample --reader-progress-bar on
python3 tools/preview.py --screen epub --reader-orientation portrait --actions next,sleep,wake
python3 tools/firmware-build.py --clean
```

`sample` battery data is invented preview telemetry; default is unknown. Firmware uses the existing checked BatteryMonitor readings. CLI options are applied before opening regardless of ordering. Firmware currently compiles the default landscape/slim-bar choice; there is no persisted settings screen. Neither command accesses hardware.

The image is **638,608 bytes**. PIO reports 71,260 bytes linked data RAM; IDF reports 131,344 bytes DRAM including 60,086 bytes executable text and 189,952 bytes not occupied by linked sections. The lazy reader allocation is **69,968 bytes on C3**, 112 bytes above the previous loader candidate; host allocation is 70,088 bytes. Both existing full frames remain 52,272 bytes each. Arena peaks remain 694 / 48,328 / 2,624 bytes, and the host Expat requested-payload peak remains 10,024 bytes with no retained parser allocations. Linked remainder and host allocation measurements do not establish actual free heap, fragmentation, runtime SDK costs or panel latency.

No Lexend/font assets, AA, native gray uploads, blur, reading menu/statistics screen, persistent preferences, configurable physical buttons or five-reading-page cadence were added. No hardware/SD/credentials/new dependencies/publication were used. The selected direction and those requirements remain in [Decision 003](003-frosted-implementation.md); bring-up still starts with the archived smaller baseline under [BRINGUP](BRINGUP.md).
