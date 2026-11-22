# Newt X3 — approved Frosted Home B

**Approved for implementation.** Owner selected “Squarer FOR SURE … This UI is wonderful and what we should ship.” This supersedes the earlier comparison-open notes. Small wordmark, square card treatment and corrected metadata alignment are selected together. The appearance is frozen; firmware integration belongs to the separate Newt coordinator.

## Canonical assets and reproduction

`screens/newt-frosted-cards-square.png` is the approved 528×792 portrait raster; SHA-256 `54e14ba8918c78fb679704289cf71fe776b211184db8e5191419070c71f4457c`. Its existing Library identity is `libfile_0134c1de11948191ab5054fe0cb6169a` (`/newt-frosted-cards-square.png`). `selected-scene.json` contains the exact ordered drawing operations. Its approval metadata is updated, while every drawing operation is unchanged. `screens/newt-frosted-cards-square.svg` is editable source with relative asset/font dependencies; PNG is the rendering authority. `freeze-manifest.json` records asset checksums and selection provenance.

`assets/haze-cover-gray.png` is the approved static background; `source-cover.png` is its generator input. `fixture-cover.png` is the original fixture cover. `newt-brand-reference.png` is the unchanged owner-approved brand source. Inter regular/bold fonts and notices are included. Lexend Deca and `lexenddeca_14_regular.h` (CrossInk's generated header, reduced to its Lexend Deca glyphs; see docs/provenance.md, "Typography") preserve reader preference; no reader typography redesign is part of this Home handoff.

Run `python3 rebuild.py` with existing Python and macOS Swift. It checks all frozen inputs, renders into a temporary directory, compares every output pixel with the canonical PNG and checks text/metadata bounds. It does not overwrite the approved image. `render.swift` is a host reference; firmware must implement the composition using its own renderer and resource budget.

## Device and drawing contract

Official FreeInk-supported X3 dimensions are 792×528 native, presented here as **528×792 portrait**. All coordinates below use top-left origin, pixels; text y-values are **baselines**, not bounding-box tops. Map these coordinates through the firmware's rotation convention once. Home remains portrait; the owner's established book-only landscape CCW preference remains separate.

Device scalar grays are exactly **0, 85, 170, 255** (black through white). Text/focus are black; card fills are opaque white; card outlines are85. Do not apply the background dither to foreground text. The host compositor rounds final scalar samples to the nearest of those four levels, including font/curve antialiasing. SDK support does not establish working Newt driver gray waveforms.

The full-screen background remains continuous behind the header: Gaussian sigma12px, radius36px, edge clamp;16% white veil; four-level serpentine Floyd–Steinberg. `tones.py` plus the source asset specify the deterministic method; use the provided precomputed background for this fixture. No header strip, divider, extra white logo block or new illustration. Do not recompute or animate the texture for clock/focus updates.

## Geometry and typography

All Home text uses included Inter: `Inter18pt-Regular` / `Inter18pt-Bold` are the actual PostScript font identities. Sizes below are pixels at device resolution, not physical points. Match metrics rather than interpreting the “18pt” name as the requested size.

| Element | Exact drawing contract |
|---|---|
| Small wordmark | x28,y12,w86,h28; crop `[200,951,628,210]` from the1024×1536 source; high-quality scaling, multiply over background. Preserve artwork. |
| Clock | `10:24 AM`, regular24, center anchor x264, baseline31. |
| Status | `Offline · 84%`, regular21, right anchor x500, baseline31. |
| Book card | Opaque white rect `(28,64,472,327)`, radius4; outline `(28.5,64.5,471,326)`, radius4,1px gray85. |
| Book title | `The Secret`: bold36 at `(50,129)`; `Garden`: bold40 at `(50,173)`. No author line. |
| Cover | `(332,87,148,222)`, nearest-neighbor reference scaling. |
| Metadata | `Chapter 4 · 32% read`, bold24 at `(50,244)`, maximum width254. |
| Progress | Outer rect `(50,267,254,4)`, radius1,1px black outline; fill `(50,267,81,4)`, radius1, black.32% is whole-book progress. |
| Resume prompt | `Press Read to carry on.`, regular24 at `(50,357)`. |
| Menu card | White rect `(28,417,472,280)`, radius4; outline `(28.5,417.5,471,279)`, radius4,1px gray85. |
| Menu labels | x73; `Morning Page` bold32 baseline462, `Books` regular32 baseline549, `Apps` regular32 baseline603, `Your X3` regular32 baseline657. |
| Supporting copy | `Book, weather & what’s next`, regular25 at `(73,501)`; `(Settings)`, regular24 at `(220,656)`. |
| Focus | Black triangle vertices `(49,442),(60,451),(49,460)`; black3px underline `(73,474)` to `(479,474)`, round caps. Sole focused row: Morning Page. |
| Physical hints | White footer `(0,742,528,50)`; four black2px outlined94×42 boxes at y749,x48/154/284/390,radius7. Labels Read/Select/Up/Down, regular28 centered x95/201/331/437, baseline779. |

The half-pixel outline inset describes the host stroke centerline; an integer firmware renderer should match its **resulting one-pixel raster**, not widen it to2px. Footer hints deliberately retain their approved radius7 and black2px outline; square treatment applies to the two main cards.

## Runtime behavior and acceptance

Read resumes the current book. Up/Down move menu focus; Select enters the focused route. `Your X3` leads to settings/device details. Match the strong triangle + bold label + underline pattern when moving focus; never rely solely on gray or touch. Physical bindings remain configurable; hint labels must reflect actual bindings rather than assumed pin order. Morning Page is a fixture app concept, not evidence of an installed app or live weather/events. Keep unavailable routes honest in implementation. App-install and permission flows remain outside this Home freeze; normal installation stays queued Wi-Fi via Hub, SD hidden, permission copy plain language.

Clock, battery, chapter and progress in this raster are fictional fixtures. Runtime values must come from supported sources. The clock is minute-format; no seconds or gratuitous redraw. Offline reading must work without network. Longer titles/status/metadata must be measured within their assigned columns and wrapped/shortened deliberately; never invade the cover. Do not introduce silent font substitution or an unsolicited visual redesign to accommodate data.

Acceptance checks for the coordinator:

1. Render this exact fixture at528×792 and compare against the canonical PNG. Require four scalar levels, zero clipping/overlap, correct baselines/anchors and unchanged card/header/footer geometry.
2. Metadata starts x50 and remains within x304. Reference typographic width245.80078125px, right295.80078125; actual dark glyph bounds x51–293. Cover starts x332:28px gap from column boundary. Progress fill remains81px at32%.
3. One clear black focus indicator; every action works with physical buttons. Resume, all four menu destinations and configured labels agree. No touch-only affordance.
4. Inspect the native X3 for readable Inter, gray85 one-pixel borders, four-level/AA mapping and visible focus. The owner approved the host appearance, not a measured hardware waveform. Any necessary contrast deviation should be surfaced against this frozen reference.
5. Restore old focused regions cleanly from the opaque panel; keep the background static. Validate dirty-region/full-refresh strategy, stale glyphs and ghosting on hardware. No refresh timing or partial-gray capability is promised by these host sources.

Prior host pixel inspection and independent review passed. `host-verification.json` retains that evidence; its A/B comparison notes are historical. This package changes selection metadata/documentation only. No firmware, existing Newt repository/service, publication or deployment was performed.
