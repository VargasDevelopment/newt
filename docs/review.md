# Local quality outcome

Reviewed 2026-10-01 on `spike/freeink-x3` against local `main`. New isolated repository; no prior project changes were mixed into this branch. Parent commissioned firmware/toolchain, screen and workflow/test workers with nonoverlapping files; integrated into one shared renderer/state and one firmware target.

## Bounded adversarial review

Two fresh rounds, stopping clean before the seven-round cap.

- Round 1 used two independent reviewers: workflow/render/bring-up clean; firmware reviewer found two P1 initialization bugs against the actual pinned SDK.
- Fixed controller reset: `setDisplayX3()` now precedes the live probe. The probe changes `ACTIVE.displayController` without changing board identity; a later `setDisplayX3()` would discard UC8279 selection.
- Fixed sleep-to-boot shared bus: `releaseSdRail()` now runs before the live probe/display begin. Even an SD-less app must release GPIO13's retained hold and deselect an inserted card so it cannot clamp the display's shared SPI lines.
- Round 2 independently checked current files and exact SDK source and returned clean; no unresolved actionable findings or speculative exotica reported.

`close-the-class` was consulted for the paired initialization mistakes. The invariant is **one canonical startup order for both cold boot and deep-sleep reset: choose X3 geometry/profile → restore shared-bus rails → probe controller → initialize display**. It lives directly in the existing single setup path; there are no duplicate boot paths or later profile resets. A generic hardware abstraction or source-text matching test would add indirection without proving the electrical behavior. Therefore these are proportionate fixes with a documented ordering invariant, **not a claim of enforced hardware class closure**. Real SDK translation and the cold/wake × detected-controller × inserted-SD matrix remain firmware/hardware acceptance gates. Pure host tests cover the exact rotation and retained-state logic separately.

## Thermonuclear maintainability review

Fresh reviewer read the branch diff/current files and specialized skill; pass, no actionable structural findings. Authored renderer is 276 lines; direct typed state transitions, a packed canvas, and a hardware adapter own separate concerns. No Wasm/guest runtime/package/security framework introduced.

The custom input task earns its place: the pinned SDK's `beginAsync()` has no checked startup outcome, no public suspension lifecycle, no held-power wake gating or overflow diagnostic, and allocates unused touch queues. The small checked queue/task preserves these required behaviors while keeping one InputManager update owner.

## Verification and terminal state

`python3 tools/check.py` passes exact clean FreeInk pin, strict Clang host compilation with ASan/UBSan, shared action/page/sleep/resume tests, truthful telemetry, deterministic full bounded frames, actual rotation/polarity and RTC corruption/range tests, invalid CLI rejection, and independent PNG pixel roundtrip. Eight exact framebuffer previews were rendered and visually inspected. `git diff --check` passed.

**Actual firmware proof:** owner-approved local toolchain acquired, official C3 code translated and linked, valid 417,072 B app image produced. ELF/map and artifact hashes are recorded with both memory summaries in `artifacts/verification.json`. IDF reports 128,944 B RAM including 60,022 B of code; PlatformIO reports 68,924 B static data RAM. The 52,272 B portrait frame is already in BSS; a separate 52,272 B FreeInk frame and task/queue allocations occur dynamically. Runtime free/largest heap and all physical behavior remain unmeasured.

After the actual build, a fresh firmware reviewer checked the pinned SDK, linker map, queue/task ownership, retained state, display allocation and bounded USB flushing: clean. A dependency reviewer independently verified all 63 artifact hashes/sizes, all 51 Python lock entries, all loaded Core module files against the fork wheel, and the C3-only package shape.

Bounded workflow follow-ups found three concrete scope weaknesses: unchecked config drift could acquire debug packages; executable flags/interpolation and inherited PlatformIO overrides could bypass the config check; inherited SCons options could select other build scripts/implementation. Fixed the class with one config preflight and an **explicit child environment**, rather than extending an environment blacklist. SCons site directories are disabled. Mutation tests check remote dependencies, new frameworks/boards/debug scripts, external paths, executable flags, variable expansion, known ambient overrides and an arbitrary future environment key. The final fresh reviewer checked the corrected code/tests and returned clean with no maintainability finding. Six bounded review rounds total, within the seven-round cap; no unresolved actionable findings.

Image inspection also caught the reference sample's ignored `board_build.flash_size`; the builder's actual `board_upload.flash_size` now produces DIO/80MHz/16MB metadata. Device bootloader/partition compatibility still requires the next supervised session. Upstream baked SDK app-description/date is identified separately from Newt source/artifact identity.

The full review workflow's local implementation, gate, bounded review and maintainability phases are complete. The explicit task scope prohibits public publication; no remote/push/PR/Codex bot-review phase was attempted. No flashing, SD changes, credentials, services, security/network changes, paid actions, or edits to the old Newt checkout occurred.

## Separate offline-reader experiment

Reviewed 2026-10-01 against `44c454d` on `spike/offline-reader`. The baseline branch and exact binary remain preserved. Focused agents implemented the bounded real FreeInkBook adapter, original synthetic fixture/tests, heap observer, progress core and host integration; root integrated MCU lifecycle/store and documentation. No new dependencies were acquired.

Fresh correctness and thermonuclear maintainability reviews found two actionable P2 defects: rendering could recover a failed reader while leaving activity/position tracking stopped, and idle reading could keep writing identical progress. Error recovery is now explicit: a parser failure pauses activity and stays latched until reopen. Actual Expat malloc-failure injection verifies failure → paint/navigation refusal → explicit reopen → resumed anchor/time tracking, plus rendering failure. The storage adapter now compares normalized encoded durable fields while ignoring sequence, returning without write for identical progress. A fake error-bearing store backend verifies 100 repeated stale-sequence checkpoints make no additional writes, while torn/corrupt writes preserve the prior slot and retries succeed. These invariants close the respective lifecycle/wear classes within this bounded app.

The whole host gate passes strict compilation and ASan/UBSan, reproducible EPUB bytes, real Book/ZIP/XML/layout, chapter crossing, semantic resume, malformed/bounded sources, parser release, activity/time wrap/saturation, record corruption/torn-save/ambiguity, actual NVS orchestration and guarded exact frames. Twelve canonical PNGs roundtrip losslessly; resumed/reopened EPUB frames equal the advanced page. The reader preview was visually inspected. Runtime NVS/power-loss durability, MCU heap behavior and every physical behavior remain untested.

Current experimental firmware and host evidence are in `artifacts/reader-verification.json`; `artifacts/verification.json` is labeled historical and points at the archived baseline firmware. Source/vendor/toolchain remain local and unchanged. No hardware/SD/credentials/publication were accessed. Public PR creation and monitored review remain excluded by the commissioning scope.

Reader review rounds 2–3 extended the storage read-failure class: an exact-size data read error, then native length-query errors conflated by Preferences with missing records, and the startup caller treating failed load as fresh progress. Replaced the lossy interface with a typed native NVS missing/present/failed adapter; every unknown read aborts save, and failed/ambiguous initial load blocks reader entry until an explicit successful reload. Store and native-adapter fault tests cover data/length/namespace failures, no active-slot overwrite, balanced lifecycle and failed commit. Physical NVS behavior remains untested.

Fresh round 4 returned clean after the native error-bearing adapter and startup gate fixes. Independent native/store/sample sanitizer executables passed; the root's complete host gate and clean real C3 compile passed. Four bounded experimental rounds total, below seven. No unresolved actionable findings; thermonuclear maintainability review covered the same experiment and its fixes.

## Separate agent app-loop experiment

On `spike/agent-app-loop`, based on preserved reader `4dd4252`, focused agents implemented the fixed portable app package, trusted C++ model/renderer and file-backed delivery fixture. Root integrated one agent build/test/preview/package command, explicit recipient queue/sync commands, cross-language/end-to-end tests and domain documentation. Both earlier firmware branches and exact archived binaries remain unchanged.

Two bounded fresh local rounds. Correctness and thermonuclear reviewers found one accepted-text class: the second card-detail row was clipped and the metadata wrapper discarded long-token suffixes. Consulted `close-the-class`; app text now has one platform-owned fit policy that splits characters without loss and prefers a word boundary only when the remaining text still fits available rows. The metadata/reader paragraph policy remains untouched. Visible suffix tests cover all maximum-length app fields plus unbroken/spaced sibling cases; canary/margin/determinism tests pass. This closes accepted visible-text loss within this fixed geometry/font contract.

Fresh round 2 returned implementation-clean after reviewing the whole current delta. It identified stale earlier C3 measurement output; source/object/stack reports were regenerated from the final renderer and the new helper appears in the receipt. No unresolved actionable source or evidence finding. Every changed source file remains below 1,000 lines; no general runtime, service infrastructure or permission framework was added.

The complete gate passes strict C++ ASan/UBSan, 7 package suites, 13 delivery tests, trusted decoder/actions/render tests, Python/C++ package agreement and end-to-end build/share-without-queue/explicit-queue/sync/restart/installed-preview checks, alongside all prior reader/firmware helper tests. The actual C3 firmware still translates and links. Tiny-app decoder/render source separately compiles for C3; static structure/stack/object reports and absent allocation references are compiler evidence, not hardware measurements. The firmware does not select tiny apps or implement device package delivery.

Delivery is explicitly serialized, single-writer file-backed host simulation. Atomic installed content/receipt replacement is tested under injected interruption and restart; physical storage durability, Wi-Fi, concurrent production queues, accounts/Registry/share URLs/signing remain outside this task. Public PR/push and hardware operations remain excluded by the commissioning scope. Exact outcomes are in `artifacts/app-loop-verification.json`.

## Separate firmware-loader checkpoint

On `spike/firmware-app-loader`, based on preserved `66015de`, focused agents implemented the shared SHA-256 wrapper, private NVS generation store and firmware/UI/RTC integration. Root integrated generated builtin identity, host/firmware routing, native adapter restrictions, complete gates and C3 evidence. Existing portable bytes/decoder and all prior branches/binaries remain preserved. No acquisition or hardware operation occurred.

Two fresh bounded local rounds and thermonuclear review found four concrete issues: captured CLI stderr lacked a file descriptor, Home mislabeled other loaded apps, fixture tests inherited caller cwd, and host/firmware duplicated semantic routing. Fixed captured output through text, truthful generic Home copy, explicit repository cwd for every test boundary, and one shared pure `applyTinyAppIntent` function. The repeated cwd defect was handled consistently across CLI and complete gate, rather than fixing individual fixture paths. Outside-directory CLI regression and an actual entire gate from `/tmp` pass. Shared routing tests and reader/device framebuffer comparisons cover the actual helper used by firmware. Follow-up reviews returned clean with no remaining actionable findings.

The full strict ASan/UBSan gate passes package digest vectors/corruption/versions/output preservation, two-generation write/commit/readback/restart faults, unknown native reads and bounded namespace/key restrictions, RTC app identity/bounds, real reader routing and all previous reader tests. C3 source translates and links with loader, decoder, renderer and routing symbols present. Compile-only load/save template and structure reports supplement actual firmware's per-function stack reports; they do not measure precompiled provider/runtime peaks or physical durability. Exact previews were visually inspected. First hardware baseline is unchanged; no physical app installation path was exposed. Evidence: `artifacts/loader-verification.json`.
