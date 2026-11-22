> Retired 2026-10-26: Stage is the only Home. This document preserves the old
> design and commands as history; the Frosted Home preview/renderer is removed.
> Shared atlas, system chrome and provenance remain. See [Stage Home](STAGE_HOME.md).

# Frosted Home B implementation

**Current navigation (2026-10-02):** Morning Page has been removed with the owner's approval. Home retains the current-book card and three evenly spaced routes: Books, Apps, and Settings (named Your X3 until NEWT-32). See [the navigation update](../assets/frosted-home/navigation.md) for current layout and verification. First Home can now show validated saved chapter/whole-book progress before opening the reader; missing or invalid storage remains explicit. The original source package and verification below describe the historical four-route design.

This original Home implemented the owner's approved square-card design, preserving its exact geometry, background and Inter typography. `assets/frosted-home/source/README.md` is the authoritative handoff. At the original checkpoint, the host fixture used fictional Secret Garden/10:24/84%/32% data and matched the original canonical PNG pixel-for-pixel. Firmware never calls that fixture factory.

At the original checkpoint, runtime Home showed the embedded **The Small Lantern**, an unset clock (`--:--`), measured battery or `--%`, and no invented cover. Whole-book progress came from the open reader and remained unknown after reset until measured again. Morning Page and Apps both opened the bundled Pocket Pause app; the supporting copy described a local note and offline reading. The current navigation removes that placeholder and copy; validated durable progress now supplies first-Home metrics before reader opening. Books continues the embedded sample, and Your X3 opened device information (since NEWT-32 the route is named Settings and opens the Settings page). No book browser, daily overview, weather, or calendar feature is implied.

The Settings route has no annotation by default: its title names the route (NEWT-32). The route was named Your X3 with the note **(Device info)** while it opened device details, then briefly **(Settings)**. A note still comes only from its own trusted `HomeData.deviceDescription` field; null and the approved fixture draw none. Cover choice does not determine this label.

Physical Home hints and actions are explicit: facing the screen, bottom-left outer button (hardware Back) means **Read** at Home, bottom-left inner OK means **Select**, bottom-right inner Previous means **Up**, bottom-right outer Next means **Down**. The side Previous/Next buttons also move focus. Power retains Sleep. Outside Home, the outer-left button means normal Back. Firmware maps that context to semantic `Action::Read`; ordinary semantic Back remains a no-op at Home. No GPIO mapping or power sequence changes. Configurable bindings and hints that follow them remain an accepted requirement, not a newly implemented settings system.

UC8279 Home uses the SDK's typed Direct four-gray mode and built-in XTH4 bank. It uploads complete selector LSB then MSB before one blocking activation. Selectors are black00/dark10/light01/white11, in LSB/MSB order. Portrait rotation preserves selectors; it does not apply PBM inversion. Unsupported mode or failed begin uses the same Home rendered in monochrome. Other screens retain one-bit rendering. The SDK owns clean transitions back to B/W, incomplete-pass cancellation and panel sleep. No SDK, NVS policy, boot selection, SD or network changes are included.

The existing 52,272-byte logical and 52,272-byte native frames are reused sequentially; there is no third framebuffer, PNG decoder, dynamic font cache or full 2-bit image in RAM. Resources/metrics are constexpr flash data. The haze background never recomputes or animates, and unchanged minute battery polling does not refresh Home. Button focus redraws the complete gray Home; partial-gray refresh, latency, ghosting and energy savings are not claimed.

`lastPaintWasGray` records whether the last actual paint left selector data in
the working frames. The capture contract from `firmware/main.cpp` is retained
word for word here:

DEV capture must reject selector frames unless its wire format supports
four gray levels; a selector plane must never be labelled monochrome PBM.

The standalone Home checkpoint added no bridge; this integration includes the
separately reviewed development-only bridge. Keep this contract here and an
accurate pointer at the capture owner when extending its wire format.

## Local commands

```
python3 tools/frosted-preview.py --fixture --out artifacts/frosted-approved-fixture.png
python3 tools/frosted-preview.py --out artifacts/frosted-runtime-unknown.png
python3 tools/frosted-preview.py --selected 2 --battery 100
python3 tests/frosted_home_test.py
python3 tools/check.py
python3 tools/firmware-build.py --clean
```

Normal resource validation uses Python's standard library. Optional `python3 tools/make-frosted-home.py --import` rebuilds raster resources with existing macOS Swift/CoreText against the pinned handoff; no tools are installed. [Resource provenance](../assets/frosted-home/PROVENANCE.md) explains licenses, exact-label masks and the bounded generic glyph atlas. Variable strings use quantized eight-phase Inter coverage, not a promise of exact arbitrary CoreText raster matching.

## Software evidence

The original full-review checkpoint is `6304f8e64ecdf0d3e3ae89359f88ed1ece2cf7fe`; the final compiled checkpoint `7cea7ec679d3fd0b4c4be7ae1d5df8e2addac7d2` adds only the truthful Device info label and its regression/golden tests. The complete host gate passes with ASan/UBSan, including exact approved pixels, truthful default metadata, four distinct focus rows, null/extreme metadata bounds, every-pixel selector rotation and typed Direct upload/fallback order. One fresh adversarial round and one maintainability round completed clean; the adversarial reviewer independently reran resource validation and sanitized C++ tests. Both original reviewers also completed a clean narrow follow-up on the final label change. The focused and complete host gates passed again on that final source.

The final ESP32-C3 build passes with the unchanged official SDK/toolchain and mandatory linked NVS reachability gate. Actual binary size is **1,206,656 bytes**, SHA-256 `bedb5b8346cf9f53182ccd5ade64c961333b665abdbd0d8383088bd97763a517`; linked flash is 1,172,480 / 1,310,720 bytes and static RAM is 71,420 / 327,680 bytes. These are compile figures, not physical heap or refresh measurements. The two existing frames remain 52,272 bytes each. The final local binary/ELF/map archive is `.build/frosted-home-7cea7ec/`; `.build/frosted-home-6304f8e/` and `.build/frosted-home-0ed1c0f/` remain historical. [Receipt](../artifacts/frosted-home-verification.json) records exact inputs, hashes, commands and limits; [approved fixture](../artifacts/frosted-approved-fixture.png) and [truthful runtime](../artifacts/frosted-runtime-unknown.png) are separate artifacts.

## Native acceptance still required

No firmware from this branch has been flashed. Source/host protocol checks do not establish physical gray separation. Review exact compiled size/hash and touched sectors before any later hardware proposal; the prior 161-sector restoration slice does not authorize or fully cover a larger Home image.

On the cracked development X3, test cold boot, all three current focus rows, each route and Home Read, Home → reader → Home, power sleep/wake and held-power wake. Inspect four separable tones, gray85 one-pixel borders, Inter baselines/legibility, static background restoration and clean B/W transitions. Capture measured refresh behavior and stop on BUSY failure, stale image or a reset loop. Keep the original/guarded-reader binaries and backups intact. The original first-boot character/source remains preserved; its approved five-activation About Easter egg is a later slice, not claimed as implemented here.
