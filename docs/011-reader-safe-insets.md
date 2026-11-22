# Reader spacing after the first hardware pass

October 1, 2026. The owner accepted the guarded reader and supplied photographs of its rendered page and CrossInk's corresponding layout. The Newt photo shows prose and the right percentage near the physical glass/shadow, with chapter and page visually crowded together. This slice improves only reading geometry/chrome; installed firmware remains `a4d8812`.

Body left/right/top inset increases from 5 to **16 pixels** in both orientations. Footer left/right and painted bottom clearance are also 16 pixels. Its row moves up five pixels within the existing 48-pixel reserved area. Battery stays left; **Chapter 1** is independently centered; **3/5** and **23%** form the right group with a fixed 12-pixel gap. The chosen optional slim progress bar remains in its existing isolated three-pixel strip. Bar Off adds no unconditional divider.

Ordinary footer text remains scale 2. Extreme chapter/page values use scale 1 when needed to preserve 12-pixel clearances, retaining every value including `UNSAVED`. No text truncation, new allocation/framebuffer, storage policy, SDK modification or font change is introduced. The fixed glyph-proof fixture explicitly retains its original reference inset; its independently pinned source/pixel oracle is unchanged. Actual EPUB pages and all 95 glyphs separately enforce the product's new safe bounds.

The sample now reflows to five pages per chapter in landscape (formerly four); its first portrait chapter remains four pages. Saved semantic character anchors select the containing passage under reflow. Page numbers are recomputed, not migrated as fixed positions.

## Exact candidate and gates

Compiled source **`808e9084ea449f0c6db2808293d26540a995a281`**; archive `.build/reader-safe-insets-808e908/firmware.bin`: **658,848 bytes**, SHA-256 `9fff133a13e9072fe1868a1dc1af4c669e3eba4064de53d318c7bfd3a17f08d1`. This adds 128 bytes over the installed guarded image. Proposed app scope stays `[0x780000, 0x821000)`, 161 sectors; payload ends `0x820da0` with 608 bytes of erased padding. **No new flash is approved or executed.**

The complete strict-warning/ASan/UBSan host gate passes, including all reader/UI/store/package/guard tests and the exact canonical font oracle. New tests cover both orientation safe bounds, actual EPUB/long-word glyph bearings, all 95 glyphs crossing boundaries, maximum uint32 labels, 100%, unknown/failure/UNSAVED states, independent footer fields and optional-bar isolation. Clean C3 compilation and mandatory linked NVS preservation checks pass. Independent bounded source/fixture reviews report no actionable findings.

Static RAM remains 71,396 bytes; linked DRAM 131,488 bytes; reader workspace 69,968 bytes on C3. These are build evidence, not new hardware measurements. All eight earlier binary archives remain unchanged. Reader Home/app/device/sleep system pixels are unchanged.

```sh
python3 tools/check.py
python3 tools/firmware-build.py --clean
python3 tools/preview.py --screen epub --telemetry sample --actions next,next --output artifacts/safe-insets-landscape
python3 tools/preview.py --screen epub --reader-orientation portrait --reader-progress-bar off --telemetry sample --actions next,next --output artifacts/safe-insets-portrait
```

The previews use labeled sample telemetry, not battery measurements. [Landscape](../artifacts/safe-insets-landscape/preview.png) · [Portrait, bar off](../artifacts/safe-insets-portrait/preview.png) · [Exact evidence](../artifacts/safe-insets-verification.json).

After a separately approved future image installation, compare physical left/right/bottom comfort, page/percentage gap and centered chapter with these exact source previews. Check both orientations, Bar On/Off, reflow to the saved passage, menu/exit-reopen and sleep/wake. The photographs supplied here justify the change but do not prove the uninstalled candidate's optical quality.
