# Stage Home

Stage is Home in the one regional DEV firmware build (owner decision, 2026-10-26).
Every boot and host preview uses the same C++ Stage renderer with quiet binary
focus. There is no Home theme setting, DEV selector or retained Home token.
The old `newt_home` SD/NVS data is recognized as Newt-owned historical data,
ignored and left intact; missing or corrupt old data also means Stage.

Home's backdrop, focus bars, cover session, gray fallback and Sleep composition
are unchanged from the previous Stage-selected path. The shared atlas and the
system bars/footers in the Frosted-named source files remain because other
screens use them. Only Frosted Home's renderer, cover/progress stamps and
selection workflow are retired. The unchanged SDK patch still requires
`FREEINK_EXPERIMENTAL_REGIONAL_GRAY=1`; Newt has no regional build switch.

New DEV build IDs are the plain 12-hex source revision (plus `-dirty` for a dirty
tree). Older candidates retain their suffixed IDs and validate against their
own pinned receipts. Preview/host, native compilation and physical checks are
separate evidence. This removal still needs the Lead's bench checks: cold
power-on, USB-powered sleep/wake and unplugged battery sleep/wake, with reset
and power evidence for each. The bench already has a Stage record; absence is
a host case, not a device acceptance claim.

## Historical implementation and evidence

The following records describe the earlier opt-in experiment, not current
selection instructions. The current reviewed renderer matrix updates only Home;
its other nineteen frame hashes are unchanged. [Decision 061](061-experimental-baseline-acceptance.md)
records its accepted scope; cold power-on remained a separate check.

## Step 2 implementation

`src/home_atlas.cpp` defines the atlas once, from scoped sections of the existing
generated `frosted_home_data.inc`. Both layouts use `home_atlas.h` for glyphs,
kerning, decoding and width. Frosted retains its fixed-label tables. The three
shared arrays cost 365,597 B on RV32; they must appear exactly once in the linked
symbol table. No generated-file exclusion or external adapter is added.

`src/stage_backdrop.cpp` links the existing 104,544-byte asset directly. An
assembly extent assertion checks its size; a generator-checked digest literal
binds host object reuse to the asset, and native input identity binds its bytes.
There is no decoder, backdrop allocation or extra framebuffer. Stage uses its
own progress text fit with the shared R24 face in the 440-pixel text region.

The narrow development actions `PRESS home-stage` and `PRESS home-frosted`
travel through the existing session, ticket and input-queue gates. Selection
uses an unused bit in Home's existing refresh state and survives navigation.
An independent 20-byte NWPH v1 SD A/B preference record commits and verifies
explicit Stage or Frosted before publishing it. It restores before the first
paint after every reset class, including complete RTC loss. The supported SD
state file is `/Newt/State/newt-home.nrs`; reading progress, resume intent,
NVS and the existing card contents remain independent. A four-byte RTC mirror
is only a trusted warm fallback when the SD medium is unavailable. Mounted
missing/corrupt records keep Frosted. Ambiguous acknowledgement is reconciled
with one bounded readback; unconfirmed commits keep the previous live choice
and honestly warn that a saved choice may restore after restart. Production
allocates no token, persists no DEV preference and exposes neither action.
DEV RTC stays at 304 bytes and production at 300; SRAM and inherited selected
stack budgets are protected.

The rejected `670` image used three disjoint 108×8 binary windows with a
2 px selected extension. Each ordinary move triggered two DU activations.
Fresh same-image measurements found median Home-focus request-to-terminal
latency 1,012.5 ms for Stage versus 581.9 ms for Frosted (12 moves each,
including their separately identifiable periodic cleanup). Warm opens measured
2,872.0 versus 2,125.3 ms (three each). This is measured comparison, not an
approved performance baseline.

The correction retains constant R24 labels, constant 2 px rounded frames and
no moving halo. Its selected straight top-edge extension is 8 px. There is no
white connector strip: the three buttons remain separate over the unchanged
gray backdrop. Historical Approved-style previews remain pixel-identical.

Three immutable native regions `{568,374,8,108}`, `{568,210,8,108}` and
`{568,46,8,108}` contain only binary button interiors. Their 324-byte checkpoint
fits the existing 480-byte driver storage. The explicitly opted-in typed plan
coalesces the old/new bars into one bounded DU activation. Both OLD and NEW
planes receive identical white-to-white padding in all gaps and unselected
rows; only selected binary bytes use their verified old/new content. This is
based on the UC8279 no-drive WW selector and command tests. Host success does not prove optical behavior. The subsequent 24-hour owner
report accepts the installed Home UI and improved navigation; it is owner
evidence rather than an instrumented optical waveform measurement. The altered gap RAM invalidates the complete regional
gray baseline, while the binary checkpoint remains authoritative for focus.
Default controller plans retain their existing per-region behavior.

Regional Stage now uses the existing Overlay base/mask lifecycle for its full
composition and transitions, with the same binary proof bound before base
capture. A rejected binding, missing mask allocation or unsupported Overlay
uses the tested complete Direct/monochrome fallback. A due device-wide cleanup
resynchronizes a complete Home composition; ordinary focus uses the one DU
path. No PNG/progressive support, extra framebuffer or duplicate atlas is added.

Sleeping from Home composes the selected Stage layout before the existing blur
and Sleep stamp. Open-reader sleep still composes the reader page in its
orientation. Sleep does not allocate another framebuffer or reload decorative
cover art after leaving Home. The shared renderer and focused tests cover both
choices. Host cold-start tests discard RTC and verify the actual durable startup
wiring; a hardware external reset restored Stage without reselection. This
external reset does not independently prove loss of RTC. The subsequent owner
report accepts repeated unplugged battery sleep/wake, reading position and the
installed Home UI. Cold power-on remains separately unconfirmed; accepted
behaviors need no repeat merely for paperwork.

All four ELFs of the first installed candidate contained one atlas. Production excludes Stage and its
backdrop; development includes one 104,544-byte backdrop. Selected stack frames
and SRAM remain within protected budgets. The combined-image loop-task whole-boot watermark was 7,052 B minimum free
of 16,384 B and remained so through reading and generation. In that rejected candidate, ten of twelve focus
moves used the regional gray path; two used full Direct gray, including cadence
cleanup. The cause of the first full pass was not isolated. This classifies driver behavior, not
panel quality. The unsupported-gray monochrome fallback has host/native coverage
and was not exercised on this gray-capable device.

The sections below retain the step-1 contract and its historical evidence.
Descriptions of duplicated atlas data, unlinked firmware and future step-2 work
below refer only to that historical revision, not the installed candidate.

Stage is the "blurred newt, open dock, halo focus" design in
`design/home-refresh/SPEC.md` (branch `design/home-visual-refresh`). Its
renderer is `src/stage_home.cpp`, drawn on Home's own Inter atlas.

The spec is not in this tree, and its branch may move. This step follows
these git blobs, which identify the spec whatever branch holds it:

- `design/home-refresh/SPEC.md`: `f7f2b65ded3345d5b0f2d925eed49cba4fc5c75b`
- `design/home-refresh/tools/build.py` (the mockup recipe):
  `7601cecfd6e3b42c16a12feb70821ff8044170bd`

`git ls-tree -r <revision> -- design/home-refresh` shows whether a revision
still holds them. A later spec change is a new step, not a silent
reinterpretation of this one.

## Selector

`include/newt/stage_home.h` adds `HomeStyle { layout, focus, backdrop }` and
`renderHome(state, info, data, plane, frame, style = {}, cover = nullptr)`.

- The default style is `HomeLayout::Frosted` with
  `HomeFocusStyle::Approved`: exactly `renderFrostedHome(..., focus, cover)`.
- `HomeLayout::Stage` without a backdrop is also today's Home.
- `HomeLayout::Stage` with a backdrop calls `renderStageHome`.

A later runtime opt-in only has to hand a style value to the paint. This step
adds no setting, no `-D` flag and no firmware caller. `src/frosted_home.cpp`
and `include/newt/frosted_home.h` are unchanged from this branch's base, so
today's Home compiles from unchanged source (see "Firmware evidence"). Their
git blobs are:

- `src/frosted_home.cpp`: `07da1c96a47166aa7e4c2784750d09bdfd436ac2`
- `include/newt/frosted_home.h`: `db619c34a5ff6353098855b9d59a8c21fd6a8e8c`

## Text

Step 1 kept its own copy of Home's Inter atlas and Frosted Home's generic
(dynamic) text path in `src/stage_home.cpp`: the same atlas include, all nine
faces in Frosted's order (the host test compares the two enumerations), and
the same fit, kerning, eight subpixel phases and anchor clipping. It never
uses Frosted's compiled fixed-label rasters or metrics, even for strings that
have them.

Ink blends over the paper with the spec mockup's rounding. At Home's
four-level glyph coverage (a `static_assert` checks that every glyph stamp
is packed) that gives Frosted's tones exactly. Overlapping glyph edges blend over the
paper, as in Frosted Home.

Both themes draw the title from `homeText`, which spells it with the UI font
as `uiBookTitle` does: accents fold to plain letters, typographic dashes and
quotes to their ASCII forms, and characters the font cannot spell, or
malformed bytes, are left out rather than drawn as `?`; a run left out reads
as one space, so it never joins two words. A title with no letter
or digit left reads "Your SD book". The folded title fits one 128-byte buffer;
a longer one is cut to whole codepoints and "…". Stage's wrapped title lines
are spans of it, not copies, so a line never grows past 127 bytes.

The progress line uses Frosted Home's rule (`homeChapterProgress`) with Stage's
own width: "Chapter N · P% read" while it fits R24 across 440 px, else "P% read".
The percent is plain, exact or estimated, as in the reader footer. A chapter
label the font cannot spell leaves only the percent. The host test pins the
wider line with a nine-digit chapter.

## Layout and states

The geometry follows the spec's `b_home`, `b_dock` and `footer`:

- the header (wordmark, clock and battery);
- the cover slot;
- the title in B32 with a word ellipsis after two lines;
- the progress line ("Not started" for a book not yet opened), and the bar
  while progress is known;
- the "Read to carry on" key cap, "Read to begin" for a book not yet opened;
- three 128x132 buttons, whose focus is a 4 px edge, a B24 label and an
  18 px Bayer halo;
- four white rail caps (Read, Select, ◀, ▶) on the frost.

The spec calls the header unchanged, but Stage follows the spec mockup's
recipe, not Frosted's pixels:

- the wordmark is the approved crop area-averaged to 86x28 and multiplied
  over Stage's frost, where Frosted uses high-quality scaling over its haze;
  775 of the 2,408 tones in the wordmark box differ from Frosted Home's;
- the clock and battery use the generic atlas path, so "10:24 AM" and
  "Offline · 84%" are not Frosted's compiled label rasters (nor are the rail's
  "Read" and "Select").

The cover is #19's prepared 144x188 snapshot drawn 1:1 at (192,94), with the
mat shrunk to (186,88) 156x200. It is drawn only when the snapshot is ready
and no card flag is set, as in Frosted Home. Otherwise a book gets the
typographic missing-cover tile. Frosted's fictional `fixtureCover` art is not
used.

States:

- **No book** (null title): a "No book" slot, "Nothing to resume", the Books
  hint, and no bar, Read hint or Read key. Routes are unchanged.
- **No card** (`kHomeNoCard`, not in the spec): an empty, unlabelled slot,
  then Home's own card words, with the card hint in place of the key cap.

The halo is the spec formula, 0.9 x (1 - d/18)^1.6 against the 8x8 Bayer
pattern, stored as 64 integer squared-distance limits. The renderer uses no
floating point. The host test re-derives every halo pixel from the spec's
`hypot`/`pow` code.

Measured on the host, a focus move changes only the old and new button+halo
windows:

| Move | Changed pixels | Window |
|---|---|---|
| Books to Apps | 9,102 | 325x161 |
| Apps to Your X3 | 9,195 | 321x161 |
| Your X3 to Books | 9,269 | 485x161 |

The spec's mockups give 9.1-9.2k px in 325x161. The window contains gray
frost, so it is not eligible for today's 1-bit regional path.

## Backdrop representation and cost

The backdrop is static, so it is computed once at build time and never per
focus move. `tools/make-stage-backdrop.py` generates it deterministically
from the approved brand reference (see `assets/frosted-home/PROVENANCE.md`).
It is stored as `assets/frosted-home/stage-backdrop.bin`: the LSB plane, then
the MSB plane, of the four-tone image, in the frame's own packing.

Rendering copies a plane straight into the frame (`memcpy`). Monochrome is the
inverted MSB. There is no decoder, scratch buffer or allocation.

If it were ever linked, the asset would cost exactly 104,544 bytes of flash
rodata and 0 bytes of RAM. The renderer's own code is not linked or measured
in firmware. Alternatives that were measured and rejected:

| Option | Flash | Why rejected |
|---|---|---|
| zlib | 36,906 B | Needs an inflate path plus a window or frame buffer. |
| 8x8 tile deduplication | 95,612 B | Almost no saving. |
| First-use computation | a new gray source asset | Firmware links only the 1-bit Sleep stamp, not the gray brand source. A streaming σ22 blur still needs a 133-row window (70 KB at one byte per pixel, more at working precision) and a long CPU pass (not measured). |

Run-length coding does not help either: the dithered planes have 188,354
(LSB) and 52,605 (MSB) runs. A later step that links Stage revisits the format
under the flash audit.

## Step-1 firmware evidence (historical)

The invariant: this step links nothing new into firmware, and today's Home
compiles from unchanged source.

- The asset lives under `assets/`, and only host tools and tests read it.
- `renderHome` and `renderStageHome` have no firmware caller, so
  `-ffunction-sections -fdata-sections --gc-sections` discards them, their
  atlas copy and their tables.

The method: at the base and at this branch, build all four variants
(default, `--dev-usb`, `--regional-home`, and both) with
`python3 tools/firmware-build.py --clean --health-report` and the same
project-local toolchain. Then compare the receipts, symbol tables (`nm -S`),
every loadable ELF section's bytes, and the images. They must match, with no
Stage symbol. The only differences allowed are the build time, the dev-usb
revision string, and the hashes and checksum esptool writes into the image.
The pull request records the measured results. Compile evidence is not
hardware evidence.

## Stack

The firmware build compiles `stage_home.cpp` in every variant but does not
link it. Its `-fstack-usage` file is the same in all four variants. To get
call chains, each unit is recompiled with the firmware's recorded flags (its
DWARF producer string) plus `-fcallgraph-info=su`. That compile's `.su` file
and instructions match the firmware object. Chains stay inside each unit, so
these are static figures, not runtime peaks.

| Function | Own frame | Deepest chain in its unit |
|---|---|---|
| `renderStageHome` | 320 B | 800 B: `drawEmptySlot` 48, `text` 288, `width64` 64, its helpers 64, `kern` 16 |
| `renderFrostedHome` | 496 B | 928 B: `drawSystemFooter` 80, `drawText` 224, `stamp` 96 + 32 |

`wrap` (128 B) and `triangle` (192 B) are on shallower chains.

## Step-2 plan recorded before linking

- **Atlas.** `stage_home.cpp` and `frosted_home.cpp` each include
  `frosted_home_data.inc` into an anonymous namespace. GNU ld does not fold
  identical internal-linkage arrays, so linking Stage today would add a second
  copy: `kHomeRuns` 150,383 B, `kHomeGlyphs` 141,120 B and `kHomeKerns`
  74,094 B, 365,597 B in all on the C3. Step 2 plans to fix this:
  1. Move these three tables into one externally linked unit,
     `src/home_atlas.cpp` with `include/newt/home_atlas.h`. Move `next`,
     `codepointIndex`, `glyph`, `kern` and `width64` there too.
  2. Leave Frosted's fixed-label tables in `frosted_home.cpp`.
  3. Draw both layouts from the shared atlas.
  4. Prove Stage Home with the one clean `--health-report` build.
- **Backdrop.** It costs 104,544 B of flash as stored (see above).
- **Stack.** Stage must join the selected stack measurements within today's
  budgets.
- **Progress words.** Give Stage its own fit for its R24 440 px line, or
  record that it keeps Frosted's.

## Sleep

Sleep is unchanged. The `sleep` preview runs `renderSleepScene`'s own
composition over Stage's Monochrome plane:

1. `frostSleepBackdrop`;
2. `renderSleepUi`.

The test proves this composition equals `renderSleepScene` over Frosted Home.
Like `renderSleepScene` today, it renders Home without a cover.

## Evidence

- `python3 tools/stage-home-preview.py` writes
  `artifacts/home-stage/<state>.png` (four tones) and `<state>-mono.png`
  (Monochrome), plus the candidate manifest
  `artifacts/home-stage-review.json`. Its `review` stays empty until a person
  records one; the tool then refuses to overwrite it. The host test requires
  an empty review, so recording one is a deliberate change to that test too.
- `tests/stage_home_test.py` (in `tools/check.py`) covers:
  - the generator `--check`, which regenerates the backdrop and compares
    bytes;
  - the same nine atlas faces, in the same order, as Frosted Home;
  - `tests/stage_home_test.cpp`: default identity over 378 Frosted renders
    (189 with the default style), every state in every plane, `renderHome`
    choosing by layout for the nine Home states (Frosted ignores a
    backdrop), the untouched backdrop, dock geometry, the exact halo,
    focus-move windows, cover, and Sleep composition and null frames;
  - spec geometry re-derived from the spec's numbers and the atlas's glyph
    ink, not the renderer's placement. Every text's inked rows sit exactly on
    its spec baseline, and the text is centred or starts where the spec puts
    it. That covers:
    - the header;
    - the title lines;
    - the progress line;
    - the key cap words;
    - the card hint;
    - the missing-cover tile;
    - the dock labels;
    - the no-card and no-book words;
    - the rail labels.

    The test also checks the bar, key cap, slots, rail caps and triangles;
  - the candidate hashes and PNGs (exactly IHDR, IDAT and IEND, with valid
    CRCs), and that the manifest is not the reviewed UI authority.
