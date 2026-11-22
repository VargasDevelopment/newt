# Selected design direction and implementation requirements

## Trial milestones and deliberate stopping point

Updated after the owner's October 1 hands-on feedback. **M1 is a daily-reader
trial, followed by a 3–5 day feature freeze. M2 is creating and trying a real
tiny app.** The X3 remains the constrained proving ground for the broader Newt
vision. Do not make M1 wait for a generalized application runtime or the full
future roadmap. No time-based ETA is justified before SD memory and physical
resume/refresh behavior pass their gates.

For each subsequent reading slice, review one working Library/open/turn/exit/
reopen interaction and its failure recovery before expanding polish. Freeze and
software-verify an exact candidate, then use the existing reading journey at the
next separately authorized owner hardware session. Failed physical reading
pauses feature expansion; host previews do not settle it. This moves the existing
check earlier without weakening final UI, battery/wake, optical/buttons or owner
acceptance. Follow [the agent workflow](agent-workflow.md#prove-a-working-reading-slice-before-polish).

| Milestone | Acceptance gate | Current evidence / remaining work |
| --- | --- | --- |
| M1: useful offline library | Open ordinary compressed EPUBs from the spare card, choose chapters, render supported Unicode honestly, and return to the verified saved passage. Preserve CrossInk book/state originals and offer a clearly reviewed migration preview. | Combined SD/UX source passes host and all four native gates; the current rendered-page probe opens first readable chapters in 59/61 backup books, with two explicit text/container-profile unsupported cases. Physical SD heap, stack, latency and glyph acceptance are pending. Actual legacy passage has an approximate, review-required SDK page-start proposal; exact within-page translation/settings are unsupported. |
| M1: dependable reading | Successful page/chapter changes, pause/menu/settings, per-content verified progress, reboot and power-button sleep/wake retain the selected book and visible passage. Missing/changed cards, corrupt records and NVS exhaustion remain visible and preserve prior state. | Installed fixture progress/settings survived a restart. The owner reports wake returning Home. Installed `16cd3bc` still fails owner sleep/wake acceptance. Its linked noreturn sleep API removes the SDK recovery path. The new supported try-entry boundary, checksummed rejection tag and primitive RTC restore pass host/native checks; actual wake still needs supervised physical evidence. Two-slot durable SD selection, per-content progress and digest-checked RTC resume pass source reset/fault matrices; physical SD/reboot/wake acceptance remains outstanding. |
| M1: coherent Frosted UI | Home, book picker, reading menus/settings/statistics/device screens share system typography, spacing and quiet borders. Preserve improved reading margins. Footer uses measured text widths and an explicit gap. Default sleep uses the approved logo; original character remains a five-press Easter egg. | Four-gray Home is installed. Owner accepts body margins/responsiveness, reports whole-Home flashing on focus, cramped footer groups and mismatched old UI/footer lettering. Theme/footer revisions are installed; owner accepts the approved default sleep logo. Pocket Pause now has readable prompts and saved enabled choices in source. Remaining Home/wake/app optical acceptance is pending. |
| M1: efficient, safe refresh | Focus changes avoid unnecessary whole-panel flashing; reading uses a separately specified five-page full-refresh cadence. Respect controller rebase/cleanup and never treat gray selector bytes as monochrome. | Installed regional XTH4 still flashes the whole navigation area. The revised default-off UC8279 extension proves a 480-byte binary island and sends 960 bytes using existing DU for focus changes, with full cleanup on the tenth. Ordinary Home remains pixel-identical. Host/linked gates and source review pass; optical quietness, gray preservation and ghosting remain unverified. |
| M1: reviewable installation and recovery | Frozen source, clean host/native gates, bounded reviews, exact image hash/sector scope, fresh backup and full readback precede a separately approved flash. Keep replacement device untouched. Check failure/recovery procedure against the verified layout. | Installed `16cd3bc` passed exact scoped full 16 MB readback, startup-NVS preservation and clean boot checks. New source candidates remain unflashed pending final reviewed gates, exact identity and a fresh backup/recovery preflight. Restoration writes require separate authorization. |
| M1 stop | The owner can read actual books, resume reliably and tolerate no unresolved crash, data-loss or severe usability defect. Freeze new features for 3–5 days of ordinary use; accept only crash, data-loss or severe-usability fixes during that window. | Do not declare this gate met from compile or host simulation. Collect actual reading, battery/sleep and failure feedback before moving on. |
| M2: a real tiny app | Describe an idea, generate an app using trusted components/actions, preview/test it, install on hardware and try it through a repeatable update path. Authoring plumbing stays with the agent. | Existing Pocket pause and agent build/test/package tools are preserved. Hardware app installation/sharing remain unfinished; they are the next milestone after the reader trial. |
| Subsequent updates and release | Establish verified USB/SD installation, then earn OTA with compatibility, power, interruption and rollback checks. Complete release QA, recovery docs, provenance/licenses, supported CrossInk-version matrix and approachable project documentation. | OTA is requested but unimplemented. Original Newt code license remains an owner decision before open-source release; no public publication is authorized. |

The SD source profile admits exact ZIP container member names of at most
511 bytes, with a separate terminator byte in the SDK resolver and retained
reader selection. Longer names fail cold admission before metadata parsing;
this keeps both 48 KiB arenas and existing retained-memory/stack bounds.
Display titles retain at most 255 bytes ending on a complete UTF-8 scalar,
with a generic title when empty, so SDK title truncation cannot reject an
otherwise readable chapter. Chapter text and source identity remain exact.

Development USB status/captures and bounded fault tests support both milestones;
they are tools, not a third product milestone. Source/build work continues
autonomously. New firmware writes, restoration writes, owner-assisted wake tests
and release/license choices remain explicit boundaries. Preserve all prior
Newt R&D and accepted hardware/source checkpoints.

Recorded October 1, 2026 from the owner's follow-ups. **The owner selected Frosted unequivocally as Newt's visual direction** and likes all presented mockup contexts. The refined reference-backed reading views are in [round5](../../../task-15/round5/README.md), used read-only. These requirements do not establish hardware acceptance or authorize device access/flashing. Exact button positions/mappings remain pending the photograph evidence and physical X3 check.

| Area | Requirement |
| --- | --- |
| Direction | **Frosted is selected.** Gazette is retained as historical comparison, not an unresolved theme choice. |
| Soft background | Aim for Gaussian-style spatial softness. Treat filtering and final quantization as separate choices. **Four-level grayscale is preferred over Floyd–Steinberg where actual hardware supports acceptable quality.** Floyd–Steinberg remains a comparison/fallback candidate, not implemented behavior. Ordered Bayer texture itself is not the desired effect. |
| Book identity | Show the actual current book's cover, with offline availability as the default. If the cover is unavailable, use an honest fallback rather than presenting an invented cover as the book's artwork. |
| Reading page | Maximize book text. **No persistent button legends or book title consuming the reading page.** Use a compact footer containing battery, chapter page **x/total** and **overall book percentage**; the selected slim progress bar completes this footer. The owner subsequently approved Option A and the reading menu; retain an off setting for comparison. This supersedes the earlier mandatory progress-bar preference. |
| Reading progress | Footer/bar values come from the same real reading position; keep chapter page counts and overall book percentage distinct. Do not substitute chapter percentage for book progress. |
| Reading menu | Include **Reading statistics** in the reading menu. Present earned, correctly scoped reading evidence; the menu and overlays respect the reader's orientation. |
| Sleep statistics | Optional, earned details such as **“Afternoon Reader”** or **“15 day streak.”** These are examples of desired presentation, not values to manufacture or eligibility rules already chosen. |
| Physical controls | Where labels are shown in system/menu/overlay contexts, place them so they clearly correspond spatially to actual buttons in the chosen orientation and match their actions. The ordinary reading page has no persistent button legends. |
| Familiarity and controls | Preserve familiar CrossInk mappings unless evidence establishes a reason to change them. Do not guess mappings from our prototype footer. **Customizable buttons are a requirement**, following the owner's explicit “We should have that” response to the Controls customization section. Exact mappings and customization scope still need design/physical verification. |
| Reader/system separation | **The system defaults to portrait.** Reader orientation is user-configurable; the owner's personal book-reading preset is **LANDSCAPE CCW** with **Lexend Deca 14pt**. Reader overlays respect reading orientation; exiting to Home returns to the coherent portrait system. System typography is chosen separately and need not use the reader font. |
| Firmware updates | Support approachable **OTA firmware updates**, distinct from tiny app/package installation. The owner explicitly requested this on October 1. Verified bounded downloads, device/image compatibility, power preflight, interruption safety, rollback/recovery and honest update status must be designed and tested before implementation. |
| Settings | Borrow CrossInk's coherent grouping/nesting and familiar behavior, presented with Newt's own theme. Keep ordinary reader choices understandable without exposing platform plumbing. |
| Responsiveness | Decoration must preserve snappy interaction. Compute a softened background once where feasible, keep crisp controls responsive, and simplify the dialog or reserve the effect for sleep if measured cost hurts interaction. |
| Reading refresh | Start with an **adjustable full refresh every five reading pages**, matching the owner's current CrossInk preference. Actual panel/mode requirements may trigger an earlier refresh or cleanup. |

Before implementing the five-page cadence, explicitly define what counts and resets it: initial page versus subsequent successful page changes; forward/back turns; chapter crossings and clamped/failed turns; telemetry/menu/modal repaints; book/session changes and sleep/wake; and whether an independently forced full refresh resets the reading counter. The candidate semantics are now recorded below; the installed image remains on the earlier policy. Grayscale/Frosted transitions must perform required SDK cleanup/rebase or stronger refresh immediately, independently of this page cadence.

The owner's hypothesis is that Frosted may aesthetically accommodate some ghosting in decorative areas. This is a visual/hardware hypothesis to test, not a solved defect or a reason to reduce cleanup. Render intentional texture explicitly; the design must remain understandable regardless of the previous image. Uncontrolled residue must not obstruct reading text, progress or physical-button labels. Future acceptance checks are recorded in [BRINGUP](BRINGUP.md#future-design-refresh-acceptance).

The existing [app/domain model](../CONTEXT.md) remains unchanged: platform-owned components and semantic actions hide complexity from people. Covers, progress and earned statistics should feed those trusted components rather than becoming user-authored plumbing.

The current [reader model](002-bounded-offline-reader.md) stores a semantic locator, active reading time, forward-page count and qualified-session count. It has no calendar/timezone history sufficient to establish a 15-day streak, and its exposed fixture has no real-book cover pipeline. Those preferences remain future requirements, not implemented claims. Define badge/streak eligibility against actual persisted reading evidence before displaying earned labels.

The [host blur probe](host-blur-probe.md) establishes bounded Gaussian filtering and separate four-level/Bayer diagnostics. It **does not implement Floyd–Steinberg, a Frosted firmware theme, real cover extraction or physical-button labels**. Native grayscale timing, cache ownership, cleanup and sleep behavior remain hardware gates. Blur is decorative and never privacy protection.

## First-boot character Easter egg

The owner requested on October 1, 2026 that the original goofy first-boot Newt
character survive in the polished firmware as an Easter egg: activate it by
clicking something **five times**, then **Back exits**. Preserve the actual
original drawing, including awake and asleep variants, rather than replacing
it with a newly illustrated interpretation. The exact function and original
rasterizer are archived with provenance in
[the first-Newt artwork](../assets/first-boot-newt/README.md).

The proposed discoverable spot is **Settings → About → version entry**, with
five deliberate OK presses opening the character view and Back returning to
About. The owner's five-press activation and Back exit are requirements; this
specific location is the proposed implementation, not a separately confirmed
owner choice. It must not require editing a manifest, installing an app or
changing firmware/settings. The Easter egg is not implemented by this archival
change, and the deployed baseline remains unchanged.

## The owner's reported CrossInk preset

Recorded October 1, 2026. Preserve the reported spelling and units; these are the owner's personal **book-reading** settings, not a universal Newt default or a verified interpretation of CrossInk's internal representation:

```text
FontFamilyLexendDeca,14pt,lineSpacing100%,wordSpacing0,antiAliasingON
screenMargins top/bottom5,left/right5
orientation LANDSCAPE CCW
paragraphjustify,hyphenationOFF,extraParagraphSpacingON,forceParagraphIndentsOFF
```

The margin values `5` and word-spacing value `0` were reported without units. Do not relabel them as pixels, points, percentages or another unit until the actual installed UI/implementation establishes their meaning. Preserve `14pt` and `100%` as reported; do not silently equate font points with SDK pixels. The previous five-reading-page full-refresh preference remains the adjustable starting cadence described above. The owner's Reddit anecdote does not establish a population preference for portrait or landscape.

The owner subsequently clarified that Lexend Deca 14pt and LANDSCAPE CCW apply **only to actual book reading**. Keep the reader preset separate from system design/settings. The system default is **PORTRAIT**; do not rotate the launcher/shell to landscape or force Lexend Deca across the system. Reader overlays follow the active reading orientation; exit to Home restores the portrait system layout.

## Familiar settings structure, Newt presentation

Read-only source review on October 1, 2026 used official CrossInk revision `b25beb13761d5851f98e7eada09aa5e7d430df48`; no code or assets were imported. Its [SettingsActivity](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/activities/settings/SettingsActivity.cpp#L247) separates Display, Reader, Controls and System, with device-capability filtering. Its [SettingsList](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/SettingsList.h#L1056) places typography under Reader → Font Options, geometry/paragraph choices under Reader → Page Layout, and Top/Bottom plus Left/Right under Screen Margin. Controls groups power, front buttons, side buttons and quick actions, including separate system/reader remapping.

Borrow that purpose-based nesting and familiar behavior with Newt's visual language. Keep book typography/orientation in Reader and shell presentation in the system/display groups; adding a personal reader preset must not silently change the launcher. This is a Newt requirement, not a claim that every upstream option is implemented or appropriate for X3.

CrossInk's [ButtonRemapActivity](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/activities/settings/ButtonRemapActivity.cpp#L30) guides physical assignments, rejects duplicates, and provides cancel/reset paths. [MappedInputManager](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/MappedInputManager.cpp#L43) derives input and hints through common mapping/orientation functions. Newt's customization should likewise keep labels and dispatch consistent, distinguish reader/system choices and make cancellation/reset understandable. These source observations establish UX precedents, not physical-device acceptance or the owner's exact mapping.

## Bounded reader/settings follow-through

These are implementation gaps to address in small slices after the refined reading references and baseline hardware checks, rather than a promise to implement every setting at once:

| Slice | Existing foothold and remaining gap |
| --- | --- |
| Typography and paragraphs | [The reader adapter](../src/reader.cpp) now uses a bounded licensed printable-ASCII subset of the exact Lexend Deca14pt atlas in the approved reading mock. [Decision 006](006-reader-typography.md) records its baked metrics, proportional advances/kerning and shared monochrome rendering. It requests SDK justification, but does not implement native AA or complete preset parity. Pinned FreeInk's [LayoutParams](../vendor/freeink-sdk/libs/book/FreeInkBook/include/layout/ChapterLayout.h) provides line-spacing, paragraph-spacing, alignment and optional hyphenator hooks. Newt exposes none as user settings. The fixed subset retains its OFL notice and source conversion provenance; runtime font selection, additional scripts/styles, native antialiasing, word spacing and exact extra-spacing/forced-indent behavior remain gaps. Headings use bounded nearest resampling of regular glyphs. `wordSpacing` and a force-indent preference are not exposed by this LayoutParams API. Hyphenation currently has no configured hyphenator; that alone is not a verified match for every CrossInk OFF behavior. |
| Geometry and orientation | [ReaderViewSettings](../include/newt/reader_view.h) now separates reader orientation and the selected configurable slim bar from the portrait system. Shared source supports 792 × 528 landscape CCW and 528 × 792 portrait within the same 52,272-byte frame; the [adapter](../firmware/frame_adapter.h) selects the corresponding native transform. Geometry uses the selected reference's 5-pixel margin and compact footer bounds, not an inferred conversion of the owner's reported units. Host tests verify reflow/anchor resume and portrait Home on exit. The next shared slice now adds orientation-preserving reader overlays and verified persisted orientation/bar settings; physical orientation and native durability remain untested. |
| Controls | [actionForButton](../firmware/main.cpp) has fixed button-to-semantic-action mappings. Add an understandable Controls customization UI and persisted choices; verify actual button identities, orientation behavior and familiar CrossInk mappings before selecting presets. Keep app-facing actions semantic. |
| Settings and persistence | The bounded menu/settings slice adds a draft/Save/Cancel screen for orientation and slim bar, with a versioned two-slot 24-byte Newt-only NVS record. Verified read-back is required before applying a choice. Unknown/future/ambiguous storage blocks saves; confirmed empty storage uses defaults. Host fault tests and linked firmware are separate from native durability evidence. Other preferences remain future work. |
| Refresh | Installed firmware retains the general-paint cadence. The M1 source candidate has the separately counted, adjustable reading cadence documented below; physical acceptance remains pending. Gray transitions retain forced cleanup. |
| Minimal reading chrome and menu | [renderReaderChrome](../src/reader_chrome.cpp) now preserves full-width book text and draws battery, chapter page x/total and whole-book page-start percentage without a persistent UI title/button rail. The optional bar changes only its reserved region. Each bounded spine is indexed once on open for a semantic text-length denominator; later paints reflow only the current chapter. First-page progress is 0%; other pages measure their first character, and the book's last page reads 100% (#105). The shared menu now exposes Return to Reading, Chapters, Reading Settings, Reading Statistics, Mark as finished and Home. Statistics use only actual per-book active time, qualifying forward-page count and qualified sessions; overlays pause tracking. Calendar streaks and aggregate library history remain separate gaps. |

The first bounded geometry/chrome implementation and subsequent steps are recorded in [Decision 003](003-frosted-implementation.md). [Its receipt](../artifacts/frosted-reader-verification.json) separates host/compile evidence from hardware. The subsequent [menu/settings slice](reader-menu-settings.md) adds shared menus and verified preference storage. The [typography slice](006-reader-typography.md) adds a fixed real reading face. Native grayscale/AA, full font selection, control customization and OTA remain future slices; no device access/flashing has occurred.
# October 1 hardware follow-up

The owner accepted the installed guarded reader's first hands-on pass. Improve the reading body's physical safe inset and split footer information into battery left, chapter alone centered, and page fraction plus overall percentage right. Preserve the selected optional slim progress bar without crowding. This is a bounded source/build slice; a4d8812 approval does not authorize its installation.

Home direction is now selected: **square-card B**, small wordmark, centered clock, seamless Frosted, quiet outlines and aligned metadata. The owner: “This UI is wonderful and what we should ship”. The selected design reference is Library `libfile_0134c1de11948191ab5054fe0cb6169a`; the design task owns the exact asset/geometry handoff. Finish reader spacing and cold-restart persistence before implementing it. Four-gray mock output and actual panel rendering must remain distinguished; no automatic flash follows design selection.


## M1 UX candidate and deferred ports

The UX candidate uses the approved Home Inter atlas for reader chrome, opaque
reader panels, settings, device/About and trusted app cards. Reader body layout
and the 16px inset are unchanged. The normal sleep image uses the approved
brand silhouette and original wordmark. Device → About → version accepts five
consecutive Select presses to show the canonical first-Newt drawing; Back
returns to About. About/Easter-egg state is session-only; reboot resumes the
Device route rather than a hidden view.

Reading full-refresh cadence defaults to five **successful semantic page
changes** and can be selected as one, five or ten in Reading settings. Both
forward and backward turns, chapter crossings and explicit successful chapter
jumps count once. Clamped/failed turns, menu navigation, footer/telemetry
repaints and checkpoint warnings do not count. Initial opening, reflow after a
verified settings save, reopening and sleep/wake start a new cadence with a
full paint. Any requested full paint resets the count. The SDK may independently
promote an earlier waveform; its actual promotion is not observable through the
pinned facade, so the next requested cadence remains conservative. Gray/BW and
orientation transitions always request a full rebase independently of cadence.
Every monochrome screen retains a separate bound of nine requested fast paints
before the next requested full refresh, including app/Device and reader overlays
or telemetry. Any full monochrome paint clears that bound and the current reader
cadence. UI repaints never count as semantic reader page changes.
Preferences retain the existing 24-byte dual-slot record: v1 reads default to
five; v2 writes cadence into its previous reserved byte. Unsupported, corrupt
or ambiguous records still block replacement.

The Home no-op cache avoids repeated paints of identical visible inputs and
invalidates after any other screen. Normal firmware performs full Direct gray
activation on focus moves. The installed regional XTH4 experiment also visibly
flashes on every move and failed acceptance. Its replacement regional candidate
uses a constant-weight Home section list and binary cursor in a white gutter.
Only a completed full Direct pass with actual equal uploaded selector planes
allows the typed UC8279 island DU operation; nine accepted moves precede full
cleanup on the tenth. Rejected operations safely fall back to full Direct.
[The proposal](../firmware/home_refresh_proposal.md) records command proof and
physical acceptance criteria separately.

Pocket Pause opens directly to a randomly selected enabled pause. Prev/Next
chooses another without immediate repeats when alternatives exist. Select opens
three enabled-choice rows and explicit Save; Back discards drafts. Its bounded
20-byte dual-slot preferences use only `newt_pause`. Failed/unsupported/corrupt
loads show unavailable choices and prohibit replacement rather than silently
enabling prompts. Wake restores committed choices and discards drafts. An
absent saved prompt remains absent through repeated sleep; only the verified
Pocket package permits that retained sentinel. Generic apps remain strict.
The 615-byte app package format and generic trusted actions are unchanged.

M1 remains gated on the separately owned SD reader/migration work and owner
verification of the selected book passage resume, pages, settings, sleep and tested
recovery. After combined acceptance, propose the existing 3–5-day reading trial
freeze; do not begin the Apps SDK/install experiment (M2) during that freeze.

**Deferred hardware port:** classic X4 support is a later milestone, well after
X3 M1 and M2. Keep the existing native-display/reader geometry and semantic button
boundaries useful for future ports. Do not add untested X4 paths now. Classic X4
and X4 Pro are distinct device targets and require independent board, display,
button, power and recovery evidence.
