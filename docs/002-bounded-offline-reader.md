# Decision 002: one real offline book before an importer

2026-10-01. Implemented on `spike/offline-reader`; the first hardware baseline stays at `spike/freeink-x3`, commit `44c454df42d1ddaba566d18b3eaa1e88475ca45b`. Its exact 417,072-byte image is archived in `.build/baseline-44c454d/firmware.bin` with SHA-256 `f7f8c8191499f3ac85e250c751f51aaaf8464eb626653b46159ff5fe5f27f337`. Start the physical bring-up with that baseline, not this reader experiment.

Use the unchanged pinned official FreeInkBook engine with `FREEINK_BOOK_SMALL=1`. Prove actual ZIP/OPF/XHTML opening, text layout, chapter navigation and character-anchor resume with one embedded original CC0 EPUB. The sample is 6,931 bytes, two chapters and 14 host-rendered pages. It uses stored ZIP entries so decompression's extra 43 KB scratch cost is not required. The generated content identity follows the fixture SHA-256; changing the book cannot silently inherit another book's progress.

The adapter accepts a source of at most 32 KiB, at most 16 ZIP entries, only stored entries, and no resource larger than 8 KiB. Its manifest supports XHTML/NCX only; CSS, images and external fonts remain unsupported. These limits reduce this proof's work, but do not establish hostile EPUB safety. **The firmware exposes only its immutable embedded fixture**, with no SD/library importer. Ordinary compressed EPUBs and multilingual typography remain required daily-reader work.

The platform's original bitmap font supplies matching layout metrics and render glyphs. Unsupported codepoints display `?`. Page callbacks are consumed synchronously; no borrowed text pointer survives the callback. Full chapter reflow gives a truthful page count. The previous successful anchor stays valid after a failed turn. Any parse/layout failure stays unavailable until explicit close/reopen, with activity paused; drawing cannot silently restart a stopped reader.

## Memory evidence

A checked lazy allocation holds three arenas (8,192 / 49,152 / 12,288 bytes), FreeInkBook objects and the progress tracker. The clean experimental image is **628,848 bytes**; linked IDF RAM is 129,224 bytes, 280 bytes above the baseline. The **C3 allocation is 69,856 bytes**, from the linked `SampleReader::allocationBytes()` disassembly; host allocation is 69,976 bytes. It is released on Home and sleep. The portrait/display buffers are the existing two 52,272-byte frames; no page-frame cache or third refresh shadow is introduced.

Host arena peaks are 694 / 48,328 / 2,624 bytes. Expat also allocates through libc: instrumented requested-payload peak is 10,024 bytes, with zero retained parser allocations after successful operations and tested errors. This measurement excludes allocator overhead, fragmentation, task stacks and firmware-only allocations. The small SDK profile does not turn Expat into an arena allocator or guarantee a hostile document budget. The full-profile CSS builder is present even when embedded styles are disabled; the explicitly sized layout arena is not optional.

Before opening, the MCU checks the largest free heap block against the workspace and total free heap against workspace plus 24 KiB headroom. That is a graceful refusal threshold, not proof that a physical X3 fits. Capture real free/largest heap before/after opening and after repeated navigation/exit during hardware testing. TTF rasterization also allocates temporary heap in the pinned SDK; do not import a preferred font until its measured cost fits.

## A small durable core

Newt owns a separate `newt_reader` NVS namespace with two alternating 52-byte records. Versioned little-endian encoding, CRC32 and modular sequence ordering select valid progress. Save writes only the inactive slot and acknowledges only exact read-back and decode; failed or torn writes preserve the older valid slot. Equal differing or half-range ambiguous sequences refuse overwrites. Native NVS errors remain distinct from confirmed missing records/namespaces; any read error blocks a save. Startup read failure or ambiguity keeps the EPUB unavailable until an explicit reopen successfully loads prior state, preventing a fresh session from overwriting unknown progress. Preferences' size query was unsuitable because it maps all errors to zero length; a small native NVS adapter preserves the actual status. Identical durable state, ignoring sequence, causes no NVS write. Checkpoints run at most every 30 seconds while open, plus exit/sleep; power loss can lose the most recent checkpoint interval.

A monotonic active-time tracker counts only reading, stops after 300 seconds without navigation, qualifies a session at 60 seconds and counts forward pages only after two seconds dwell. Menus, sleep and unavailable screens earn no time. It handles ordinary millis wrap, rejects unmeasurably large intervals and saturates counters. These are deliberately small Newt semantics, not an import or compatibility promise for CrossInk's statistics. No calendar/timezone streaks or historical stats UI exist yet. Saved locators use chapter and character anchor rather than a layout-dependent page number.

NVS orchestration is host-tested with the actual error-bearing native NVS adapter and a fake store, including unavailable storage and failed length/data reads, read-back corruption, torn writes, ambiguity and 100 unchanged checkpoints. The native adapter also runs against a host NVS fault shim, testing missing versus error, malformed size, bounded reads and set/commit failure. This does not prove physical flash atomicity, partition capacity, endurance or persistence across device power loss. CrossInk settings, SD and credentials are never opened. The original baseline performs no reading-progress NVS writes; installing this separate experiment would permit its own namespace writes after separate hardware authorization.

## Reproduce and next gate

```sh
python3 tools/check.py
python3 tools/preview.py --screen epub --actions next,sleep,wake
python3 tools/firmware-build.py --clean
```

The host gate compiles real pinned Book/ZIP/XML/layout source under ASan/UBSan and includes allocator-failure injection. `artifacts/epub.png`, `epub-next.png`, `epub-resumed.png` and `epub-reopened.png` are exact shared firmware-frame previews. Successful resume previews compare byte-for-byte. Compilation, host pixels/behavior, and physical behavior remain separate evidence; see `artifacts/reader-verification.json`.

No compiler, SDK, package or font acquisition was required. The official source pin and toolchain receipt remain unchanged. No hardware, SD, credential, network service, Registry or control plane was accessed. After baseline recovery/display/button/sleep acceptance, separately review the reader binary and its Newt-only NVS behavior before writing it. The first reader hardware gate is fixture open/14-page navigation/chapter bounds/menu resume/deep-sleep resume/power-cycle progress and heap stability. Real books, font quality, importing and daily use come afterward.

The reviewed reader source checkpoint is `2bb99680fe9b4028054efc2776fffb7ca8a8fc58`. Its exact compiled artifacts are archived at `.build/offline-reader-2bb9968/`; receipt hashes remain valid when a later baseline build replaces `.pio/build/newt_x3`.
