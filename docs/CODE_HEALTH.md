# Code-health ratchet

The gate protects a few useful maintainability and resource invariants. It runs
read-only: no dependency acquisition, firmware compilation, device access or
baseline update. The existing host tests, allocation-failure tests, generated
asset checks, source pins and linked NVS preservation gate remain required.
Full review workflow passes still judge function complexity, duplicated decisions and
whether a proposed abstraction reduces the concepts a maintainer must hold.
No regex complexity score or repository-wide duplication percentage is enforced.

## Local commands

```sh
python3 tools/check.py
python3 tools/code_health.py
python3 tools/firmware-build.py --clean --health-report
python3 tools/code_health.py --variant regional-dev-usb
```

There is one firmware build: the regional gray SDK copy plus the DEV USB
bridge, keyed `regional-dev-usb` in data and paths (see "One firmware build"
below). `--dev-usb` and `--regional-home` are retired and fail. The host gate
runs both ratchet fixture suites and the structural gate. Native changes
require the clean resource build. The last command verifies an already emitted
receipt; it does not build. Plain incremental compilation remains available but
supplies no new resource acceptance evidence. The pinned platform edits its
framework packages, which are identity inputs, during every build. A
per-checkout toolchain lock (`tools/native_toolchain.py`) therefore makes
PlatformIO runs, each with its build's report, and identity captures take
turns. A health build refuses to capture its identity beside a
build-script backup that an unfinished run (compiledb or a failed build) left;
a plain `python3 tools/firmware-build.py --clean` restores it. A shared
toolchain view (`tools/toolchain-share.py`, docs/toolchain-proposal.md) keeps
every identity input at the same project-local path with the same bytes, so
receipts and ownership rules are unchanged. A health build refuses a view
whose files' metadata (inode, mode, modification time) no longer matches its
pinned, read-only store entry; `tools/toolchain-share.py verify` rehashes it.
There is no new CI service or tool dependency.

## Authored code and boundaries

Git discovers tracked and nonignored untracked C/C++ sources/headers, Python and
Swift under `tools` and `tests`. Tracked deletions pass. Authored symlinks are
rejected so moving source behind an alias cannot remove it from the policy. The
versioned policy uses 500 lines for tools and 700 for tests. Existing oversized
files have individual path ceilings; a new offender cannot replace a removed
file or spend another file's reduction. Line count is a review signal, not a
request to squeeze formatting or scatter helpers. Extract a cohesive owner only
when it makes the design simpler.

The Rust firmware's own structure is checked by `rust-health/rust_health.py`.
Natives (`extern` ABIs, `#[link]`, exports) belong in Salamander
(`rust/salamander`); files outside it keep counted exceptions that only shrink.
The gate also runs the firmware config check and each tracked generator
(built-in app, reader fixture, Stage backdrop, reader atlas) in `--check` mode.
The C++ Newt and its include-ownership checks were deleted in NEWT-90; they
live only in git history.

The pinned vendor SDK, build/cache directories, artifacts and assets are outside
authored-code metrics. Their compiled RAM/flash costs remain included; source,
generated assets and vendor inputs bind the resource receipt. There is no
blanket no-allocation rule: the bounded reader and Expat have intentional
allocation lifecycles already exercised by the host gate.

## Resource evidence

The build has `ram_bytes`, `flash_bytes`, `wifi_lent_bytes` and selected
`source::compiler-signature` stack ceilings in `code-health-baseline.json`.
Each key names the source path under `rust/` that the compiler reports.
`--health-report` requires a clean full Rust archive and native link, captures
input identity before compilation, runs the NVS, Wi-Fi and next-boot linked guards, then emits
`artifacts/code-health-regional-dev-usb.json` and compares it with those ceilings.
The baseline is never written by a build, even when a ceiling fails.

The receipt binds authored/generated/vendor inputs, authored and effective
variant config, pinned manifests and actual framework headers/core sources/C3
archives/linker scripts, platform build recipes, compiler/linker backends and outputs.
It also hashes installed PlatformIO/dependency implementations and cached bytecode,
SCons/esptool/install implementations, venv entrypoints/config and the interpreter's
resolved path and binary. The external interpreter fingerprint does not grant
compiler dependency ownership outside the isolated project.
The build invokes its local venv interpreter explicitly with isolated Python
imports, so relocated entrypoint shebangs cannot select another checkout's venv.
Fresh compiler dependency reports must prove that every included input belongs
to those hashed roots, including resolved symlink targets; headers pulled from
tools, tests, root-level files or outside the project fail ownership rather than
silently extending the build. Link inputs must likewise come from covered
packages or dependency-proven local compilation. The canonical build preflight
pins the reviewed source filter, compiler/linker flag set and generated variant
definitions; custom scripts, response files or external search paths require a
separate explicit scope change.
Changed source/config/compiler/artifacts, missing evidence, wrong variants or
incomplete selected stack entries fail. Verification remeasures output values
instead of trusting editable JSON numbers. Selected unbounded dynamic stack
frames fail. Static frames and `dynamic,bounded` estimates describe individual
functions only; they exclude call chains, precompiled-provider stacks, interrupts
and runtime peaks. Successful builds do not prove stack safety.

Flash is the actual app `.bin` length, not only the linked-code summary. The
existing verified X3 app1 carrier is 0x780000 with capacity 0x770000 (7,798,784
bytes). `board_build.partitions` explicitly targets the existing six-entry
layout in `rust/firmware/x3_existing_partitions.csv`. Resource capacity comes from
the emitted `partitions.bin`, with exact entries, MD5 and padding validated;
the historical SDK sample's 1,310,720-byte slot is not this build target.
The flash policy reserves ten percent rounded up to a 4,096-byte sector:
782,336 bytes of reserve, leaving 7,016,448 usable bytes. Both the actual image
length and its reviewed flash budget must fit that usable limit. The reserve is
an admission margin, not extra allowance added to today's measured ceilings.
The gate remeasures the generated table and image; editable report capacities
cannot replace those outputs or authorize a partition change.
The scoped app-image workflow never installs the generated table.
Fresh device identity, security, partition/OTA layout, exact affected-span recovery
and readback must independently pass before a write. A build receipt does not
authorize deployment to a board using the smaller reference table.
SRAM accounts for the shared C3
instruction/data pool using the actual linker map and emitted section addresses,
including alignment/reservation while avoiding alias double counting. It is not
PlatformIO's smaller static-data-only figure or a runtime free-heap measurement.
The lent Wi-Fi range is reported apart as `wifi_lent_bytes`, not in
`ram_bytes` ([Memory policy](#memory-policy)).
Keep reader arenas, framebuffer assertions and parser lifecycle tests. Heap
fragmentation, allocator overhead, task-stack high-water marks and physical
storage/display behavior require separate measured hardware acceptance.

## Memory policy

Owner decision, 2026-10-21: protect the safety margin, not the totals.

1. **Floors protect stability.** A floor is a minimum the device must keep
   while reading: free internal heap, and the matching stack margin. It comes
   from need: the largest allocation the runtime makes plus a stated safety
   margin, verified on the device in the worst case. It never moves without
   specific owner approval. Until that measurement lands, the current numbers
   stay: the requested 65,536 B reading heap floor (not yet met) and the owner's
   scoped 50,468 B whole-boot exception (docs/061), which is never extended.
   No registered stack margin floor exists yet. The deleted C++ Newt used a
   16,384 B loop task (docs/060); Rust uses the owner's 20,480 B choice of 2026-11-14.
   Rust's 5,212 B bench stop and native-inclusive model remain separate device
   acceptance checks; this image transition does not move a floor or task size.
2. **Above the floor is budget.** A feature or correctness fix may grow flash,
   RAM, RTC or a selected stack ceiling only with one measured record
   for the build: before/after, reason and reviewer. No silent creep: a
   flash, RAM or selected stack ceiling edit without its record fails the gate.
   RTC has no gate ceiling yet; its growth is reviewed, and the record's reason
   states RTC before and after.
3. **Ratchet down.** After a verified improvement, tighten the ceilings.
4. **Off costs nothing.** A feature that is off costs the reader nothing by
   default. Memory a feature holds while unused (Wi-Fi static RAM is the known
   case) needs its own explicit owner approval. The record's reviewer does not
   give it; the record quotes it in `owner_approval`.
5. **Unchanged.** Never raise ceilings or move floors merely to pass a gate,
   never expand exclusions or adapters, never remove selected stack
   measurements, never update ceilings automatically.

How the gate (`tools/code_health_flash_policy.py`) enforces this:

- **Current state only.** The baseline holds each ceiling and floor once, with
  its latest record: `source_revision`, `before`, `after`, `reason`,
  `reviewer` and, where the owner approved something, `owner_approval`. The
  record's `after` must equal the current value. Git keeps every older record
  (the C++-era ledger is in history before NEWT-69).
- **Checked against the base commit.** `tools/code_health.py` (in the host
  gate) and `firmware-build.py --health-report` read the baseline at each
  commit this one builds on: its merge bases with `origin/main`, or its first
  parent when it is already on `origin/main` (a main push). Without
  `origin/main` the check stops; run `git fetch origin`. A clean export's root
  commit has no base. Against that base:
  - a ceiling that moved, up or down, needs a new record whose `before` is the
    base value (`null` for an added key) and whose `after` is the new value;
  - a floor that moved also needs `owner_approval` naming the date and what
    the owner approved;
  - a record that changed while its value did not fails, so records cannot be
    rewritten quietly;
  - a limit cannot be removed. Renaming or retiring a selected stack key needs
    its own reviewed tooling change;
  - a base from before records (pre-NEWT-69) accepts only unchanged values.
    A branch built on one must move onto main before it changes a limit.
- **Lent Wi-Fi range.** `.dram0.newt_wifi` holds Wi-Fi's static data, and
  every boot but update mode lends it to the heap before reading allocates
  (docs/084-wifi-memory.md). Reading does not pay for it, so `ram_bytes` is
  the occupied shared SRAM less that range: what stays resident. The range is
  reported as `wifi_lent_bytes` and capped by its own ceiling. A build with a
  range above its ceiling (or with no ceiling) fails. The ELF check
  (`tools/verify-wifi-link.py`) proves only Wi-Fi code uses the range, so
  lending it is safe; that check, not the ratchet, keeps other data out of it.
- **Floors.** `floors` holds bytes, or `null` for not yet derived. The gate
  never measures floors; they are device facts, recorded by hand. Firmware
  reserves, such as the 64 KiB cover `RESERVE` (`rust/firmware/cover_memory.rs`),
  are source constants under source review, not tied to these records.
- **Adoption.** Ordinary checks cannot know which build a record measured, so
  padding is caught at adoption. After the clean build, run
  `python3 tools/code_health.py --variant regional-dev-usb --adopt BASE`
  on the planned merge, with `BASE` the destination branch tip it merges into.
  Every ceiling above its `BASE` value, and every added key, must equal this
  verified build's measurement.
- **Limits.** Records are editable review metadata. Reviewers check the
  measurement and any `owner_approval` claim. The gate has no RTC ceiling and
  no `rtc_bytes` budget field: every receipt measures RTC, but RTC growth does
  not fail the gate. The gate also cannot see memory held while a feature is
  idle when savings elsewhere offset it, nor heap held while idle; review, not
  the gate, catches those. Follow-up: add an `rtc_bytes` ceiling at the
  measured value through one reviewed record.

## One firmware build

Owner decision, 2026-10-26: there is one firmware build, today's regional
DEV build (the tracked regional gray SDK patch plus the DEV USB bridge). The
authored `platformio-rust-app.ini` is that build; `tools/firmware-build.py` adds its
plain 12-hex Git build ID in the generated copy under
`.build/regional-dev-usb/`. Its key `regional-dev-usb` stays in the baseline,
receipts, `.pio`/`.build` paths and SDK routes. A build without the DEV bridge
returns only when OTA or a release needs one. The retired variants' last
ceilings are in git history.

[Measuring the need-based floor](memory-floor-measurement.md) is the device plan.

## Reviewed baseline changes

The sections below are the review log from before NEWT-69. The flash and
protected record numbers they cite are in `code-health-baseline.json` at commits
before NEWT-69; the live baseline keeps only each limit's latest record.

The initial provenance was reviewed M1 source
`a2e75e273431605ec4b09b41048835a7a6763762`, with official FreeInk
`cd6f5b831c8fad899723b6d406586f00c575f021`. Adoption provenance is final reviewed
UX head `cbd692b223956f1371046924655d612168494361`, whose compiled inputs match
code commit `16cd3bc140e9410346726cc5d90fffba36639b4c`. The isolated branch was
rebased without conflicts or firmware edits. Per-variant input hashes pin the
remeasured source snapshot. Blocked SD work is not integrated or covered; any
later integration requires its own reviewed measurements. This evidence
authorizes no installation.

Initial clean-build ceilings retained here for comparison (bytes):

| Variant | Shared SRAM occupancy | App image |
| --- | ---: | ---: |
| default | 132,400 | 1,230,752 |
| dev-usb | 133,040 | 1,236,112 |
| regional | 132,432 | 1,232,608 |
| regional-dev-usb | 133,056 | 1,237,984 |

The shared pool is 321,296 bytes and the built app slot is 1,310,720 bytes.
RTC sections occupy 172 bytes separately. Selected per-function ceilings are
192 bytes for setup, 208 for loop, 80 each for package hashing/loading and
736 for the native package slot reader. They do not add up to a proven task
stack bound. The known oversized-file allowance is only `firmware/main.cpp`
at 545 lines before adoption. Resource ceilings use observed values, so justified
feature growth requires an explicit resource review rather than automatic slack.

The adoption change adds the already reviewed `UiRefreshPolicy`: one header,
one global instance, and two paint hooks bound monochrome fast refreshes to nine
between full refreshes. App-image ceilings increase by 48 bytes except dev-usb,
which increases by 32 bytes; emitted image alignment differs by variant.
SRAM, RTC and selected stack ceilings stay unchanged. The new flash ceilings
are 1,230,800 (default), 1,236,144 (dev-usb), 1,232,656 (regional), and 1,238,032
(regional-dev-usb), leaving 79,920 / 74,576 / 78,064 / 72,688 bytes in the
unchanged 1,310,720-byte app slot. This accepts the measured cost of the reviewed
refresh bound, without arbitrary percentage headroom or new exclusions.

USB builds also encode the existing development identifier. Clean-checkout
USB images measure 1,236,144 and 1,238,016 bytes; separately measured builds with
the existing `-dirty` identifier measure 1,236,144 and 1,238,032 bytes. The
regional USB image grows 16 bytes while the regular USB image size is unchanged.
The ceilings cover those measured normal development builds, as the initial
ceilings did. Receipts bind
the actual effective configuration and compiled identifier; the build does not
normalize or conceal dirty state to obtain a passing metric.

The sole file allowance changes explicitly from 545 to 549 lines for the four
reviewed integration lines in `firmware/main.cpp`; `UiRefreshPolicy` itself is
already isolated in an 18-line shared header. Moving these necessary hooks to
an extra wrapper solely to reduce the counter would obscure ownership. No
other file allowance, dependency adapter, generated exclusion or policy
threshold changes. Future growth still fails until separately justified.

An exception changes only the specific path, adapter or resource ceiling. In the
change description, record the failing measurement, why simpler ownership or
less resource use is unsuitable, the exact new limit, remaining headroom and
the review decision. Exclusion/threshold changes require matching code and
baseline policy edits and the same review. Do not remove selected functions,
skip behavior/fault tests or raise ceilings merely to obtain a passing gate.

Explicit flash growth for a feature or correctness fix can be reviewed and
accepted below the usable partition limit. Before NEWT-69 each change appended a
hash-pinned history record; today it replaces the limit's one record
("Memory policy" above).

For the current runtime, the initial anchor is the already-reviewed baseline
snapshot `042929a42e20c69feb11042ffd5d2d40c4bc7471`, including its adopted glyph
costs. Its canonical anchor SHA-256 is
`89bc5851751e1be1ae2e808a87d6ee080ec6babdcd626ec06878fe21bc56b569`.
Flash ceilings remain 1,244,992 / 1,260,224 / 1,248,784 / 1,264,064 bytes for
default / DEV USB / regional / regional DEV USB. All current RAM and selected
stack ceilings remain unchanged. The adaptation records no fabricated historical
flash updates and supplies no new native or device evidence. The policy helper
joins the current native procedure/source-cache identity; existing receipt,
runtime and dependency validation remain intact.


The first flash update records all four clean native measurements at
`243a9643508845cda742abd28957add42d6346bf`. Compared with runtime `6911ddb`,
default and DEV USB actual image sizes are unchanged; regional and regional
DEV USB each grow 16 bytes. The latter already fits its existing ceiling, so
only the regional ceiling changes from 1,248,784 to **1,248,800 bytes**. The
complete measured ceiling map is 1,244,992 / 1,260,224 / 1,248,800 / 1,264,064.
The native-relevant release integration includes the frozen Commitizen
`b86ddeed54f1038a348bc17e36bdf2d6226f4798` About SemVer label and the reviewed
XML allocation-header alignment helper. These aggregate measurements do not
isolate the About label as the cause of the 16-byte regional growth.

The usable partition remains 7,016,448 bytes after its 782,336-byte reserve;
regional retains 5,767,648 bytes of usable partition headroom and zero bytes of
ratchet headroom. RAM, RTC and every selected stack measurement match the prior
runtime across all four variants. All protected budgets, history pins, adapters
and exclusions remain unchanged. The audit binds each original report and its
four emitted artifacts; it does not turn the original regional failure into a
historical pass or claim a fresh gate for the budget-edit commit. Current-source
admission still requires fresh canonical host and native verification. No new
device or optical evidence is supplied.

These committed records are editable, trusted review metadata, not a signature
or cryptographic approval. Reviewers must verify the supplied source, receipts,
hashes and cost attribution; schema validation cannot establish that a named
reviewer approved them. Current admission still independently remeasures the
hash-bound build outputs. Builds and failed checks never update the baseline.
The source pin and digest protect the initial snapshot against JSON-only edits;
they do not authenticate reviewers or defend against an unauthorized rewrite of
the validator or its compiled history pins. Those source changes still require
repository review.

After an improvement, run the full host gate and clean native measurements,
lower the relevant ceilings/remove retired file allowances, and review that
diff with its receipt. Improvements pass against the existing baseline without
rewriting it; until tightening, removed debt can return within the old ceiling.
The baseline is an upper limit on known debt, never an endorsement of that debt.

## Local implementation verification

The original full host gate passed, including ASan/UBSan and existing
storage/render/fault tests. All 36 focused ratchet fixtures passed. Clean native
builds and receipt verification passed for all four variants, with the existing
linked NVS guard.
Four fresh adversarial review rounds ended clean after fixing input-coverage,
include-resolution and literal-parsing defects. The strict maintainability
review ended clean after consolidating artifact selection and measurement.

Adoption reran the host gate and all four clean native resource builds on the
final UX source. Resource receipts now also bind object/dependency reports and
linker inputs, with 48 focused fixtures plus the build-scope mutation matrix.
Adoption reviews fixed seven checker defects: unbound compiled dependencies,
local header shadows, unrecognized include directives, initial UTF-8 BOMs,
multiline comments breaking directive parsing, the missing project-root
include search candidate, and GCC whitespace continuation handling. Dependency
ownership now uses the same canonical
input set as pre/post hashes; include resolution selects
the first actual local candidate before applying an external allowlist. Local
imports must be scanned C++ files, so ignored or unsupported headers fail.
The scanner recognizes literal `#include` and `%:include` spellings and rejects
computed headers and unsupported `include_next`/`import` directives.
It normalizes initial UTF-8 BOMs before lexing, matching compiler treatment.
Multiline block comments become whitespace while original offsets preserve
physical diagnostic lines. Search order is quoted sibling, project include
directory, then project root, matching the approved native configuration.
Whitespace between a continuation backslash and newline is spliced as GCC
does. Directive-marker detection keeps physical newlines while directive value
extraction collapses block comments, retaining compiler search behavior and
diagnostic locations without treating comments as executable text.
Seven fresh adoption review rounds ended clean after those seven findings were
fixed. The strict maintainability review also ended clean. The closure is
canonical compiled-input ownership plus compiler-consistent preprocessing and
ordered local include resolution; no firmware behavior or budget was changed
to accommodate checker fixes.

No firmware source was changed by this implementation. These checks do not
establish runtime stack peaks or device behavior. This local full-review phase
includes no remote review, publication, installation or flashing.

## Reviewed M1 physical-regression resources

The current revision adds committed Pocket Pause preferences and selection, a
returning sleep-entry boundary with a checksummed rejection tag, and an
experimental UC8279 binary cursor island. Four clean native builds pass the
linked preservation and sleep boundary gates; the existing resource ceilings
correctly reject this measured feature growth. The proposed exact limits are:

| Variant | Shared SRAM (growth) | App image (growth) | SRAM headroom | App headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 132,512 (+112) | 1,236,512 (+5,712) | 188,784 | 74,208 |
| dev-usb | 133,152 (+112) | 1,241,872 (+5,728) | 188,144 | 68,848 |
| regional | 133,024 (+592) | 1,239,184 (+6,528) | 188,272 | 71,536 |
| regional-dev-usb | 133,648 (+592) | 1,244,560 (+6,528) | 187,648 | 66,160 |

The native PocketPause instance occupies 112 bytes. The regional feature adds
a bounded 480-byte island checkpoint; it does not retain a whole framebuffer.
The measured shared-pool totals include all other code/data and linker alignment
changes, rather than summing isolated object sizes. Removing these states would
remove the requested committed prompt choices or the binary-baseline proof.
Image growth includes the shared readable app/settings renderer, conservative
two-slot preference store, return-capable sleep lifecycle and native driver path.

Paragraph wrapping now uses one shared fixed-font engine, and preference
verification reuses slot scratch rather than an extra 20-byte local buffer.
The compiler still measures the loop at 256 bytes after those simplifications;
no indirect-call trick is used to hide that frame.

The loop's compiler frame grows from 208 to 256 bytes while routing the bounded
app state transition and explicit preference save. Preserve this measured frame
rather than forcing out-of-line calls solely to hide cost from the selected
measurement. Setup falls from 192 to 176 bytes; the proposal tightens that limit
to 176. Selected package frames remain 80/80/736. RTC occupancy is 160 bytes
(previously 172), including the new 68-byte retained record. These are individual
compiler frames and static occupancy, not runtime task-stack or heap proof.

Review decision: accepted after three adversarial rounds (latest clean), two
strict maintainability rounds (latest clean), and independent verification of
all four exact identities, artifact hashes, dependency/link ownership and
remeasured outputs. Adopt only the table's exact SRAM/image limits and loop
256/setup176; package frames remain 80/80/736. Tighten the existing main.cpp
allowance from 549 to its current 544 lines. No exclusion, selected function,
adapter, scope or toolchain pin changes. Final clean builds bind compiled source
`456fcd68735ae8cb990204f7c2f7271ed43c57b8` and pass all four resource/link
gates against these exact ceilings. App lengths match the reviewed dirty
measurements; final image identities are recorded in
[the build manifest](../artifacts/m1-ux-456fcd6-builds.json). Ordinary Home pixels remain
unchanged; experimental optical quietness and physical sleep resume remain
unverified.

## Reviewed book wake and frosted sleep resources

The owner accepted Home/Pocket improvements on installed `456fcd` but reported
reader wake still returning Home. This candidate retains an actual book checkpoint
in the existing 68-byte RTC layout, restores it after durable-state loading, and
opens its exact page. All other destinations wake Home. A read-only dev diagnostic
snapshot separates boot eligibility, requested position, reopening and later input.
Fresh sleep composition smooths the destination and overlays the unchanged approved
artwork through a transparent silhouette with a 3px white halo.

Four clean native measurements, independently revalidated for exact current input
identity, compiled dependencies, artifact hashes and linker ownership, support:

| Variant | Shared SRAM (growth) | App image (growth) | SRAM headroom | App headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 132,512 (+0) | 1,242,384 (+5,872) | 188,784 | 68,336 |
| dev-usb | 133,216 (+64) | 1,248,480 (+6,608) | 188,080 | 62,240 |
| regional | 133,024 (+0) | 1,245,056 (+5,872) | 188,272 | 65,664 |
| regional-dev-usb | 133,728 (+80) | 1,251,168 (+6,608) | 187,568 | 59,552 |

The SRAM growth is dev-only immutable boot evidence plus linker alignment; normal
firmware adds no persistent SRAM. RTC total remains 160 bytes. Sleep reuses the
existing logical/native frames and a 792-byte column array for a 25x25 rolling box
filter. Its compiler frame measures 912 bytes, which is added as a sixth selected
measurement. That individual frame excludes call-chain/runtime peaks. Loop improves
from 256 to 224 bytes in all variants. Setup improves to 160 in ordinary variants
and remains 176 with USB. Existing package frames remain 80/80/736, and main stays
544 lines. No file ceiling, exclusion, adapter, dependency or toolchain pin expands.

The exact mask originally required 15,808 bytes. Strict review required its simpler
sequential representation: 2,195 alternating uint16 runs plus the initial bit total
4,391 bytes, saving 11,417 bytes. Decode needs only a counter/toggle in the existing
raster loop. Source logo bits, decoded mask SHA and all three grayscale previews
remain identical. Remaining image growth buys actual checkpoint validation, fresh
composition, canonical settings branding and bounded read-only diagnostics.

Review decision: accept only the table's measured SRAM/image costs, tighten loop
to 224 and per-variant setup as above, add blur912 and preserve all existing package
measurements. Fresh adversarial review ended clean after closing mono quantization
and geometry failure cases. Strict maintainability round3 ended clean after mask
compression, with independent four-receipt validation. Final clean source-frozen
builds must bind their exact images before flash handoff. At that review the older
installed image failed sleep/wake; physical acceptance of this candidate was pending.

Final source-frozen builds bind compiled source
`f6c6792bf8e450f4ea8f3f77d1ba05db50f31fae`, pass all four resource/preservation
gates, and preserve the reviewed ordinary/regional input identities. The clean
regional-dev image is 16 bytes smaller than the dirty reviewed measurement; its
ceiling tightens accordingly to 1,251,168. Exact identities and archive locations
are in [the book-wake build manifest](../artifacts/m1-book-wake-builds.json).

The parent subsequently authorized that exact regional development image, and the
owner reported the independent physical sleep/wake and requested UI checklist
passing. A later battery-only sleep/wake returned Home; resume is intermittent and
its cause is unobserved. This remains the accepted UI baseline. Follow-up battery-region
and Home progress source changes were first measured against these prior exact
limits. Their explicitly reviewed flash-only exception is documented below;
SRAM, selected frames, exclusions and structural limits remain unchanged.
The diagnostic client correction is host-only. USB attachment behavior remains
unproven and all additional hardware access is on hold.

## Reviewed Home status and progress costs

Compiled source `2c0a86d04102119ab5e9a10e4f4748f5a6f2bbee` passes the complete ASan/UBSan host gate and all
four clean native resource and linked-preservation gates. Previous measurements
exceeded installed-baseline flash ceilings; two independent reviews accepted the
necessary flash-only costs. After the cached-RTC correction, fresh independent
resource audits rehashed every input/configuration/artifact, remeasured outputs,
verified linked NVS/sleep guards and RTC layout, and confirmed both DEV identities.
The exact ceilings now tighten two variants by 16 bytes from the previous review.

| Variant | Exact new flash ceiling | Increase from installed baseline | App-slot headroom |
| --- | ---: | ---: | ---: |
| default | 1,242,864 | 480 | 67,856 |
| dev-usb | 1,248,976 | 496 | 61,744 |
| regional | 1,246,576 | 1,520 | 64,144 |
| regional-dev-usb | 1,252,688 | 1,520 | 58,032 |

The necessary cost buys validated saved-progress projection, confirmed-absence
reset, protection against corrupt/unmapped record replacement, and deferred idle
Home battery painting. Every ordinary Books entry reloads durable state so failed
exact RTC recovery cannot authorize a replaced cached bookmark. Regional code
activates the bounded status region and retains cursor proof only when both full
uploads match the current on-panel checkpoint. The cohesive owners and real-route
regressions were separately reviewed; removing checks or trimming accepted visual
assets would drop required behavior. No assets, framebuffers or heap allocations
are added; fixture chapter metadata adds eight bytes.

Review decision: accept only these measured flash ceilings with no extra slack.
SRAM remains 132,512/133,216/133,024/133,728 bytes; RTC remains 160 bytes with the
68-byte checkpoint. All six selected stack limits, main's 544-line limit, dependency
pins, exclusions and adapters are unchanged. Full reports remain in the frozen
archives; the manifest summarizes the six selected frames. Read-only resource
verification must pass under the exact updated limits. Policy/receipt changes
alter no compiled inputs or frozen DEV IDs.
[Exact manifest](../artifacts/m1-status-progress-builds.json).

The images are uninstalled. Repeat USB-connected success and battery-only failure
remain a physical resume blocker; status waveform behavior is unmeasured. This
resource decision authorizes no hardware.

The Home status/progress measurements above are historical. The pending recovery
and owner-approved three-route Home revision below supersedes that proposal.

## Reviewed pending recovery and three-route Home costs

Compiled source `929082488fb6610b15feb25570fb60e5435f720f` combines the immutable
pending reader checkpoint with the owner-approved Morning Page removal. A pending
checkpoint must remain independent of mutable durable storage and its write
permission; temporary heap/parser failure, Settings, failed live exits and another
Home sleep retain the actual newer anchor/geometry/statistics. The strict typed
rejection path still fails closed. RTC v6 remains 68 bytes and durable progress v1.

Home now has three real routes. Its dead 128-byte supporting-copy cache and unused
label/focus raster data are removed while the approved shell and font atlas remain.
The native recovery owner occupies 48 bytes in initialized static storage. The
combined measured shared SRAM therefore falls by 80 bytes in every variant.
No reader workspace, framebuffers, heap allocations, SDK pins or adapters are added.

| Variant | Exact SRAM ceiling | SRAM change | Exact flash ceiling | Flash change | SRAM headroom | App headroom |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| default | 132,432 | -80 | 1,234,064 | -8,800 | 188,864 | 76,656 |
| dev-usb | 133,136 | -80 | 1,240,144 | -8,832 | 188,160 | 70,576 |
| regional | 132,944 | -80 | 1,237,872 | -8,704 | 188,352 | 72,848 |
| regional-dev-usb | 133,648 | -80 | 1,243,984 | -8,704 | 187,648 | 66,736 |

Changes compare with the preceding reviewed Home status/progress policy. The
regional development image is 7,184 bytes smaller than installed f6. Images use
302/303/303/304 app1 sectors, respectively. No image is installed or authorized
for hardware; the intermediate larger C4 checkpoint is superseded.

Review decision: two independent combined source/resource audits are CLEAN.
Both rehashed actual inputs/configuration/toolchain/compiler/interpreter and all
303 output artifacts per variant, verified frozen/live correspondence, remeasured
SRAM/flash/selected frames, and checked NVS/sleep guards, RTC and DEV identities.
Adopt only their exact limits below. Final policy fixtures, structural validation
and all four source-bound resource checks pass after adoption.

The only required ceiling increase is ordinary setup's measured frame from 160 to
176 bytes while restoring the bounded pending owner. It is accepted explicitly;
no out-of-line indirection hides the compiler frame. Tighten ordinary loop from
224 to 208; USB loop/setup stay 224/176. Preserve all four other selected frames
736/80/80/912, including the frost measurement. Tighten main's allowance from
544 to 542 lines. Adopt only exact measured SRAM/flash limits with no slack.
RTC total stays 160 bytes, including the retained 68-byte record; reader allocation
stays 69,968 bytes natively and 70,088 bytes in the host gate. Individual frames and static occupancy do not measure runtime
peaks. No policy threshold, generated exclusion, adapter, pin or source scope expands.

Full receipts remain in frozen local archives; the versioned manifest summarizes
all six selected frames, image hashes, input/toolchain/interpreter bindings, DEV IDs,
guards and proposed sectors. The host gate covers the combined source; all 158
individual authored hashes and nine complete source/assets/apps/patches/vendor
Git trees match its isolated gated worktree. Receipt/policy-only changes preserve
compiled input identity and exact DEV IDs. The [manifest](../artifacts/m1-status-progress-builds.json)
and [handoff](m1-ux-handoff.md) contain the final proposal and physical blockers.
Battery-only resume remains FAILED/intermittent with cause unobserved. This cost
acceptance authorizes no USB, reset, input, flash, merge or public publication.

## Reviewed rejected-sleep and native identity corrections

Compiled source `46765221157421cd076a5deff01fca695bf1f534` closes the next three
review findings. When wake-source arming rejects sleep, a verified live reader
resumes ownership and retires the old pending snapshot. Unavailable readers
retain recovery. Actual firmware-branch regressions in both orientations prove
that later successful or failed saves cannot replay the pre-sleep checkpoint;
restoring the old branch reproduces the rollback.

Native Pocket Pause privileges require exact supported canonical fields, ordered
cards and actions. The existing generator derives the constant from the same
validated source as the unchanged 615-byte portable package. Valid replacements
with independently recomputed digests use generic behavior. The comparison uses
bounded fields, not struct padding, a repeated SHA operation, runtime decoding,
heap allocation or a second hand-authored definition. Actual ELF measurement
confirms the canonical semantic object occupies 607 read-only bytes.

Build identity rejects directory symlinks at input roots and during traversal,
including aliases retargeted between existing owned directories. Legal file
links remain bound to contents and ownership. Inventory found no directory
links in approved roots and six supported compiler file links. Negative controls
fail against old code; the resource fixture suite now has 37 passing tests.

| Variant | Exact flash ceiling | Growth from preceding policy | App headroom | SRAM ceiling (unchanged) |
| --- | ---: | ---: | ---: | ---: |
| default | 1,234,880 | 816 | 75,840 | 132,432 |
| dev-usb | 1,240,960 | 816 | 69,760 | 133,136 |
| regional | 1,238,688 | 816 | 72,032 | 132,944 |
| regional-dev-usb | 1,244,800 | 816 | 65,920 | 133,648 |

Two independent actual resource audits are CLEAN. Both bind all four input,
configuration, compiler/toolchain/interpreter and 303-artifact sets to the frozen
images; independently remeasure costs and verify guards, pins, RTC and DEV labels.
Adopt only the exact measured flash ceilings above. All 21 structural-policy and
37 resource fixtures, structural validation and four actual receipt checks pass
after adoption.

The exact flash-only increase buys canonical read-only identity and bounded
comparison plus the canceled-sleep ownership handoff. Dropping identity fields,
allowing reordered prompt slots or retaining stale ownership would reintroduce
the reviewed defects. The canonical data is generated once rather than decoded
or hashed on each UI operation. No package payload, schema, SDK or hardware path
changes. The regional development image remains 6,368 bytes smaller than installed
f6 and uses 304 sectors `[0x780000,0x8b0000)`.

All six selected frame limits stay unchanged: normal loop/setup 208/176, USB
224/176 and package slot/hash/load/frost 736/80/80/912 bytes. Main remains 542 lines.
SRAM, RTC160/retained68 and reader allocation native69,968/host70,088 stay unchanged.
No exclusions, adapters, pins, structural thresholds or arbitrary headroom expand.
Full source-frozen receipts retain all actual output measurements; the manifest
summarizes the same six selected frames. Receipt-only adoption changes no compiled
source tree or DEV identifier. [Exact manifest](../artifacts/m1-status-progress-builds.json).

The owner's 17:45 f6 A/B shows correct CCW reading sleep in both power states,
then USB wakes to text and battery wakes Home. It narrows the observed divergence
to after destination rendering without proving a reset/restore/input cause. The
[RAM-only on-device evidence plan](battery-wake-evidence-plan.md) is proposed,
not implemented. No image is installed; all hardware remains on hold. Actual-book
and SD/Martian work remain separately owned and uncovered by fixture tests.

## Rejected-checkpoint preference authority

Semantic rejection restores acknowledged reader preferences and any Settings
draft after a typed semantic RTC checkpoint rejection. Temporary unavailability
retains the original geometry and draft for retry; exact success retains its
verified geometry. The direct four-line caller decision keeps preference authority
outside the recovery helper and adds no reopen, storage operation, tracker credit
or implicit fallback. Fresh bounded review found the stale draft sibling and
justifies main's exact 542-to-546 line exception. The actual measurements and
independent resource decision below tighten flash ceilings; selected frames,
exclusions, adapters and dependency pins stay unchanged. This correction does not establish the cause
of battery-only wake failure.

## Reviewed semantic settings and executed procedure costs

Compiled source `a1c5df1cb0b5180ec54cb482226ee896ac03cb94` closes semantic checkpoint rejection
across actual reader settings and an already-open Settings draft. Both orientations
exercise heap-blocked pending recovery, first Save rejection and immediate second
Save. Removing only draft rollback reproduces the leak. Temporary unavailability
and exact success retain their original geometry and progress authority.

Procedure receipts bind actual authored build, collection, verification and check
helpers, including applicable tagged/optimized/legacy bytecode and PNG helper
caches. Missing procedures and aliased procedure/cache entries fail closed;
applicable cache content and presence are bound. Native children use the existing
sanitized environment and `-B`; code-health children use `-E -B`.
Canonical entrypoints normalize cache namespaces; direct unsupported prefixes
fail. These bindings grant no additional compiler ownership. Source-unchanged,
valid-cache controls execute altered helpers under old code; the actual verifier
launcher test isolates the inherited-prefix defect. New tests stay within existing
thresholds (Home 698 lines, resource fixtures 695, cache fixtures 175).

| Variant | Exact flash ceiling | Change from C5 | App headroom | SRAM ceiling (unchanged) |
| --- | ---: | ---: | ---: | ---: |
| default | 1,234,848 | -32 | 75,872 | 132,432 |
| dev-usb | 1,240,944 | -16 | 69,776 | 133,136 |
| regional | 1,238,672 | -16 | 72,048 | 132,944 |
| regional-dev-usb | 1,244,784 | -16 | 65,936 | 133,648 |

Two independent actual source/resource audits and two fresh source reviews are
CLEAN. Audits bind all four input/configuration/procedure/cache/compiler/toolchain/
interpreter sets and 303 artifacts per variant, independently remeasure occupancy
and frames, and verify frozen/live equality, guards, pins, RTC and DEV labels.
Adopt only these exact measured flash ceilings. All 21 structural-policy, 39
resource and 7 executed-cache fixtures, structure and four actual receipt checks
pass after adoption.

The sole structural exception is main's exact 542-to-546 line increase for the
four-line reader/draft authority decision. Keeping preference policy in its caller
avoids a coupled recovery-helper API; it adds no reopen, fallback, write or tracker
credit. All six selected frames stay unchanged: normal loop/setup208/176,
USB224/176, package slot/hash/load/frost736/80/80/912. SRAM, retained RTC68/total160,
native reader allocation69,968/host70,088 and canonical read-only definition607
are unchanged. No thresholds, exclusions, adapters, dependency pins or arbitrary
resource headroom expand. Compact summaries preserve the same six measurements;
full reports remain archived. Receipt-only adoption changes no compiled source
tree, procedure/cache bytes or frozen DEV identifier.

The [exact manifest](../artifacts/m1-status-progress-builds.json) and
[handoff](m1-ux-handoff.md) contain image hashes and app1 sector spans. The owner's
17:45 installed-f6 sequence still shows correct CCW reading sleep in both power
states followed by USB text versus battery Home. The cause remains unobserved.
The [RAM-only evidence plan](battery-wake-evidence-plan.md) is proposed and
unimplemented; no new image is installed and all hardware stays on hold. Actual
books/SD/Martian integration and physical acceptance remain separate blockers.

## Reviewed generated-config write boundary

Compiled source `156447570ad867e5f46b0e5bdafbffe6e77e0dee` rejects aliased project/build/variant
parents and symlinked, broken, nonregular or hardlinked generated-config files
before writing. All three generated variants use the same 18-line guard; valid
new and existing unaliased regular files remain supported. Actual CLI fixtures
exercise 54 cases, protected target bytes and rejected native-command dispatch;
four old-code controls reproduce unsafe writes or accepted aliases.

Two independent source reviews and two actual four-variant resource audits are
CLEAN. Fresh full host, four clean native builds, linked NVS/sleep guards and final
21 structural-policy / 39 resource / 7 executed-cache fixtures pass. Procedure
identity reflects the changed builder even though C++ inputs remain unchanged.

| Variant | Exact flash ceiling | Change from C6 | SRAM ceiling (unchanged) |
| --- | ---: | ---: | ---: |
| default | 1,234,848 | 0 | 132,432 |
| dev-usb | 1,240,944 | 0 | 133,136 |
| regional | 1,238,672 | 0 | 132,944 |
| regional-dev-usb | 1,244,784 | 0 | 133,648 |

All six selected frames remain unchanged: normal loop/setup 208/176, USB 224/176,
package slot/hash/load/frost 736/80/80/912. The 546-line allowance for main remains the previously reviewed
four-line settings/draft exception. No resource or structural allowance increases;
compiler ownership, pins, adapters and exclusions stay unchanged. Native reader
allocation remains 69,968 bytes, host 70,088, retained RTC 68 / total 160. Static receipts
cannot establish runtime peaks or physical battery resume acceptance.

## Reviewed direct native-output write boundary

Compiled source `f795dd396a09a56a150d17332972d796de4545cc` validates canonical parents and regular,
unaliased, singly linked files for generated config, SDK provenance, linked guard
and health output receipts. A bounded shared helper uses random exclusive staging
and atomic replacement. SDK provenance preflight precedes SDK destination
replacement; obsolete predictable temporary aliases remain untouched. The helper
and its applicable executable caches are bound as authored procedures without
expanding compiler ownership.

Two independent source reviews and two actual four-variant resource audits are
CLEAN. Fresh full host, actual CLI protected-byte fixtures and calibrated old-code
controls, four clean native builds, linked NVS/sleep guards and final
21 structural-policy / 39 resource / 7 executed-cache fixtures pass. Procedure
identity reflects the changed scripts/helper while C++ inputs remain unchanged.

| Variant | Exact flash ceiling | Change from C7 | SRAM ceiling (unchanged) |
| --- | ---: | ---: | ---: |
| default | 1,234,848 | 0 | 132,432 |
| dev-usb | 1,240,944 | 0 | 133,136 |
| regional | 1,238,672 | 0 | 132,944 |
| regional-dev-usb | 1,244,784 | 0 | 133,648 |

All six selected frames remain unchanged: normal loop/setup 208/176, USB 224/176,
package slot/hash/load/frost 736/80/80/912. The 546-line allowance for main remains the previously reviewed
four-line settings/draft exception. No resource or structural allowance increases;
compiler ownership, pins, adapters and exclusions stay unchanged. Native reader
allocation remains 69,968 bytes, host 70,088, retained RTC 68 / total 160. Static receipts
cannot establish runtime peaks or physical battery resume acceptance.

The next section records the upstream UX-only native namespace measurements.
The SD integration and transactional-reader tables below supersede its reader
resource figures for this branch.

## Reviewed configured native namespace preflight

Compiled source `e270071d0cad2e93d4f696980e13b4ac3faee946` validates canonical selected PlatformIO
build/library-output ancestors and the six declared mutable cache/temp roots
before the first authored child and before native cleaning or building. Default,
development, regional and combined variants share this preflight. Legal read-only
SDK library and compiler/interpreter links remain supported. No dependencies are
installed and no compiler ownership, pins or exclusions expand.

Two independent source reviews and two actual four-variant resource audits are
CLEAN. Fresh full host, actual CLI protected-root fixtures and calibrated old-code
controls, four clean native builds, linked NVS/sleep guards and final
21 structural-policy / 39 resource / 7 executed-cache fixtures pass. Procedure
identity reflects the changed builder while C++ inputs remain unchanged.

| Variant | Exact flash ceiling | Change from C8 | SRAM ceiling (unchanged) |
| --- | ---: | ---: | ---: |
| default | 1,234,848 | 0 | 132,432 |
| dev-usb | 1,240,944 | 0 | 133,136 |
| regional | 1,238,672 | 0 | 132,944 |
| regional-dev-usb | 1,244,784 | 0 | 133,648 |

All six selected frames remain unchanged: normal loop/setup 208/176, USB 224/176,
package slot/hash/load/frost 736/80/80/912. The 546-line allowance for main remains the previously reviewed
four-line settings/draft exception. No resource or structural allowance increases;
compiler ownership, pins, adapters and exclusions stay unchanged. Native reader
allocation remains 69,968 bytes, host 70,088, retained RTC 68 / total 160. Static receipts
cannot establish runtime peaks or physical battery resume acceptance.



## SD reader integration adoption (2026-10-02)

This section supersedes fixture-only resource figures and the deferred-SD status
above. The integration uses UX head `9800a1c8ea4ef0833c631f7b6f1141f883e2d803`;
per-variant input hashes identify the measured combined source. The new reader
adds SD catalog/source ownership, complete-content hashing, deflate/Unicode
layout, separate per-book progress and a 32 KiB aggregate XML allocation cap.
These costs are necessary for the requested reading behavior; removing identity,
allocation admission or pending-checkpoint protection would weaken correctness.

The first combined images exceeded the unchanged 1,310,720-byte partition:
1,346,144 / 1,352,272 / 1,349,952 / 1,356,112 bytes. We did not accept those
images or increase partition capacity. Home tables now use validated 16-bit stamp
sizes and 8-bit indices into the frozen, sorted alphabet. Aligned representations
saved 53,792 actual image bytes in every variant before the final UX settings
fix. That fix and final development identifiers add 48 / 48 / 48 / 48 image bytes; the final table below
includes it and measured development identifiers. Raster data, glyph coverage and
provenance remain unchanged; all 88,209 kerning comparisons and frozen pixel
checks passed. The new exact measured ceilings are:

| Variant | Shared SRAM bytes | App image bytes | Partition headroom |
| --- | ---: | ---: | ---: |
| default | 133,200 | 1,292,400 | 18,320 |
| dev-usb | 133,920 | 1,298,528 | 12,192 |
| regional | 133,696 | 1,296,208 | 14,512 |
| regional-dev-usb | 134,416 | 1,302,368 | 8,352 |

This is explicit feature-cost adoption, with no percentage slack. Flash remains
tight, especially the regional USB variant. Compared with the preceding UX
baseline, SRAM rises by 768 / 784 / 752 / 768 bytes. RTC rises from 160 to 300
bytes because the separate SD filename/full-SHA identity record is 140 bytes;
the source-discriminating SleepRecord itself remains 68 bytes. USB setup's
individual compiler frame rises from 176 to 192 bytes for the integrated source
selection; normal setup stays 176. Loop and all four pre-existing package/sleep
selected limits are preserved. Added selected frames cover SD hashing (1,216),
selection (368), reader open (832), layout (1,216), XML creation (32) and resize
(32) bytes. These estimates exclude call chains and do not establish task-stack
safety; the SDK ZIP locator alone has a 4,240-byte frame.

Main adopts the latest UX's existing 546-line ceiling; the four added lines
restore committed settings after rejecting an invalid RTC checkpoint. The filesystem enumeration adapter
moves from shared source into firmware. Four narrow direct SDK adapter
allowances cover EPUB admission, file BookSource, phased layout and Expat; each
has a single named external header and a tested ownership boundary. No generated
exclusion, structural threshold, SDK pin or partition capacity changes. The
canonical native source filter adds only the SD library/directory files, and
its exact linker flag set adds the reviewed XML creation wrapper. Actual compiler
input ownership and output remeasurement remain enforced. Fresh adversarial and
maintainability review assess this adoption together with the source change.

Host/library evidence does not authorize installation or prove physical SD,
wake, heap fragmentation, runtime stacks or latency. See the current trial in
[015-m1-library-trial.md](015-m1-library-trial.md).

Final compiled source checkpoint: `efb28bb362e35e1e1945cf6bef8b7d139ff10317`. All four receipts bind the
executed build/verification procedures and their existing bytecode as well as
the authored source, toolchain, actual outputs and exact selected frames.

The regular USB variant was also measured with both identifiers: the clean
identifier image is 1,298,512 bytes and the existing `-dirty` identifier image
is 1,298,528 bytes. Adopt the exact observed development ceiling (16-byte image
alignment difference), without altering or hiding repository state.

Final validation: complete `python3 tools/check.py` passed on the integrated
source. All four clean native builds and linked NVS guards passed compilation;
all four final actual receipt/resource checks passed after exact-budget adoption.
The host fixture suites passed 21 structural, 39 resource and 7 procedure-cache
cases. Fresh reviews found no remaining actionable source or maintainability
findings; the fixture source/frame shim extraction preserves the new settings
regressions within the unchanged 700-line test policy. No private evidence is
included in these receipts.


## Codex review correction and base reconciliation

The following measurements supersede the initial SD integration table above.
Source checkpoint `72dfed42d69e87b7b42c49b9882726f31f570763` includes UX base
`05746191a8c9db3b67231eb2f06ca545b46fc432`, including protected native output
writes. The active SD identity is retained and restored independently of the
Home-versus-reader sleep destination. Eligible reset, full retained identity,
checksum/version and reader source-discriminator checks remain mandatory;
cold RTC bytes still cannot authorize a source or checkpoint. Home clears stale
fixture metadata and derives its Read availability from the selected book's
read-only durable store until a verified source supplies actual metadata.

The shared filename predicate rejects the full C0/DEL/C1 control ranges for both
catalog and RTC identity. The host matrix covers every C1 scalar plus valid
boundary names. A new sanitized regression executes verbatim selection and
sleep-entry code with actual SampleReader EPUB pages, Home/Library/app/device
routes, pending checkpoints, rejected-entry restarts, cold/corrupt/collision
rejection, inactive clearing and Home metadata/storage outcomes. Restoring either
of the original defective guards makes the regression fail.

Four clean native builds pass the existing resource ceilings. RAM, RTC and all
twelve selected compiler frames stay unchanged. Main shrinks from 546 to 544
lines; its allowance tightens accordingly. Flash ceilings also tighten to the
exact observed images without new slack, adapters or exclusions:

| Variant | Shared SRAM bytes | App image bytes | Partition headroom |
| --- | ---: | ---: | ---: |
| default | 133,200 | 1,292,336 | 18,384 |
| dev-usb | 133,920 | 1,298,448 | 12,272 |
| regional | 133,696 | 1,296,144 | 14,576 |
| regional-dev-usb | 134,416 | 1,302,304 | 8,416 |

Final review-fix validation: complete `python3 tools/check.py`, all four clean
native resource builds and all four source/procedure/output receipt checks pass.
Fresh adversarial/maintainability review found no remaining actionable issue in
these corrections. The privacy audit of 370 prospective text files found no
private book identifiers, passages, source paths or migration evidence.


## Transactional selection and bounded display metadata

Source checkpoint `ac89d56535fc5269d572733dfce09358b12a4a90` addresses the next Codex findings. Opening a
book rolls back the selected full identity, saved-policy bit, progress, Home,
readiness, UI and geometry on every candidate failure. A candidate commits only
after a successful initial page. Display titles retain a complete UTF-8 prefix;
empty titles use a generic label. Admission and retained paths share the SDK
resolver's explicit 511-byte container-name bound. No larger retained selection
or arena is introduced.

All four clean compiles and linked guards passed. The old exact flash ceilings
correctly rejected the necessary new behavior by 736 / 752 / 752 / 736 bytes.
A fresh adversarial and strict maintainability review accepted this exact
measured cost: transactional rollback needs a bounded prior-state snapshot, and
display metadata must not determine whether chapter text can be read. Reusing
the existing source owner and fixed title storage is simpler than another
reader or metadata owner. The following measured flash values become the exact
ceilings, without percentage slack, partition changes, RAM increases, new
exclusions, adapters or policy thresholds.

| Variant | Shared SRAM | App image | Partition headroom |
| --- | ---: | ---: | ---: |
| default | 133,200 | 1,293,072 | 17,648 |
| dev-usb | 133,920 | 1,299,200 | 11,520 |
| regional | 133,696 | 1,296,896 | 13,824 |
| regional-dev-usb | 134,416 | 1,303,040 | 7,680 |

RTC stays at 300 bytes, including the unchanged 68-byte SleepRecord and separate
140-byte full SD identity. PhasedReader remains 1,040 host bytes, with two
separate 49,152-byte arenas. All twelve previously selected frames stay within
their ceilings. The compiler outlines dispatchAction into a 400-byte frame;
PhasedReader::select grows from 112 to 128 bytes. Both are now selected, giving
fourteen protected frames. Loop ceilings tighten from 208 to 64 bytes in
non-USB builds and from 224 to 112 in USB builds; USB setup tightens from 192
to 176. These smaller individual frames reflect outlining and do not prove
lower total or runtime stack consumption. Hardware stack/heap acceptance remains
outstanding.

Verification passed: complete host gate, updated ratchet fixtures, all four
source-bound resource receipt checks, and fresh source/resource review.
Transactional mutations detect missing identity, Home or saved-policy rollback.
Authored cases cover loaded/empty prior SD progress, the inactive fixture route,
select/storage/admission/render failures, UTF-8 title cuts and 510/511-byte path
restore with 512/1,024-byte cold rejection. The read-only library trial stays
59/61 with 57 next-page anchor restores; the private proposed migration anchor
restores on the actual reader. Privacy remains 370 prospective text files with
zero private-evidence hits and six unchanged backup sources. Migration is
approximate and review-only; no progress or hardware writes occurred.


## Latest configured namespace integration

Compiled checkpoint `4bdc5b9afeb56bd1af909cf5478bdbc2e2c82465` merges upstream UX
`503006535a98be816f4bf1493f69d0e20db82df4`. The builder validates selected native build,
library-output and mutable cache/temp directory namespaces before its first
child or clean/build command. Approved C++ source filters, pins, XML flags and
the SD selection/metadata implementation remain unchanged. The upstream
UX-only handoff is explicitly scoped to its historical images and directs
combined-candidate review to the SD evidence. A fresh integration review is clean.

Final `tools/check.py` passes on this merged state, including 54 configuration
and 472 native-directory boundary cases and eight native-output tests. Four
fresh clean images pass the linked NVS/sleep guard and final receipt checks.

| Variant | Shared SRAM | App image | Partition headroom |
| --- | ---: | ---: | ---: |
| default | 133,200 | 1,293,072 | 17,648 |
| dev-usb | 133,920 | 1,299,216 | 11,504 |
| regional | 133,696 | 1,296,896 | 13,824 |
| regional-dev-usb | 134,416 | 1,303,040 | 7,680 |

The ordinary development identifier now includes the correctly reported
`-dirty` suffix from the handoff scope edit. Its aligned dev-USB image costs
16 more bytes than the clean checkpoint above; a fresh resource review accepted
only that measured ceiling, 1,299,216 bytes. Other flash values are unchanged.
This records normal development identity rather than hiding dirty state.
RTC, arenas, all fourteen compiler frames, partition capacity, main's 544-line
allowance, pins, adapters and exclusions remain unchanged. Individual static
frames still do not establish runtime stack safety.

The read-only library and private migration results remain unchanged, with
370 prospective text files free of private evidence and six invariant backup
sources. Migration remains approximate and unapplied; hardware remains untested.


## Upstream UX-only awake evidence history

The two following sections describe PR #3 before SD integration. Its fixture-only
images and budgets are historical; the combined candidate requires fresh native
receipts and reviewed SD resource costs below.

## Reviewed bounded awake wake evidence instrumentation

Two independent frozen source/domain/maintainability reviews are CLEAN for the
12-file diagnostic bundle `7a17fdebcd901f17519d72df7371ae662d2089f9dc21b0606728deffa01193f0`.
The exact main allowance changes from 546 to 556 lines: three hooks follow actual
completed-display receipts and seven net DEV lines capture primitive input context
before dispatch and its result afterward. The existing reader-open line passes an
optional evidence pointer. These observations belong at their existing ownership
boundaries; callbacks or a broader display/input refactor solely to reduce line
count would obscure sequencing. No other structural allowance changes.

The DEV receipt is 80 bytes of primitive awake RAM, replacing the previous 64-byte
diagnostic backing state; host SleepSession is 88 bytes versus 72. The DEV-only
DeviceInfo pointer, native global SRAM/flash and selected frames still require
fresh four-variant measurements and independent cost review after the procedural
C10 rebase. No resource ceiling, selected frame, SDK pin, adapter or exclusion
changes at this source checkpoint. The existing 68-byte RTC sleep contract and
NVS startup preservation guard remain untouched. Source tests establish capture
boundaries, not the physical battery-only failure cause or resume acceptance.


## Final measured awake wake-evidence cost

Compiled combined source `e1715bccd4ea25e633561397a503e8a80211f1f8` passes the full
host gate and four clean native compilations/linked preservation guards. Two fresh
actual runtime/resource/cost audits approve exact DEV-only diagnostic growth.

| Variant | Flash ceiling | SRAM ceiling | Selected loop/setup |
| --- | ---: | ---: | ---: |
| default | 1,234,848 | 132,432 | 208/176 |
| dev-usb | 1,244,528 | 133,152 | 240/192 |
| regional | 1,238,672 | 132,944 | 208/176 |
| regional-dev-usb | 1,248,384 | 133,664 | 240/192 |

Ordinary costs stay exact. DEV flash +3,584 / regional DEV +3,600, SRAM +16 each,
loop/setup +16 each are proportionate to one 80-byte receipt and thirteen bounded
readable rows. Native session72→88; info28→32 consumes former internal linker
padding, so data/alignment remain fixed and BSS grows16. Other four selected frames
remain736/80/80/912; no selected reductions exist to tighten. Supplemental individual
DEV frames: open wrapper80→48 plus newly outlined attempt96/capture64; paint80→96
(regional96→112), formatter80, Device renderer144 unchanged, EPUB chrome80→96.
These frames do not establish combined call-stack peaks or hardware high-water.

The ten-line main556 exception is unchanged from its independently reviewed
source checkpoint. All other structure/pins/adapters/exclusions/ownership remain
unchanged; reader allocation69,968 native/70,088 host and RTC68/total160 remain
exact. Initial DEV builds correctly failed only four old caps each; exact reviewed
adoption and final21/39/7 policy/receipt fixtures plus all four live resource checks
now PASS. C10 also binds actual invoking Python/runtime/stdlib and separate PIO
runtime, including native extensions/shared library/applicable executable caches.
Copied approved offline driver metadata relocation is separately bound and does
not imply historical toolchain digest equality. No new acquisition or hardware
acceptance is claimed. Battery cause remains unmeasured until the owner fixture.


## Combined SD recovery, standalone witnesses and awake evidence

The current SD candidate adopts UX head `fe4c4fa` and retains the fourteen
selected compiler frames. The prior 544-line main grows to an exact reviewed
556 lines: three completed-paint hooks, seven net DEV consumed-input observation
lines and two lines reporting a failed SD-source admission before reader parsing.
These observations belong to their existing display, input and source owners.
A fresh strict maintainability review accepts this narrow exception; no structural
policy, adapter, generated exclusion, pin or partition changes. The SD attempt
adds a typed outcome without enlarging the 80-byte awake evidence record. RTC
remains the separate 68-byte sleep and 140-byte SD source identity contracts.

Books entry owns its single recovery open, preventing a second load of an older
durable checkpoint. Shared host rendering receives the actual catalog/focus state
and exercises original CC0 fixtures. Standalone migration/probe invocations use
fresh SDK objects and executable identities, verify actual pinned SDK bytes even
when Git index flags hide changes, and recheck source/compiler/binary binding
before publication. These host facts do not establish hardware acceptance.

Later routing correction: this historical checkpoint's pending Books redirect
was replaced by Library browsing with replacement selection blocked. Home Read
owns exact recovery; the original cost and provenance above remain historical.
See [Books recovery routing](048-books-recovery-routing.md) for the correction.


The independently reviewed four-variant measurements bind source checkpoint
`60ccdca700eb` (compiled source unchanged from merge checkpoint `74788a6`) and the
actual procedure/driver Python runtimes. All clean compiles and linked NVS guards
pass. The DEV ratchet initially rejected the three intended old limits in each
variant. Fresh actual ELF/bin/map/stack and source/procedure/output checks accept
these exact ceilings, without percentage slack:

| Variant | Shared SRAM | App image | Partition headroom | Loop/setup frame |
| --- | ---: | ---: | ---: | ---: |
| default | 133,200 | 1,292,992 | 17,728 | 64/176 |
| dev-usb | 133,936 | 1,302,672 | 8,048 | 112/192 |
| regional | 133,696 | 1,296,816 | 13,904 | 64/176 |
| regional-dev-usb | 134,432 | 1,306,512 | 4,208 | 112/192 |

Ordinary flash ceilings tighten by 80 bytes. DEV-only diagnostics add exactly
16 bytes of BSS, 3,456 / 3,472 flash bytes and 16 bytes of setup frame. Loops and
the other twelve selected frame values remain unchanged; all fourteen selections
are retained. The 80-byte evidence backing replaces the old 64-byte diagnostic
state; no new reader arena, framebuffer, heap allocation or RTC state exists.
Supplemental individual DEV frames are openReader 128, outlined attempt 112,
capture 64, paint 96 / regional 112 and formatter 80 bytes. These observations
do not establish combined runtime stack safety or hardware acceptance. The
regional USB image remains close to the unchanged 1,310,720-byte partition.


Final combined validation: complete `python3 tools/check.py` PASS with ASan/UBSan,
including thirteen standalone provenance tests, eighteen migration tests, ten
Python-runtime tests, real external wake/recovery paths and exact preview checks.
All four final source/procedure/runtime/output resource verifiers PASS after exact
adoption. The read-only library trial remains 59/61 first readable chapters, two
unsupported results and 57 restored next-page anchors. The private proposal still
requires review and is unapplied; its actual Newt page/spine anchor restores. The
privacy audit finds zero private evidence in 381 prospective source text files,
and all six backup identities are unchanged. No hardware or SD write occurred.


## Upstream UX-only compiler-output preservation history

The following upstream measurements describe the fixture-only UX candidate.
Combined SD selection requires its own fresh resource receipts below.



## Reviewed complete compiler-output-tree preflight

C11 closes the Codex P1 descendant-alias write-through finding using one cohesive
`owned_output_tree` invariant and one builder preflight call. The entire selected
`.pio[/variant]/build` root is checked before any child, including target descendants
and sibling outputs. Only real directories and unaliased regular files are accepted;
symlinks, multiply linked files and special nodes fail closed. Approved libdeps and
toolchain dependency links remain outside this compiler-output ownership boundary.
Two fresh source/domain/maintainability reviews are CLEAN. Focused controls cover
168 rejections, eight lawful trees and 24 actual old-builder write-through cases
across all four clean/incremental routes. Correct fixture globals/root are asserted;
the earlier mock-output isolation mistake was restored and byte-verified before gates.

Compiled source `829f8c834568c351a99350483c9f90e24bc15f5b` passes the complete ASan/UBSan host gate and all
four fresh clean native builders (exit 0), linked NVS/sleep guards and unchanged
exact resource limits. Two fresh actual receipt audits are CLEAN. All flash/SRAM
and six selected frames exactly match the previous reviewed diagnostic measurements;
no allowance, pin, adapter, exclusion or ownership expansion is needed. C++ input,
compiler, toolchain and both runtime identities match; changed procedure sources
and DEV configs are bound by current receipts. Fresh hashes reflect build timestamps,
ELF/image digests and DEV identity, so historical byte equality is not claimed.
Only reviewed-source provenance changes in the baseline. Current candidate/archives
are bound in `artifacts/m1-wake-evidence-builds.json`; original old-cap builder exits
remain recorded separately as historical evidence. Hardware acceptance remains pending.

## Durable SD selection and refreshed standalone evidence

The current combined SD candidate uses source checkpoint
`bf2c88f8f8ef55cca578b24cc32a105e32a7bef4`, integrating UX head `25fc259`.
The subsequent merge of `main` (`855db43`) has no source or procedure delta.
Four clean native compilations and linked preservation guards pass. Their first
resource checks correctly reject the old flash, setup and dispatch ceilings.
A fresh independent audit of actual ELF/bin/map/stack outputs and live
source/procedure/runtime identities approves these exact measured costs:

| Variant | Shared SRAM | App image | Partition headroom | Loop/setup frame |
| --- | ---: | ---: | ---: | ---: |
| default | 133,200 | 1,295,552 | 15,168 | 64/624 |
| dev-usb | 133,936 | 1,305,248 | 5,472 | 112/640 |
| regional | 133,696 | 1,299,360 | 11,360 | 64/624 |
| regional-dev-usb | 134,432 | 1,309,088 | 1,632 | 112/640 |

Durable selection adds 2,560 / 2,576 / 2,544 / 2,576 flash bytes, 448 bytes
of setup frame and 16 bytes of dispatch frame. It adds no static SRAM, heap,
framebuffer or RTC storage. The isolated namespace stores a 144-byte explicit
NWLS v1 wire record in two slots; these are distinct from the 140-byte RTC SD
identity and 68-byte sleep record. Unknown records are preserved. Startup reads
without writes; selection saves occur only after successful page publication,
with an inactive-slot write and exact read-back. Failed saves remain visible,
including across eligible sleep/wake, while newer RTC ownership is preserved.

All fourteen selected functions remain protected. The select frame tightens
368 to 208 bytes in every variant; its outlined identity constructor is guarded
at 176 bytes. Three durable store frames are added: slot read 208, save 624 and
decode 176 bytes. All eighteen frame ceilings are exact. The visible linked
DEV boot chain totals 1,216 bytes and the selection-save chain 1,728 bytes;
these partial chains exclude NVS/SDK/RTOS descendants and interrupts and do
not prove runtime stack safety. Hardware high-water remains unmeasured. No
structural exception, adapter, exclusion, pin, flag or partition expansion is
introduced. The main source remains the reviewed 556-line exception.

The complete ASan/UBSan host gate passes, with seventeen standalone provenance
tests, eighteen migration tests and ten Python-runtime tests. After the procedure
merge, all eleven current native-output tests also pass; the historical alias
control pairs the old builder with its own configuration. Fresh integration and
strict maintainability reviews are clean. Final live resource checks pass for
all four variants, with 21 structural, 39 resource and seven procedure-cache
fixtures passing against the adopted baseline.

The regenerated SDK aggregate records 61 metadata opens and 2,859 streamed XML
parses, with a fresh verified helper receipt. Its requested XML peak is 15,832
bytes including Newt ownership headers. This is metadata/XML evidence only;
the distinct actual Newt reader trial remains 59/61 first-readable chapters,
two unsupported results and 57 next-page anchors restored. The reviewed migration
proposal remains approximate and unapplied. A final privacy audit finds zero
private evidence in 384 prospective source text files, with six migration backup
identities and all 61 probe book identities unchanged. Hardware testing remains
outstanding and X4 is deferred.

## Durable SD cold-boot destination

NWBR v1 adds a 32-byte explicit reader-or-Home intent in the isolated
`newt_resume` namespace. It references PR2's selected full-SHA source and existing
per-book 52-byte progress through the complete canonical progress CRC, rather
than copying the RTC record or inventing a second progress store. Only genuine
PowerOn with a verified SD selection and successfully loaded, matching progress
may reopen Reading. Existing RTC and embedded-sample behavior remains separate.
Unknown records stay untouched; startup remains read-only. A corrupt intent peer
could be a newer Home tombstone, so automatic resume fails closed. Deliberate
saves repair only known torn inactive peers, verify exact readback, and deduplicate
only when the resulting strict record remains loadable. Failed destination saves
cancel physical sleep before source/display/rail shutdown; the input task and
live reader return to the awake route with a visible storage warning.

The exact main exception becomes 573 lines from 556. Its additional orchestration
loads canonical SD progress before selecting a cold destination, checkpoints that
destination at actual exit/sleep boundaries, and shares the existing wake-pin
rejection cleanup with failed destination acknowledgement. Back also requires a
verified Home intent before closing the reader; failure restores Reading, resumes
the live workspace or preserves pending recovery, and reaches shared Page cleanup.
This adds two reviewed lines beyond the initial 571-line exception. Independent strict
review accepts these lines at their existing ownership boundaries. No structural
policy, adapter, exclusion, SDK pin, compiler flag or physical layout changes.
The typed inactive-slot implementation is shared by two actual wire schemas;
selected-book NWLS v1 bytes and its known-torn-peer fallback stay unchanged.

The build explicitly declares the retained CrossInk partition layout rather
than relying on the SDK sample's 1,310,720-byte slot. The independently checked
historical 16MiB image has a partition-table MD5 and valid OTA records selecting
app1 at 0x780000, capacity 0x770000. The CSV is an identity-bound firmware input;
configuration preflight pins its path. Native receipts measure and enforce the
actual generated table's minimum application-slot capacity. Exact labels, types,
subtypes, offsets, sizes, flags, MD5 and padding must match the declared six-entry
layout. Smaller SDK tables and same-capacity layout substitutions are rejected. Every installation still requires fresh target
identity/security/layout evidence, backups covering the exact new sector span,
full readback, preservation checks and bounded idle startup. No partition table
or bootloader is installed. This distinction does not authorize stock-board
installation or claim physical SD sleep/wake acceptance.

Independent actual ELF/bin/map/frame measurements approve exact SD cold-resume
costs on the frozen source inputs:

| Variant | Shared SRAM | App image | Existing app1 headroom | Loop/setup frame |
| --- | ---: | ---: | ---: | ---: |
| default | 133,216 | 1,298,288 | 6,500,496 | 80/656 |
| dev-usb | 133,952 | 1,308,080 | 6,490,704 | 112/672 |
| regional | 133,712 | 1,302,096 | 6,496,688 | 80/656 |
| regional-dev-usb | 134,448 | 1,311,920 | 6,486,864 | 112/672 |

Flash grows 2,736 / 2,832 / 2,736 / 2,832 bytes; shared SRAM grows 16 in every
variant: the native backend is 8 bytes and the linker adds 8 bytes of heap-start
reservation. Setup grows 32 bytes; ordinary loops grow 16 and DEV loops remain
unchanged. All other original selected frames remain exact. The two selected-book
read/save operations retain 208/624-byte limits under their shared typed names;
none is removed. Six new selections protect intent read96/save160/decode48,
canonical progress CRC80, persistence96 and cancellation16. All 24 ceilings are
exact. The DEV partial boot chain is 1,248 bytes, selection-save chain remains
1,728, new sleep-persistence chain512 and Home-exit chain928. These exclude NVS,
SDK, RTOS, interrupt descendants and actual task high-water; they are not runtime
stack bounds. Existing reader/framebuffer/awake evidence allocations and the
68/140-byte RTC contracts stay unchanged; total linked RTC remains300 bytes.

The first clean cost builds correctly rejected old compiler-symbol selections.
The complete host gate can generate a newly bound authored Python cache, so those
intermediate receipts are cost evidence, not final deployment identity. Final
clean builds and live resource checks must follow the host gate and source freeze.
No cache exclusion/removal, percentage slack or physical layout change is used.

The production change was frozen at `20115adc917173e23b0d6da2a7057f4c7bc15a65`.
The required gate now also invokes all eight production-wiring mutation controls;
each must compile and then fail its runtime assertion. The complete ASan/UBSan
host gate and all four clean native builders pass. The later Home-intent failure guard adds exactly
80/80/80/96 flash bytes in the interim dirty cost builds. Final clean
regional DEV measures 16 bytes less; its exact ceiling tightens to 1,311,920.
This is the measured clean-versus-dirty identifier distinction, consistent with
aligned image size, rather than a source optimization. No compiler inputs change
for this baseline/docs/evidence-only tightening. SRAM, RTC and all 24 selected frames
remain unchanged. Two independent actual cost audits approve only these
measured flash values and the 573-line integration exception. Fresh actual artifact audits
verify current cache/runtime identities, all 334 outputs per variant, compiler
ownership, 24 selected frames and independently disassembled preservation/sleep
guards. `artifacts/m1-sd-resume-builds.json` binds final source scopes, log hashes,
image/ELF hashes and the exact proposed carrier write spans. These build receipts
remain distinct from attached-device installation and owner SD-resume acceptance.

### Reader admission and cancellation correction

The real owner reading trial rejected Martian on the installed SD-resume build.
Host EPUB compatibility and a successful firmware installation did not establish
on-device readability. The native reader admission needs 164,864 bytes before
allocation; the measured idle Home heap was 166,232 bytes. The former 12,344-byte
resident SD catalog would reduce that to 153,888 even before mounted-SD overhead.
Actual mounted free heap and largest block remain required hardware measurements.

The filename catalog now exists only while browsing. Selecting copies the bounded
validated filename and full identity, then releases catalog storage while keeping
one source handle. The initial open reuses that already-hashed handle; each true
reopen reads the validated filename directly and verifies its full SHA again.
The 48 KiB shared/parse arenas and 32 KiB XML/runtime reserves remain unchanged.
Errors distinguish admission memory, parser memory, SD reads and unsupported EPUB.
A counted physical Sleep queue cancels subsequent bounded source reads, then the
normal UI queue consumes Sleep after the transaction unwinds. Successful exact
restoration retains its pending checkpoint until actual frame publication, closing
a late-cancellation loss found by the fresh strict review.

The independent strict maintainability review accepts only three exact path
ceilings: main 593 (previous 573), home_progress_gate 702 (previous generic 700), and
sd_selection_sleep 710 (previous generic 700). Main adds cancellation accounting,
open scopes and diagnosis at existing input/open/dispatch ownership boundaries.
The verbatim-main fixture adds its real cancellation owner include/global; the
selection fixture adapts the nullable catalog/direct-open/hook and failure-result
API. Extracting those hooks solely for line count would add ownership concepts.
Generic 500/700 thresholds, external adapters and generated exclusions stay fixed.
Source review does not establish device elapsed latency, physical stack high water,
optical appearance or reading acceptance. Fresh actual compiled resource costs
must be independently reviewed before any ceiling changes or candidate handback.

The actual mounted Library observation is 135,364 free bytes, so releasing the
catalog alone cannot satisfy the unchanged admission reserve. The reader now
borrows the pinned SDK's stable 52,272-byte display build storage exclusively for
parsing. A 10,820-byte bounded plan deep-copies the selected page's text and run
geometry. XML/session teardown precedes returning the clobbered display lease;
only then does rasterization paint the staging frame. The published logical frame
never contains parser scratch. Both 48 KiB arenas and both 32 KiB reserves remain
intact. Owned/borrowed actual reader tests compare pixels, anchors and statistics
in both orientations, with lease, allocation, I/O and cancellation faults.
Host allocation arithmetic is not a native heap or device acceptance measurement.
The C3 compiler reports 16-byte maximum arena alignment; the SDK framebuffer's
plain allocation does not promise that alignment. The configure adapter therefore
aligns only the parse window within the existing display allocation, discarding
at most 15 unused bytes while preserving the full 48 KiB arena. Each lease checks
current raw identity, aligned window and capacity before use. Host tests exercise
all raw offsets 0 through 15, both orientations, exact reopens and fault cleanup;
their host alignment differs from the separately compiled C3 alignment probe.

The final independent source review accepts the exact main ceiling of 608: the
borrowed-storage setup and DIAG2 observations stay at their existing firmware
ownership boundaries, with a separate small fingerprint helper and initial
opening feedback. Generic ceilings, adapters and exclusions are unchanged.
DIAG2 preserves the legacy boot facts and adds live workspace, heap, Library and
opaque device observations. Its maximum tested record is 507 bytes, requiring a
512-byte development-only TX buffer (64 bytes added); its local body grows from
380 to 464 bytes. The longest canonical input is 63 printable bytes, so the RX
buffer reduces from 96 to 80 bytes and recovers 16 persistent bytes. The firmware
aggregate assertion includes the eight-byte
fingerprint and retains its strict 1 KiB bound. Native builds must verify that
bound and fresh selected stack costs. Synchronous opens block USB polling, so
DIAG2 does not claim concurrent progress visibility or prove physical latency.

### Combined reader, Frosted Library and acceptance checkpoint

Two independent actual-artifact audits reviewed `af451342c0156e46ae30011cfc28b3c39fb0743c`.
The four clean C3 compilations pass their linked NVS and returning-sleep guards.
The exact reviewed resource ceilings are:

| Variant | Shared SRAM | Application image | Existing app1 headroom |
| --- | ---: | ---: | ---: |
| default | 133,248 | 1,236,144 | 6,562,640 |
| dev-usb | 134,064 | 1,246,784 | 6,552,000 |
| regional | 133,760 | 1,239,952 | 6,558,832 |
| regional-dev-usb | 134,576 | 1,250,640 | 6,548,144 |

Shared SRAM increases exactly 32 / 112 / 48 / 128 bytes relative to the installed
SD-resume source. Flash decreases 62,144 / 61,296 / 62,144 / 61,280 bytes. The
lossless immutable Home backdrop tiles save 67,492 raster bytes, offsetting the
new reader lifecycle and diagnostics code. No framebuffer, arena, XML reserve,
SDK flag, exclusion, adapter or partition change funds these improvements.

All 24 selected compiler frames remain present and static. Only setup increases
16 bytes to 672 in normal variants and 688 in DEV; phased layout increases 32 to
1,248; phased open increases 16 to 848; dispatch increases 16 to 432. These changes
cover borrowed-storage configuration, parse-lease cleanup, copied-page rendering
and cancellation/recovery ownership. All other selected ceilings remain exact.
RTC remains 300 bytes, shared SRAM capacity 321,296, and application capacity
7,798,784. Compiler frames do not measure task high-water or descendant peaks.

Actual C3 allocation constants are 99,352 bytes for the owned reader and 61,020
for the borrowed reader, saving 38,332. The largest allocation stays 49,152;
the two 48 KiB arenas, 32 KiB XML budget and separate 32 KiB runtime admission
reserve are preserved. Native admission therefore needs 126,556 free bytes
and a 49,152-byte contiguous block in borrowed mode. Actual mounted heap,
allocation success and elapsed reading latency remain hardware acceptance facts.

The unchanged strict DEV assertion measures its named objects plus the modeled
224-byte queue payload increase: 1,016 < 1,024 bytes. It excludes the additional
SDK `ESP_EFUSE_MAC` descriptor, which occupies 28 bytes in DEV `.dram0.data`.
The complete linked static DEV-minus-normal SRAM increase is 816 bytes in both
variant pairs, including SDK data and linker packing. Adding the separate modeled
queue increment gives 1,040 bytes before allocator/queue metadata and task peaks.
These scopes are distinct; none establishes a complete runtime DEV cost below
1 KiB. Full linked occupancy remains covered by the exact per-variant ratchet;
the existing named assertion and its bound are preserved.

The acceptance checkpoint adds the reviewed fixed reading journey and 20-state
renderer contract. The Library borrows its nullable catalog only while browsing,
closes every optional cover handle before return, and retains no catalog pointers
or rasters. Sidecars decorate covers and never replace the full SHA book-opening
identity. Source/host/native receipts do not authorize flashing or establish
physical reading, optical refresh, battery wake, or owner acceptance.


### Reviewed cooperative reader opening and input progress

Two independent actual-artifact audits are CLEAN for
`c437f6746221cd146ee49be6fc6374380297dec7`. Each rehashed all 334 output
artifacts per variant, checked current input/configuration/tool identities,
remeasured ELF/map/image/compiler frames, and verified the linked NVS, returning
sleep and strong 16 KiB loop-task provider with its actual startup argument.
The old budgets correctly rejected only the following measured RAM/flash growth:

| Variant | Shared SRAM | Application image | SRAM / image change | app1 headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 133,264 | 1,237,520 | +16 / +416 | 6,561,264 |
| dev-usb | 134,080 | 1,248,816 | +16 / +896 | 6,549,968 |
| regional | 133,776 | 1,241,328 | +16 / +400 | 6,557,456 |
| regional-dev-usb | 134,592 | 1,252,656 | +16 / +896 | 6,546,128 |

These exact ceilings fund the bounded opening timestamp, cancellation checks,
accepted/consumed progress drain and DEV phase traces described in
[the correction contract](020-bounded-reader-open.md). `BookOpenControl` grows
4 to 8 bytes; BSS grows eight including packing, and heap alignment adds eight
more. Net occupied IRAM is unchanged. There is no new persistent buffer, reader
allocation, queue, parser arena or reserve. RTC remains 300 bytes. The named DEV
aggregate stays 1,017 < 1,024 bytes; it is not a complete runtime DEV cost bound.
The linked DEV-minus-normal static SRAM difference remains 816 bytes.

Both DEV `loop()` compiler-frame ceilings tighten from 112 to 96 bytes. The
other 23 selected frames remain unchanged in those variants; all 24 remain
unchanged in non-DEV. No frame is removed. The existing 608 to 615 main.cpp
allowance was independently reviewed for the seven direct owner integration
lines. Generic file thresholds, exclusions, adapters, SDK pin, compiler flags,
partitions, both 48 KiB arenas, XML budget and admission reserves remain intact.

The provenance binds the reviewed source and measured per-variant input digests.
These preliminary builds are evidence for the cost decision; final clean PASS
builds follow the resource-only adoption freeze. The full ASan/UBSan host gate
passed the reviewed source, including real compressed-reader rollback in both
orientations and exact PBM/PNG previews. Production, tests and tool scopes must
remain identical across this policy-only adoption; focused ratchet fixtures
check the final policy before native receipts are created.

The 20-second deadline is cooperative and cannot interrupt a blocked SDK driver
call. Source and native checks do not establish physical latency, sibling reading,
optical behavior or owner acceptance. The original 30-second terminal deadline
and full 23-stage reading journey remain required. No corrected image has been
flashed; the previous failed activation's completion remains unknown.

## Reader admission diagnostic checkpoint

The observation-only admission checkpoint retains the generic 500/700 thresholds,
all adapters/exclusions and every original selected frame. Two independent source
reviews accept only main.cpp 619→631: one outer profile and the development-only
I/O/SHA callback bridges stay at the existing scheduler owner. The maintainability
review also accepts home_progress_gate 702→703 for the zero-output serial mock's
write interface; the existing wake mock receives the same interface adaptation.
These changes do not alter acceptance, device authorization or native budgets.
A strict review found unchecked printf could falsely claim a complete row after
allocation failure. Fixed 160-byte formatting and exact write acknowledgement
close that defect, with zero/short write controls for original/detail/final rows.
The targeted adversarial and maintainability rereviews are CLEAN. Numeric profile
storage is 432 host bytes on the existing scope's stack; new native frames and
simultaneously live nested scope storage require separate actual cost review.

Two independent actual-output audits approve these exact preliminary costs:

| Variant | Shared SRAM | RTC | Image | Image change |
|---|---:|---:|---:|---:|
| default |133,264|300|1,238,640|+752|
| dev-usb |134,080|300|1,253,904|+4,208|
| regional |133,776|300|1,242,464|+768|
| regional-dev-usb |134,592|300|1,257,760|+4,224|

SRAM and RTC are unchanged. Only DEV original selected frames increase: hash
1,216→1,232 and dispatchAction 432→864. All original 24 remain selected. Four
additional exact guards per variant protect openReader 160/608, admission 32,
verifyStreams constprop 448 (former 416) and observer 48. DEV also selects the
scope destructor 240 and detail formatter 272, giving 28/30 selected frames.
Native DWARF confirms profile 432 and scope 444 bytes. Two lexical scopes coexist
(888 bytes), already included in their actual compiler frames. Nested scopes
borrow counters but still have their own automatic storage.

Both audits rehash all 334 outputs per variant, rederive ELF/map/image/frame costs,
and independently recheck linked NVS, retained RTC, returning sleep and the actual
16 KiB task provider. Compiled-source/configuration/compiler/toolchain/runtime
identities match. The host gate generated authored Python caches during these
preliminary builds: only procedures_sha256 was stale for the first three reports;
the fourth matched at audit time. These are cost receipts, not final candidates.
Final four clean builds must bind the frozen source and current procedures.

The DEV direct-reader formatting path has a measured partial frame subtotal of
3,648 bytes, including loop task/loop/dispatch/openReader/scope destructor/details,
a row helper 224, snprintf 176 and linked _svfprintf_r 1,152. This excludes descendant
helpers and other reading paths; it is not a runtime stack bound. The historical
6,616-byte free observation does not certify this diagnostic. New hardware stack
observations require a separate exact-image grant. Both 48 KiB arenas, XML/runtime
reserves, framebuffer, SDK, task size, partition and acceptance remain unchanged.

Final frozen-source builds at 4a31ec31750b8877a2fb555375fd2f1d6c3eb13b all exit 0.
Two independent final audits verify CURRENT canonical identities with no stale
procedure/cache fields and all 334 canonical and archived output hashes per
variant. All 28/30 selected ceilings equal their actual frames. Final regional
DEV is 1,257,744 bytes, 16 below the dirty preliminary image; its exact ceiling
therefore tightens to 1,257,744. This identifier/alignment difference is not a
source optimization. The other three exact image values remain unchanged.
The full host gate passed the identical compiled/tool/test scopes; policy fixture
suites pass 21/39/7 checks. `artifacts/m1-admission-diagnostic-builds.json` records
all final image/ELF/log/receipt identities and source scopes. A separately granted
physical diagnostic is still required; no installation or M1 acceptance is claimed.

## Scoped directory-window cost review

The physical diagnostic identified central/local seek alternation during EPUB
inspection. Source checkpoint `b739b6722ecd9d509b2908df624f598066cc32ba` borrows
4,096 bytes of the existing parse arena during inspection and releases them
before catalog/inflater work. It introduces no persistent/global buffer or heap
allocation. Removing the validity guards, scheduler checkpoints or strict bounds
to save code would weaken the intended cancellation/source behavior; changing
the SDK or adding a general cache would broaden ownership unnecessarily.

Four clean preliminary measurements retain unchanged SRAM, RTC (300 bytes), all
existing 28/30 selected stack frames, 16 KiB task ownership and linked NVS/sleep
guards. The actual app images measure 1,239,648 / 1,254,896 / 1,243,456 /
1,258,752 bytes for default / dev-usb / regional / regional-dev-usb. Relative to
the diagnostic ceilings, these are 1,008 / 992 / 992 / 1,008 bytes of explicit
feature growth. The regional DEV preliminary identifier is dirty; these are
cost measurements rather than final exact-image acceptance.

The specific flash ceilings adopt those measured values with no percentage
slack. Remaining app headroom is 6,559,136 / 6,543,888 / 6,555,328 / 6,540,032
bytes in the unchanged 7,798,784-byte app1 slot. Three new helper frames are
selected in every variant: `ZipDirectoryWindow::read` 32 bytes, `readFully` 32
bytes and `validSource` 48 bytes. This expands coverage to 31/33 selected frames;
it removes no existing selection. Individual frames are not a call-chain or
runtime stack bound. Final clean images must be remeasured against the frozen
source/procedures, and any smaller final measurement must tighten its ceiling.
No structural threshold, oversized-file allowance, include adapter, generated
exclusion, arena, reservation, partition or integrity policy changes.

Final four clean builds at `57ba552281813d6979b71c9714b6a715537b800d` all exit 0.
Their compiled source matches the implementation checkpoint; policy and docs
are the only intervening authored changes. Default, dev-usb and regional image
sizes match the preliminary measurements. Final regional DEV is 1,258,736 bytes,
16 fewer than the dirty preliminary image; the exact ceiling tightens to that
value, leaving 6,540,048 bytes. This is an identifier/alignment difference, not
a source optimization. All 31/33 selected ceilings equal actual static frames.
Final receipt identities and full host evidence are recorded in
`artifacts/m1-directory-window-builds.json`; build evidence does not authorize
installation or certify runtime stack/physical reading.

A separately authorized installation of that exact final image subsequently
passed full-flash preservation, two bounded boots and the complete fixed
23-stage reading/resume/recovery journey. Its 186 actual loop-task observations
show 5,752 bytes minimum free on the unchanged 16,384-byte task, including an
observation after the final reader paint. This is scoped runtime evidence, not
an all-input stack bound or M1 acceptance. No resource ceiling or source scope
changes follow from it. See `032-directory-window-physical.md` and
`artifacts/m1-directory-window-physical.json` for the independently audited
measurement and remaining owner gates.


## Scoped reader page reuse cost review

Independent adversarial and maintainability reviews at `100a6b4` approve the
reader page reuse feature's measured +848-byte maximum per image. This funds
owned current-page repaint, certified same-chapter prefixes and live source
guards, without extra runtime allocation or retained parser state. The host
benchmark verifies all 132 paired page/anchor/totals/pixel rows; it does not
establish physical latency. See `034-reader-efficiency.md` for identity,
cancellation, rollback and prefix limitations.

Default / dev-usb / regional / regional-dev-usb preliminary ceilings become
1,240,496 / 1,255,744 / 1,244,304 / 1,259,584 bytes. Final clean measurements
must tighten smaller values. RAM, RTC 300, 16 KiB task, all original 31/33
selected frames, structural policy, adapters and exclusions remain unchanged.
New selected static frames are repaint 96, validateSource 32 and readAt 32
bytes: fs::File in normal variants, ReaderProfileFile in development variants.
Coverage becomes 34/36 selected functions; individual frames do not bound a
call chain or runtime stack. Four fresh clean source-bound reports remain
required. Baseline source/image/evidence and current progress are preserved;
this resource adoption authorizes no hardware action.

Four final clean builds at `f22f72044de2f20bac68051263ddf2e7f9ffa4d2` all exit 0.
Their image measurements equal the adopted exact ceilings, and all 34/36
selected frame ceilings equal actual static frames. Full host ASan/UBSan,
recovery, both orientations and exact previews pass. The complete archives and
proposal are recorded in `artifacts/m1-reader-efficiency-builds.json`; both
independent final audits are CLEAN at e27e497. They verify current canonical
identities, all 334 canonical and archived output hashes per variant, rederive
ELF/map/bin costs and NVS/sleep/RTC/task guard proofs. Compiled/tool/test scopes
remain identical to f22f720; only resource provenance/docs/evidence changed.
This evidence contains no
hardware action or runtime after measurement.

## Scoped failed-chapter sleep destination cost review

Two fresh independent adversarial and maintainability reviews approve the
acknowledged last-good checkpoint condition at `7f44c25`. The failed reader
retains a valid saved tracker while `opened()` is false; intended Reading must
not become Home merely because the next chapter could not be rendered.

Four clean preliminary builds independently measure flash increases of
16 / 16 / 32 / 16 bytes. Exact ceilings become 1,240,512 / 1,255,760 /
1,244,336 / 1,259,600 bytes. There is no extra slack. SRAM, RTC 300,
all original 34/36 selected compiler frames, the 16 KiB task, structural policy,
adapters, exclusions and NVS preservation guard remain unchanged. Both reviews
rechecked all 334 output hashes per variant and linked guard receipts.

Four final source/procedure-bound clean builds remain required; smaller final
measurements must tighten these values. This exception funds only the guarded
sleep destination fix. The periodic-progress/wake-intent checksum gap and actual
owner battery/first-spine/focus acceptance remain release blockers. No record
migration, SD change, installation or hardware write is authorized here.

All four final clean native builds at `06248a911bb8` now exit 0 and reproduce
those exact adopted measurements. Full host ASan/UBSan, both orientations,
recovery controls and exact previews pass. Both independent final native audits
are CLEAN, including 2,672 canonical/archive output hash comparisons,
independent ELF/map/image remeasurement and linked NVS/sleep/RTC/task guards.
Compiled/tool/test scopes remain identical to `7f44c25`; the final evidence is
`artifacts/m1-battery-recovery-builds.json`. This source checkpoint is not a
flash authorization or proof of the owner's battery/first-spine/focus behavior.

## Scoped committed-checkpoint integration review

Two fresh independent adversarial and maintainability source reviews approve
only `firmware/main.cpp` 631 → 647 lines. The 16 added lines integrate immediate
explicit opening acknowledgement, staged progress saves and committed cold
authority at existing owners. Algorithms remain in a small project-owned
`reader_checkpoint.h`; no artificial wrapper or formatting compression is used.
The selection test's extracted in-memory backend is reusable and preserves its
verified behavior. Its source is now below the generic 700-line test boundary,
so its old 710-line exception is retired. Native ceilings and all original
selected frames remain unchanged until separate measured cost reviews.
See `041-committed-reading-checkpoint.md` for transaction and evidence limits.

## Scoped committed-checkpoint native cost review

Two independent adversarial and maintainability reviews approve exact measured
costs from four clean builds of `8e49f46`. Flash ceilings become 1,244,240 /
1,259,488 / 1,248,048 / 1,263,328 bytes, increases of 3,728 / 3,728 / 3,712 /
3,728 bytes. They fund protected peer matching, strict validation and the final
NWBR commit; there is no extra slack. SRAM and RTC 300 remain unchanged.

All original 34/36 selected static frames remain. Dispatch becomes 576 bytes
normally and 1,008 with development USB; development openReader becomes 624.
Selected-codec save tightens from 624 to 496. Eight new guards measure selected
saveProtected 656 and loadMatching 640, Library loadCheckpoint 368, saveReadingCheckpoint
368, loadCommittedPowerOnReadingResume 112, and full-digest checked progress
readSlots 64, save 336 and checkpointSlot 80. Coverage becomes 42/44 functions.

Both reviews rederive all output hashes, ELF/map/image/compiler-frame costs and
linked NVS/sleep/RTC/strong 16 KiB task guard receipts. Individual static frames
do not establish aggregate runtime stack peaks. Generic policy, adapters,
exclusions, task size, arenas and partition capacity remain unchanged. Four
fresh frozen-source/procedure clean builds and final audits are still required.
This source cost exception authorizes no hardware action or release acceptance.

The complete host gate exits 0 at `086044f`. Post-host checks detect procedure
bytecode caches created by that run, so the earlier `9540109` native receipts
are retained as historical evidence. Four fresh clean gates after caches settle
at `086044fc05a8` exit 0 and reproduce the exact adopted costs. Both refreshed
independent final native audits are CLEAN: current identities, 2,672 canonical/
archive hashes and rederived ELF/map/image/frame and linked guard proofs pass.
Final receipts are in `artifacts/m1-battery-transaction-builds.json`.


## Scoped opening-feedback source review

Fresh independent adversarial and thermonuclear maintainability reviews approve
only main.cpp 631→644 for visible opening and physical Back cancellation: two
owner includes, four lines at the existing paint boundary, one outer-scope
paint check and six lines for a shared consumer after DEV ticket ownership.
The first review approved 638; owner analysis then found repeated cancellation
could reach Home Read. The shared boundary consumes every physical Back tag
before ordinary action mapping. Two fresh second-round reviews are CLEAN. The new display/input helpers isolate their concrete ownership
boundaries; further extraction would add indirection rather than simplify them.
All generic thresholds, adapters, exclusions, native RAM/flash/stack ceilings,
SDK pin, partitions and reader admission/storage contracts remain unchanged.
Native cost measurement and separate exact candidate review remain required.
See [037-opening-feedback.md](037-opening-feedback.md) for behavior and evidence
limits. This structural allowance authorizes no hardware action.

## Combined runtime recovery source review

Fresh independent combined adversarial and maintainability reviews approve
exactly main.cpp 647 to 660: the recovery owner remains intact and gains the
13 previously reviewed opening-feedback owner lines. Navigation changes stay
in backdrop expansion and borrowed thumbnail drawing. All prior 42/44 selected
frames remain exact; four previously reviewed opening guards make 46/48.
No generic threshold, adapter, exclusion, task size or partition changes apply.
The final Library acceptance check occurs before selected-identity writes;
physical Back arriving during accepted durable writes then follows the actual
Reading exit route. The authored integration fixture must exercise that route,
rather than substituting token consumption alone. See
[042-runtime-recovery-integration.md](042-runtime-recovery-integration.md).

## Combined runtime preliminary native cost review

Two independent native cost reviews approve exact flash ceilings 1,244,880 /
1,260,128 / 1,248,672 / 1,263,952 bytes: increases of 640 / 640 / 624 / 624
over committed recovery. SRAM remains 133,264 / 134,080 / 133,776 / 134,592
and RTC remains 300. Every one of the 46/48 selected static frames equals its
existing ceiling, including all original 42/44 recovery guards.

Both reviewers independently verify 337 canonical and 337 archived artifacts
per variant, rederive ELF/map/image/partition/compiler-stack measurements and
linked NVS/sleep/RTC/owned 16 KiB task proofs. The preliminary builds compile
successfully and fail only the previous flash ceilings. Their dirty source IDs
are historical measurements. Exact final identity requires the full host gate
to settle procedures, frozen source and four new clean native builds. No RAM,
stack, task, partition, policy, adapter or exclusion relaxation is approved.

The complete combined host gate exits 0. Four fresh post-host clean native gates
at `1feff5199636890a5afc2aaf19179a65b4e4dacf` also exit 0. Both independent
final audits are CLEAN: all 2,696 canonical/archive hashes, current bindings,
rederived measurements and linked guards verify. Actual DEV flash tightens
16 bytes to 1,260,112; other ceilings remain exact. Final proof is
`artifacts/m1-runtime-recovery-builds.json`. The regional development candidate
is 1,263,952 bytes, SHA-256
`9bd9bd8fe518f8b6a30f8358dbb8d821fb6097ae5a9bc3e5ad1a3caa64effecb`.
The exact image is installed with full readback and outside-APP1 preservation.
Boot identity and saved-page opening pass on the X3; first Next fails Unsupported
and blocks M1 acceptance. See `docs/042-runtime-recovery-integration.md` for actual
device evidence and its limits. These observations do not alter native cost proof.

## Reader spacing correction resource review

The full host gate passes at frozen source
`29fffd243c63c0b75f19df33c247782aa2b8136b`. Four subsequent clean native
compiles succeed and initially fail only the previous flash ceilings. Two
independent complete artifact audits approve exact flash ceilings 1,244,928 /
1,260,176 / 1,248,736 / 1,264,000 bytes, growth of 48 / 64 / 64 / 48 bytes
over the adopted final runtime image. This cost provides explicit coverage,
advance, kerning and no-ink semantics for U+200B and U+2003; removing it would
restore the privately reproduced page rejection. The original atlas is unchanged.

Each audit verifies all 2,696 canonical/archive artifact hashes, fresh outputs,
current complete build identities, actual ELF/map/bin/partition/stack measurements
and linked NVS/sleep/RTC/task guards. Shared SRAM remains 133,264 / 134,080 /
133,776 / 134,592 bytes, RTC remains 300, and every original 46/48 selected
frame stays static at its existing ceiling. No generic policy, adapter,
exclusion, task, partition, RAM or stack allowance changes. App headroom in the
unchanged 7,798,784-byte carrier is 6,553,856 / 6,538,608 / 6,550,048 /
6,534,784 bytes. This adoption accepts exact measured flash cost without slack.

The frozen regional development candidate is 1,264,000 bytes, SHA-256
`30323f13649b6a076c17a542d5ca5d9bf59de1644b1015c3b405ea46083221c4`,
build `29fffd243c63-regional-experiment`. See
[043-reader-spacing-failure.md](043-reader-spacing-failure.md) and
`artifacts/m1-reader-page-failure.json`. Metadata adoption preserves the exact
compiled source/configuration and requires read-only current receipt validation;
it neither rebuilds the frozen image nor authorizes installation or physical M1
acceptance.

## Canonical Unicode fallback flash adoption

The owner's scoped approval accepts the attributable correctness cost of frozen
source `6598fa543de1897137a9d1138991788161cee3f2`. The committed
`artifacts/reader-text-policy-verification.json` binds its source files, prior
baseline and final four-variant measurement summary. The single bounded
semantic-width optimization saved 80 / 96 / 80 / 80 bytes before this adoption.
App-image budgets increase by exactly 64 / 48 / 48 / 64 bytes to 1,244,992 /
1,260,224 / 1,248,784 / 1,264,064 bytes. No arbitrary budget slack is added.
The baseline records that source and its per-variant compiled-input digests.

That summary reports unchanged shared SRAM (133,264 / 134,080 / 133,776 /
134,592 bytes), RTC (300 bytes), all selected stack frames and linked guards.
RAM, stack, task, partition, adapter, exclusion and structural allowances remain
unchanged. Minimum reported physical headroom is 6,534,720 bytes in the verified
7,798,784-byte carrier, before the separately reviewed reserve policy.

This adoption independently checks the frozen summary and source hashes; its
final raw receipts and build artifacts are not committed here. Existing committed
`code-health-*.json` receipts describe the preceding spacing build. They must not
be relabeled or accepted as final Unicode evidence. The existing read-only native
resource verifier must validate current complete receipts and actual outputs at
integration. Changed development identifiers or build inputs require fresh
measurements. This metadata approval neither establishes device acceptance nor
changes the resource verifier or waits for the broader reserve/latency policy.

## Current-runtime Linux provider integration

The combined process integration restores the already reviewed host-only
`include/newt/host_sha256.h` provider boundary. Its exact adapter list contains
only `CommonCrypto/CommonDigest.h` and `openssl/evp.h`, matching the locked host
providers. ESP32 package hashing remains on mbedTLS. This is a reviewed adapter
admission for that named file, with no new general include or exclusion rule.
All current glyph-inclusive RAM, flash and selected stack budgets are preserved.

Native procedure identity also binds `release_tools.py`, including its executable
bytecode variants, for the release identity wiring supplied by the versioning
integration. The existing complete runtime/compiler/input/output checks remain.
The final combined source still requires its current release inputs and fresh
appropriate host/native proofs; component controls and old receipts do not confer
that acceptance.

## Reader runtime source integration

PR #16 retains the existing SDK/host-crypto boundaries and adds the specific
`canonical_book.h` adapter for bounded package-parser/XML metadata selection.
Its 682-line main allowance is the existing 660-line integration plus 22 lines
for the reviewed scheduler/action receipts and accepted-open title publication;
generic thresholds and all other exceptions stay unchanged. This structural
synchronization is not native resource adoption. Missing selected frames must
be restored through protected history and fresh four-variant measurements.
Historical runtime receipts do not prove this integrated candidate.

The frozen scalar oracle includes one generated data table,
`tests/fixtures/reader-font-scalar/reader_font_data.inc`. Its exact preexisting
SHA-256 is checked by the independent oracle runner; it receives the same
explicit generated-data classification as the production table. No implementation
file, generic directory or new exclusion is added.


## Fresh PR #16 resource adoption

Clean measured source `df2b1c23f80b4ad4cb134ffe64c3f5424c4be52e` supplies the
four `artifacts/pr16-raw-code-health-*.json` reports. Each original measurement
failed only the preceding main flash ceiling; these raw reports are cost evidence,
not passing acceptance receipts. Archived actual outputs, receipt hashes, source
inputs and emitted stack frames were independently checked by
`pr16_source_correctness_r1` and `pr16_source_maintainability_r1` before adoption.

| Variant | Previous image ceiling | Measured new ceiling | Shared SRAM |
| --- | ---: | ---: | ---: |
| default | 1,244,992 | 1,288,976 | 133,264 |
| dev-usb | 1,260,224 | 1,307,776 | 134,080 |
| regional | 1,248,800 | 1,292,768 | 133,776 |
| regional-dev-usb | 1,264,064 | 1,311,584 | 134,592 |

This is the aggregate measured cost of the reviewed runtime correctness,
cache/session, semantic-metadata and font/UI integration; it does not isolate
individual feature costs. Flash ceilings equal actual images, without added
slack. The partition and reserve are unchanged; minimum usable headroom is
5,704,864 bytes. RTC remains 300 bytes and all four RAM ceilings are unchanged.

Protected history restores 44 omitted selected runtime frames per variant at
their original tighter limits. It also tightens selection to 80 bytes and
`openReader` to 144 bytes (non-DEV) / 560 bytes (DEV). Every previous selection
is preserved; the resulting 90 / 92 / 90 / 92 selected frames are present and
static in these measurements. Static individual frames do not establish task
stack safety or runtime peaks. The original audit anchor and preceding flash
record remain intact; explicit source-policy pins bind the appended protection.

Changing those procedure pins requires fresh clean builds of all four variants.
The final source still requires its complete host/safety/native gates and exact
release identities. Raw measurements, review metadata and historical runtime
receipts cannot substitute for those gates or for physical device acceptance.


## Immediate-reader Home projection deferral

Clean source `c3bfafc7e2ceae8b7fdb5b92b16b2e550baeab6c` supplies the four
`artifacts/pr16-opening-raw-code-health-*.json` cost reports. Both independent
reviewers checked the actual archived outputs, complete input identities, and
emitted static frames before this explicit adoption. Each raw report failed
solely the preceding flash ceiling; it remains cost evidence, not acceptance.

| Variant | Measured ceiling | Growth | Shared SRAM |
| --- | ---: | ---: | ---: |
| default | 1,289,056 | 80 | 133,264 |
| dev-usb | 1,307,856 | 80 | 134,080 |
| regional | 1,292,864 | 96 | 133,776 |
| regional-dev-usb | 1,311,680 | 96 | 134,592 |

Ceilings equal the current images without slack. RAM, RTC, partition and reserve
are unchanged. All existing selected bounds remain intact. New guards bind the
shared progress-only loader to its measured 336-byte frame and its boolean
caller to 32 bytes in all variants; selected counts become 92 / 94 / 92 / 94.
These static frames do not prove whole-chain stack safety or a device saving.
Both complete histories and the original anchor are preserved. Only their two
policy pins change; fresh complete canonical host/safety/four-clean-native proof
on the resulting commit is required. The large-book latency gap remains open.

## Canonical declared-section publication

Implementation `80bd6dbe48c59e0bcf997e5834f45ef422d2c6b7` supplies the four
`artifacts/pr23-raw-code-health-*.json` diagnostic cost reports. The default
report was produced from earlier dirty source `01506ec`; its complete compiled
input hash matches the frozen implementation bytes. The other three reports
bind clean `80bd6db`. Neither identity is relabeled. All four compiled and failed
only the prior flash ceiling. Independent correctness and maintainability
reviews checked report/input hashes, archived outputs and emitted frames before
this explicit adoption; these raw reports are cost evidence, not final gates.

| Variant | Exact flash ceiling | Growth | Shared SRAM |
| --- | ---: | ---: | ---: |
| default | 1,295,440 | 6,384 | 133,264 |
| dev-usb | 1,314,208 | 6,352 | 134,080 |
| regional | 1,299,248 | 6,384 | 133,776 |
| regional-dev-usb | 1,318,032 | 6,352 | 134,592 |

The cost buys optional canonical navigation parsing and transactional metadata
publication through two owned selection slots. Missing or unusable declarations
retain the generic label; actual source/I/O/cancellation failures reject the
candidate. Linked C3 getters measure a 3,571-byte selection allocation request,
102,212 owned / 62,676 borrowed external allocation totals, and a 49,171-byte
largest external request. Preflight includes the selection allocation. These
are allocation requests, not runtime heap peaks or device allocation proof.

Flash ceilings equal measured images without slack. RAM, 300-byte RTC, partition
and reserve remain unchanged; minimum usable headroom is 5,698,416 bytes.
Protected history adds eight static guards for selection lifetime, cache capture
and canonical navigation, and tightens seven affected frames: open 864 to 64,
layout 1,248 to 464, select 144 to 128, cache stream 1,168 to 384, cache show
304 to 208, external overview 1,104 to 304, and Home projection 1,456 to 1,072
bytes. Every preceding selected entry remains, giving 100 / 102 / 100 / 102
guards. Individual static frames do not establish whole-task stack safety.

`src/reader_navigation.cpp` joins the selected diagnostic source list. All
existing sources, pinned tools, rules, flags and 17 negative controls remain.
The audit anchor and complete preceding histories are preserved; the protected
source/digest pins bind the reviewed append. Fresh exact-source complete host,
selected safety and four clean native gates remain mandatory. UI authority was
separately reviewed on all 20 authored frames. These changes establish no
physical behavior or latency saving; performance acceptance remains unmet.


## Historical sibling: fresh full-digest Library receipt reuse


Reviewed source `1acf07d1c690bab27f9619993e206ddbf6dd93cc` preserves first-import
all-member admission. An unchanged Library reselect reads the complete current
source and checks its full digest against a validated admitted receipt before
using the existing verified warm path. Missing, corrupt or changed receipts
retain strict import; a changed existing receipt can require both the comparison
hash and the new import hash. Filename, size and timestamps are not identity.
The both-orientation cold fixture now uses independently empty stores/processes;
actual Library reselect fixtures verify exact saved pixels/anchor/progress and
zero repeated all-member admission or durable/derived writes.

Four output-clean raw builds match the reviewed source bytes and fail only the
preceding flash ceilings. Their reports are retained as
`artifacts/selection-reuse-raw-code-health-*.json`; the complete output closures
remain in the local gate archive. Both USB variants were also rebuilt with the
committed source identifier and confirm the same exact image sizes. Paired
`selection_reuse_adversarial` and `selection_reuse_maintainability` reviews checked
all report, input and output hashes, linked memory/stack measurements, and the
unchanged partition reserve before this explicit cost adoption.

| Variant | Measured flash ceiling | Growth | Shared SRAM |
| --- | ---: | ---: | ---: |
| default | 1,289,584 | 528 | 133,264 |
| dev-usb | 1,308,384 | 528 | 134,080 |
| regional | 1,293,392 | 528 | 133,776 |
| regional-dev-usb | 1,312,208 | 528 | 134,592 |

Ceilings equal actual images without slack. RTC remains 300 bytes, and the
unchanged usable partition has at least 5,704,240 bytes of headroom. RAM and
every previous stack guard remain unchanged. Protected history adds only
`prepareSelection` at 112 bytes and `hashSource` at 64 bytes in all four variants,
for selected counts of 94 / 96 / 94 / 96. The Library selection caller remains
80 bytes. No retained fields, heap allocation or catalog lifecycle change.

The noinline boundary separates the new digest workspace from that caller;
it does not reduce whole-chain stack use. Linked selection/preparation/hash
frames total 256 bytes before children. Receipt-load and changed-source import
prefixes reach 384 bytes before descendants; missing/corrupt-load fallback
releases the preparation frame before its import tail call. Strict-open/hash
frames total 144 bytes before children. Individual frame guards and these
partial prefixes do not establish runtime peaks or whole-task stack safety.

Both complete audit histories and the original anchor are preserved. The two
explicit protected-history source pins change. Fresh complete host and four
clean native gates on the resulting commit remain mandatory. Source fixtures
and cost evidence do not establish physical timing, optical page usability,
cold-cacheless acceptance or a new installation.

The diagnostic capacity correction at `90b593afbf36954735fa84a5d84f2723db741f5e`
keeps the 160-byte formatting buffer and caller bounds, but reserves only the
actual formatted row length. One exact write acknowledgement still determines
each emitted bit; no wait, retry, retained storage or mask relaxation is added.
Four fresh clean native gates record images of 1,289,584 / 1,308,336 / 1,293,392 /
1,312,160 bytes. Both USB ceilings are tightened by 48 bytes to those measured
images. Non-USB ceilings, RAM, RTC, every existing selected stack guard and the
protected history remain unchanged. The complete output closures are archived
locally, with the corresponding reports committed as
`artifacts/selection-reuse-diagnostic-code-health-*.json`. The authored FAT
fixture now uses a short temporary root so a long checkout does not exceed the
unchanged 256-byte storage path bound before its intended historical mutation.
No device performance improvement is inferred from these resource measurements;
the final policy commit still requires the complete host, selected safety and
four fresh native gates.

## Combined selection reuse and canonical navigation

The normal merge preserves the complete `04b49c06c4efd04cda31c1cd53dfc7c07c5b5592` handoff ancestry. The figures in the preceding sibling section describe that branch alone. Its baseline is retained byte-for-byte in `artifacts/selection-reuse-branch-baseline.json`; those flash deltas are not added to navigation ceilings. The active linear flash audit remains the navigation audit until all four combined images are measured and independently reviewed.

Every existing navigation RAM and selected-stack limit remains unchanged or tighter. A protected-history append adds the handed-off `prepareSelection` and `hashSource` guards at 112 and 64 bytes, producing 102/104/102/104 selected frames. The policy pins bind that append; fresh combined measurements and reviews are still required. The actual-byte diagnostic emission and short authored FAT-root corrections are preserved. No combined firmware, device behavior or latency acceptance is claimed by this source merge.

## Measured combined selection-reuse cost adoption

Four output-clean builds of the normal ancestry-preserving combined source
`91e977e16ffe72f45460e4aeafd925d16ef7bb6e` compiled and linked, then correctly
failed only the preceding navigation flash ceilings. Independent correctness
and maintainability reviews rehashed every live/archive output (343 per
variant), checked complete compiler/configuration/procedure/input identities,
and rederived ELF/map/static-frame/partition measurements before this adoption.

| Variant | Exact combined flash ceiling | Growth from navigation | Shared SRAM |
| --- | ---: | ---: | ---: |
| default | 1,295,968 | 528 | 133,264 |
| dev-usb | 1,314,688 | 480 | 134,080 |
| regional | 1,299,776 | 528 | 133,776 |
| regional-dev-usb | 1,318,512 | 480 | 134,592 |

Ceilings equal actual combined images without slack. RTC remains 300 bytes;
partition capacity, reserve and every RAM/selected-stack limit remain unchanged.
All 102/104 selected guards are static, including selection preparation at 112
bytes and source hashing at 64 bytes. The full prior flash and protected audit
histories remain intact; no protected-history pin changes are needed for this
flash-only adoption. Minimum usable headroom is 5,697,936 bytes.

Baseline provenance binds the combined source and its actual per-variant input
hashes. Sibling branch reports/baseline remain historical observations and are
not summed as combined measurements. These raw costs establish no admission,
physical timing or installation. A fresh complete canonical gate, final source/
output review and actual merged-source Linux/Mac coverage remain required.

## Generic Home title resource tightening

Fresh output-clean measurements at `ee9a3755484b324eb904a9782cd805d3ff45a566`
remove the dynamic-title whole-label shortcut while preserving the glyph atlas,
kerning, Home shell and all reader/storage behavior. The two title-face stamp
records and their payloads are no longer compiled into device resources. All
four images are 4,912 bytes smaller; this update tightens flash ceilings to
those exact measured values without adding slack.

| Variant | Previous ceiling | Measured new ceiling | Reduction |
| --- | ---: | ---: | ---: |
| default | 1,295,968 | 1,291,056 | 4,912 |
| dev-usb | 1,314,688 | 1,309,776 | 4,912 |
| regional | 1,299,776 | 1,294,864 | 4,912 |
| regional-dev-usb | 1,318,512 | 1,313,600 | 4,912 |

RAM remains 133,264 / 134,080 / 133,776 / 134,592 bytes, RTC remains 300,
and all 102/104 selected stack ceilings, partition/reserve and protected-history
pins remain unchanged. The complete 343-member output set per variant and all
core outputs were copied with 2,744 live/archive hash comparisons. The flash
audit binds the exact raw source, input/report hashes and core artifacts.

Independent correctness and maintainability reviews verify the cost and this
tightening. Raw measurements and a deliberately stopped preliminary host run
do not confer canonical acceptance. Final committed-source host, selected
safety and all four clean native variants remain required, followed by the
actual-main Linux/Mac coverage. No individual-book performance or physical
acceptance follows from the title-rendering correction.

The baseline's top-level `provenance` retains the historical `91e977e`
source/input binding for the unchanged RAM and selected-stack limits. It is
historical metadata, not current flash evidence or source admission. The latest
`flash_policy.updates` entry separately binds the new `ee9a375` four-variant
flash measurements. Final source admission must come from the fresh gate.


### Combined generic Home, warm verifier and Books recovery cost

The reviewed runtime bundle `5c82505` contributes generic temporary verifier
scratch batching, explicit Books browsing while exact recovery is pending, and
historical 48 KiB lease fixtures. These mechanisms are integrated with main's
heap-staged two-slot selection and the generic Home title correction. A tail
overlap guard also protects the newer owned selection allocation. Full-SHA book
identity, strict admission, fresh consumed-block authentication, durable formats,
startup NVS guards, sleep ownership and existing allocation/reserve bounds remain.

The clean implementation measurement source is
`187af07ed38bc0fb25a475783bc3101ed4fd7090`. Its tree is identical to subsequent
ancestry merge `9b2a3f1a338bea6ad5590d71bde19c3dd69a0ac5`, which includes merged
main `0732a7e`. Neither commit is relabeled as the later resource-adoption source.
Four fresh clean builds measure flash 1,292,464 / 1,311,136 / 1,296,272 / 1,314,944
bytes: +1,408 / +1,360 / +1,408 / +1,344 over the exact title-only ceilings, with no
slack. Every image remains smaller than pre-title main `9e3f040`. The producer
returns only the explained old flash-ceiling failures; these are raw cost
observations without release identities, not a canonical acceptance PASS.

Actual linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes and RTC is 300
bytes. Every inherited 102/104 selected static ceiling is retained. Four new
measurable warm helper guards add exact static caps per variant: cached bind16,
bindWarm32, selectAndVerify112 and selectionScratch32 bytes. The protected audit
appends this addition and explicitly advances its source-policy pins; all earlier
history, tighter values, original anchor and partition reserve remain unchanged.

The external raw ledger binds all 1,372 output files with 2,744 live/archive hash
comparisons, core16 images/ELFs/maps/partitions, and each freshly validated report.
The independently reviewed adoption changes only the four exact flash ceilings,
four new helper guards per variant, their appended audits, explicit protected
history pins and this evidence record. Historical RAM/stack provenance remains
historical; this new helper/flash audit binds the implementation measurements.
Fresh final committed-source host, selected-safety and four clean native phases
remain required after adoption. Host source-call reductions, focused parity and
routing reproductions establish no physical speed or actual save-failure cause.
The runtime owner alone performs the separately bound install and device trial.


### Unbuffered SD library source cost

Commit `3314f99` replaces the Arduino VFS `FILE` behind
library book sources with `SdSourceFileImpl`, an `fs::FileImpl` over a
read-only POSIX descriptor (see [Decision 049](049-sd-source-descriptor.md)).
The resident 4 KiB stdio buffer had pushed saved Read below the unchanged
reader admission requirement on the owner's X3.

Four fresh clean builds of that commit measure flash 1,293,952 / 1,312,608 /
1,297,760 / 1,316,432 bytes: +1,488 / +1,472 / +1,488 / +1,488 over the warm
verifier ceilings, with no slack. Each build fails only the old flash ceiling.
Actual linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes.
These are unchanged from the inherited ceilings.

`FileBookSource<fs::File>` and the development `ReaderProfileFile` keep their
types. Their selected stack keys and every other selected limit are therefore
retained unchanged. No protected history, anchor, reserve or exclusion changes.
The `flash_policy.updates` entry binds the four reports and their image, ELF,
map and partition hashes. The final committed-source host gate and the four
clean native phases remain required after adoption. Device admission evidence
is the separately recorded preserving install in Decision 049.


### Per-action SD checks and lent action storage cost

Commit `5a483d5` adds three things (see
[Decision 050](050-reader-action-storage.md)):

- per-action FAT path checks;
- caller-lent action storage with retained verified blocks;
- the two-name verified cache-absence memo.

Four fresh clean builds of that commit measure flash 1,296,560 / 1,315,264 / 1,300,368 / 1,319,072 bytes: +2,608 / +2,656 / +2,608 / +2,640 over the
SD-source ceilings, with no slack. Each build fails only the old flash ceiling.
Actual linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes, unchanged.

The protected audit appends exact static guards for eleven out-of-line frames
these mechanisms introduce or newly separate, with the same size in every
variant:

- `repaint` 48, `rejectChangedReceipt` 16, `loadFreshProjection` 48 and
  `loadFreshIndex` 48;
- `copyRetained` 64 and `retainBlock` 128;
- `FilePort::release` 304 and `finishDeferredPathChecks` 16;
- `SampleReader::paint` 64, `makeSession` 160 and
  `SdChapterCacheStorage::fileSize` 32.

It also tightens eight affected inherited ceilings to their measurements:

| Frame | Ceiling (bytes) |
| --- | --- |
| `open` | 96 -> 64 |
| `strictOpen` | 128 -> 80 |
| `prepareOverview` | 64 -> 48 |
| `prepareSemanticOverview` | 320 -> 304 |
| `FilePort::stable` | 192 -> 128 |
| `ImportedRevision::load` | 224 -> 192 |
| `verifyBlock` | 112 -> 96 |
| `ImportedRevision::readAt` | 48 -> 32 |

Action lambdas are forced inline so that each selected public frame measures its
complete body. The protected history pins advance explicitly. Earlier entries,
tighter values, the anchor, the reserve and the exclusions remain unchanged.

These records bind the implementation measurements. Because the record commit
changes policy inputs, the final committed-source host gate and the four clean
native phases remain required after adoption. Host counts and the one-device X3
timings in Decision 050 are kept separate from these resource facts.


### SD reading-state cost

Commit `e1dfe12` moves reading state to SD (see
[Decision 051](051-sd-reading-state.md)). It adds the `SdReadingState`
backend:

- fixed 4 KiB files;
- two CRC-32C generation copies per key;
- explicit-record `O_EXCL` temp creation;
- damaged-file set-aside;
- verbatim legacy NVS migration.

It also adds fail-closed routing in `NvsProgressBackend`, and the
card-unavailable sleep policy with one retry.

Four fresh clean builds of that commit measure flash 1,302,640 / 1,321,312 /
1,306,432 / 1,325,136 bytes. That is +6,080 / +6,048 / +6,064 / +6,064 over the
action-storage ceilings, with no slack. Each build fails only the old flash
ceiling. Linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes, unchanged.

The companion protected audit adds exact guards for seven of the backend's
out-of-line frames, identical in every variant:

- `SdReadingState::begin` 48;
- `Active::open` 640 and `Active::create` 624;
- `Active::write` 320 and `Active::read` 320;
- `Active::newest` 64 and `Active::directory` 144.

The save path reaches these frames through a virtual call, which no existing
key measured. Every other selected limit is unchanged. Earlier entries, the
anchor, the reserve and the exclusions remain unchanged; the history pins
advance explicitly. The final committed-source host gate and the four clean
native phases remain required after adoption. The host contract, mutation
results and one-device X3 evidence stay in Decision 051.

### Round 2 reader and Home cost

Commits `ea019be` through `1d5e298` add round 2:

- the dev-only cache purge (dev-usb variants);
- Overlay frosted Home, the quiet cursor island and one device refresh policy
  ([Decision 055](055-overlay-home-refresh-policy.md));
- the cold-open opening screen;
- the warm-admission session credit
  ([Decision 053](053-warm-reopen-memory.md));
- staged validation Phase A, with per-chapter failure pages and the
  compressed-input bound ([Decision 054](054-staged-validation-phase-a.md));
- the fixes from the fresh adversarial and maintainability reviews.

Four fresh clean builds of `1d5e298` measure flash 1,306,496 / 1,326,208 /
1,310,736 / 1,330,416 bytes. That is +3,856 / +4,896 / +4,304 / +5,280 over the
SD reading-state ceilings, with no slack. Each build fails only the old flash
ceiling. Linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes, unchanged.

The companion protected audit tightens every selected frame that now measures
below its ceiling:

- `verifyStreams` (`constprop`) 448 → 64. Phase A moved its body into the
  out-of-line `inspect` and `RequiredMembers::admit` frames.
- `ReaderSemanticMetadata::load` 272 → 208 and `save` 288 → 272.
- `dispatchAction` 1008 → 992 and `traceReaderDetails` 272 → 256 (dev-usb
  variants).

It adds exact guards for the new or changed out-of-line frames:

- `inspect` (`constprop`) 400 and `RequiredMembers::admit` (`constprop`) 96.
  GCC names both clones by their parameters only. A later `constprop` clone in
  `epub_admission.cpp` with the same parameter list would share the key, which
  keeps the larger frame;
- `expandMember` 160 and `RequiredMembers::directory` 48;
- `CanonicalBook::open` 592 (its new signature);
- `LibraryReaderSelection::receiptPresent` 432 and `readerHeapRequirement` 32;
- `SampleReader::locateExternal` 48, `leaveFailedChapter` 112,
  `noteChapterFailure` 16 and `renderChapterFailure` 80;
- `driveFromInverse` 16 and `BookOpenControl::restart` 16;
- `purgeDevCaches` 352 and `purgeBelow` 144 (dev-usb variants);
- `paintFrostedHome` 80 and `paintOverlayHome` 64 (regional variants).

RAM and every other selected limit are unchanged; nothing is raised or
removed. Earlier entries, the anchor, the reserve and the exclusions remain
unchanged; the history pins advance explicitly. Individual frames do not prove
whole-task stack safety. The reader-task high-water on a cold open is device
evidence in Decision 054. The final committed-source host gate and the four
clean native phases remain required after adoption.

### Card reliability and recovery cost (rounds 4 and 5)

Round 4 (`32fa599` through `8893d0b`; the card swap, the missing card and the
labelled estimate) is recorded in `a1787ae`, with its measurements in
[Decision 056](056-reader-reliability.md).

Round 5 (`6699538`, `10b3241`, `656155b`, `48175d4` and `04d9b69`) fixes two
recovery defects reproduced on `246f805` and applies four rounds of fresh
reviews:

- a book held from a wake, or across a card change, stays unconfirmed until a
  card's selection names it;
- a wake into the chosen sample resumes it;
- a failed Books open rolls back every trust flag and derives a cleared or
  refused message again.

Four fresh clean builds of `04d9b69` measure flash 1,311,728 / 1,331,488 /
1,315,968 / 1,335,712 bytes, the same as `48175d4`: `04d9b69` changes only
firmware comments. That is +400 / +400 / +416 / +400 over the
round-4 ceilings, with no slack. Each build fails only the old flash ceiling.
Linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes, unchanged.

The companion protected audit adds an exact guard for the new out-of-line
comparison, `LibraryReaderSelection::checkSelection`, at 192 in every
variant. It tightens nothing, because no selected frame measures below its
ceiling. The dev-usb `dispatchAction` frame measures 992, its ceiling:

- `6699538` measured 1,008, over the ceiling, and was not kept.
- `10b3241` packs `openBook`'s six-flag rollback snapshot into one byte,
  which brings the frame to 976.
- `48175d4` restores the shown message on a failed open, which spends those
  16 bytes again.

RAM and every other selected limit are unchanged; nothing is raised or
removed. The history pins advance explicitly.

The round-5 pulled-card fix (`9c7e008`, with review fixes and a test in
`6e31612`) lets a sample chosen after the booted card is pulled open and stay
open (host evidence; device evidence is in Decision 056). Four fresh clean
builds of `6e31612` measure flash 1,311,840 / 1,331,600 / 1,316,080 /
1,335,824 bytes, +112 in every variant over the `04d9b69` ceilings, with no
slack. Each build fails only the old flash ceiling. Linked SRAM is
unchanged. `6e31612` changes only firmware comments and a parameter name after
`9c7e008`, and measures the same.

The companion protected audit:

- tightens `LibraryReaderSelection::resumeAfterCard` from 192 to 176 in
  every variant. Its "no card answered" test is now out of line; that this
  causes the drop is inferred, not traced in the older build;
- guards that test, `LibraryReaderSelection::cardSilent`, at 16 in every
  variant.

After a failed read, the new presence check runs `resumeAfterCard` 176 +
`cardSilent` 16 + `cardAbsent` 16 + `SdLibrary::present` 112 before the
filesystem stat (from `checkSelection`, 192 + 144). That runs after the
durable selection load returns, not inside it. The stat below it is not
measured, as for the existing `present()` callers. `checkSelection` measures
its 192 ceiling, unchanged. Nothing is raised or removed, and the history pins
advance explicitly.

**Line exception.** `firmware/library_reader.h` measures 537 lines against
the 500-line production threshold (496 at `246f805`). Its exact path ceiling
is 537, with no headroom. It was 512 at `6699538`, 519 at `10b3241`, 522 at
`04d9b69` and 529 at `6e31612`.

- **What the lines are.** Every line added from 496 to 529 belongs to the
  selection trust state machine or the sample-wake branch of `restoreOnBoot`:
  the repeatable comparison (`checkSelection`), the "no card answered" test
  (`cardSilent`), the third trust bit and its triggers, the refusal literal,
  the packed rollback snapshot and the sample-wake return. The Home cover (#19,
  Decision 057) adds 8: the include of `home_cover_signal.h`, the change of
  Home's cover generation when Read finds a stale held file (its own line),
  and `cardAbsent`, which now changes it too and is written out over six
  lines under a two-line comment.
- **Why not split it.** Splitting that state machine across two classes would
  hide which code owns the trust bits. The cover's changes belong where the
  selection learns that the card may have changed; storage stays pure
  (Decision 057).
- **Why not squeeze it.** A round-6 delta review rejected fitting the cover's
  lines under 529 by collapsing `sourceLive` to one line and putting two
  statements on a line. `sourceLive` keeps its `f008308` form.
- **How to retire it.** The stateless `hash()` (35 lines) could move to a free
  function in its own firmware header, which would take the file to about
  502 lines; a further cohesive move would then retire the exception. Its
  selected stack key (`LibraryReaderSelection::hash`, 1,216; dev-usb 1,232)
  would have to move to the new key at its measured size. Two checks reject a
  removed selected key. The resource verifier
  (`tools/code_health_resources.py`) fails with
  `Missing selected stack measurement`, and the flash policy's protected
  history (`tools/code_health_flash_policy.py`) refuses an update that removes
  a selected entry. So that move needs its own reviewed policy change, which
  the Home cover does not make.
- **Review decision.** The round-5 maintainability reviews accepted the
  exception: r1 at `10b3241` and r2 at `656155b`. The combined review of
  `48175d4` found the flash and frame cost within policy. The pulled-card
  maintainability review of `9c7e008` accepted the further lines: one
  predicate in the class that owns both of its inputs. The Home cover's 8
  lines (529 to 537) were accepted by claude_p19_r7_delta, the round-7 delta
  review of an earlier build of this series whose fixes are folded into these
  commits: permitted under the path-exception rule (lines 172-177); a plain
  `hash()` move would remove a selected stack key, and a forwarding wrapper
  that only keeps a key alive is not a simplification.

The final committed-source host gate and the four clean native phases remain
required after adoption.

### Bounded chapter styling cost

The bounded EPUB styling port (draft PR #18, onto `f008308`) adds matching
reading faces and one chapter rule slot. Its design and evidence limits are in
[the styled-layout profile](reader-styled-layout-plan.md).

**Adapter exception.** `include/newt/reader_styles.h` gains one direct SDK
adapter allowance, `css/Css.h`, in both the code and baseline policy.

- **Why it is needed.** The single rule slot owns up to 64 SDK `CssRule` values
  and the `CssStylesheet` view that layout receives through
  `LayoutParams::stylesheet`. Both are complete types only in `css/Css.h`.
  No existing allowance reaches that header: `FreeInkBook.h` does not include
  it, and `layout/ChapterLayout.h`, which does, is allowed only in
  `src/phased_reader.cpp` and `src/reader.cpp`.
- **Why not avoid it.** Any design needs exactly one `css/Css.h` allowance,
  because `src/reader_styles.cpp` calls `CssStylesheetBuilder`; the only
  choice is which file holds it. Raw slot bytes would move the allowance to
  that source and add launder/reinterpret code for the rules and the view.
  An opaque owner would need a second heap allocation, failure path and
  preflight accounting. Widening an existing adapter header would hand the
  CSS types to an unrelated adapter. The header follows the existing
  `reader_font.h` to `BookFont.h` pattern.
- **Scope.** One header in one file. The new styling sources reach the rest of
  the SDK only through existing adapters (`newt/canonical_book.h`,
  `newt/epub_admission.h`). The source draft also allowed three headers for
  `src/reader_styles.cpp`; this port needs none of them.
- **Review decision.** Accepted by claude_p18_maintainability_r1 on a
  pre-rebase draft of this port; the allowance is unchanged since.

**Heap.** Styles add no allocation. Between passes the 1,812-byte rule slot
and its 4-byte integrity seal sit at the top of the existing shared workspace
block and bound the shared arena below them. A layout pass needs that whole
block, so the slot and the pass's 48-byte `ReaderStyleWatch` are lent to the
pass at the top of the parse block, above layout's fixed parse demand.
Resolve's 108-byte record (112 on hosts) sits at the top of the parse block,
and its embedded text, at most 4 KiB, just below the slot's home, from resolve
until build. Resolve allocates only its member-name directory and one path
(`ReaderStyles::kResolvePathBytes`) from the parse block. No Selection or stack
frame holds style state. The selection allocation request keeps its `f008308`
size, 3,571 bytes on C3 and 3,591 on hosts, so external-reader admission still
requires 128,292 bytes. Allocation requests do not establish runtime heap
peaks.

**Source size.** The style wiring would take `src/phased_reader.cpp` past the
500-line limit. The semantic progress attachment and projection first moved
unchanged to `src/phased_reader_semantics.cpp`, leaving `src/phased_reader.cpp`
at 464 lines; with styles `src/phased_reader.cpp` is 491 lines.
Do not join lines to fit more; the next extraction candidate
is the `SelectingSink` and captured-paint block. The safety profile selects
all three new reader units.

**Stack.** `PhasedReader::select` keeps its 128-byte frame: the Book's view
reaches `resolveStyles` as scalars, and `buildStyles` schedules its own reads,
so `select` holds no style temporary. `PhasedReader::layout` measures 448
bytes, 16 below its 464 ceiling, and is tightened. `ChapterPageCache::build`
keeps its 240 ceiling only because the writer's restart begins the cache file
through the out-of-line `beginWriting` (32 bytes); with that call inlined in
the restart loop, the store and index addresses stayed live across both passes
and the frame measured 256. `beginWriting` returns before the writer's pass
starts, so it deepens no chain. The watch, the slot and resolve's record live
in the workspace blocks, never in a frame.

The new reader-task chains, measured with the pinned C3 compiler in all four
variants (identical frames):

- Resolve under `select`: `layout` 448 + `select` 128 + `resolveStyles` 64 +
  `ReaderStyles::resolve` 112 (head scan inlined), then the SDK SAX parser and
  the 96-byte `SourceScan::onStartElement`. Build: `layout` 448 + `select` 128
  + `buildStyles` 112 + `ReaderStyles::build` 144, then member inflation and
  `addText` 48. A Book-free recapture runs `ReaderStyles::resolve` below
  `buildStyles` instead. A stopped Deferred pass reaches the same calls through
  the 64-byte `settleStyles` instead of `select`.
- Watch: `ReaderStyleWatch::readAt` (32 bytes) sits between the SDK's chapter
  reader and `ChapterSource::readAt` on layout's decode chain. Its lexer chain
  (`step` 16 + `startElement` 16 + `is` 32) runs only after that inner read
  has returned, so the inflate chain below it is not deepened.
- Cache writer: `ChapterPageCache::build` 240 + `streamChapter` 384 + `select`
  128, then the resolve and build chains above, or `settleStyles` and those
  chains after a stop. The firmware links this chain through `show`'s
  `buildOnMiss` branch but always passes false; only hosts build caches.
- Warm and restart reselection: `RevisionImportSession::selectAndVerify` 112 +
  `ChapterPageCache::stored` 176 (its 128-byte cache name) + the existing
  `identity` 224, which returns before `select` runs, as in `show`.
- SDK cascade: with a stylesheet attached, the pinned SDK's
  `LayoutEngine::onStartElement` (128 bytes) calls `cascadeFor` (80 bytes)
  below Expat's 128-byte `doContent`. From `ChapterLayoutSession::step`, that
  start-element chain measures 48 + 32 + 32 + 16 + 128 + 128 + 80 = 464 bytes
  (`step`, `feedChunk`, `XML_Parse`, `contentProcessor`, `doContent`,
  `onStartElement`, `cascadeFor`). The existing end-element chain through
  `LayoutEngine::flushParagraph` (112) and `placeLine` (1,536) stays far
  deeper.

The resource collector reads only first-party `.su` files, so SDK frames such as
`cascadeFor` and `LayoutEngine::onStartElement` have no guards; the numbers
above are measurements, not ceilings. Individual frames do not prove whole-task
stack safety.

**Flash record.** Four fresh clean `--health-report` builds of `9afb1ad`
measure flash 1,439,440 / 1,459,200 / 1,443,680 / 1,463,424 bytes, +127,600 in
every variant over the `6e31612` ceilings, with no slack. Each build fails only
the old flash ceiling. Linked SRAM is unchanged. The styled face tables account
for 116,982 bytes: bold, italic and bold-italic bitmaps, glyph and kerning
tables, and the 682-byte kerning class map the three share. The three style
objects link 9,546 bytes of code and constants (`reader_styles` 6,055,
`phased_reader_styles` 1,786, `reader_style_watch` 1,705) and 2,464 bytes of
unwind and exception tables. That split comes from the default variant's linker
map, not from separate builds. The map charges each shared inline function to
the first object that links it, so the object totals are upper bounds: together
the tables and objects exceed the measured growth by 1,392 bytes. Face
selection, cache-route and reader wiring changes are not attributed separately.
The largest image leaves 5,553,024 bytes of the 7,016,448-byte usable partition.
A pre-rebase draft of this port measured +126,608; `768e44b` measured 880 bytes
more, which is not attributed further. The repeated-head scan and the watch's
bulk text skip (`303691a`, `9afb1ad`) add 112 bytes in every variant; every
frame is unchanged.

The companion protected audit tightens `PhasedReader::layout` from 464 to 448
in every variant. `PhasedReader::select` 128, `ChapterPageCache::streamChapter`
384, `ChapterPageCache::build` 240 and `ReaderNavigation::bind` 96 are
unchanged. It adds 33 guards at the measured sizes of the new reader-task style
frames, identical in every variant:

- `PhasedReader::buildStyles` 112, `resolveStyles` 64, `settleStyles` 64,
  `deferStyles` 48, `lendStyles` 32, `watchStyles` 32, `styleSlot` 16,
  `stylesReady` 16 and `resetArenas` 0, the slot seal `sealOf` 32 and the
  `ReaderStyleSlot` constructor 16;
- `ReaderStyles::build` 144, `resolve` 112, `declaresStylesheet` 32 and
  `limitNesting` 0;
- the SAX handlers `SourceScan::onStartElement` 96, `onText` 16 and
  `onEndElement` 16, and `FaultTrack::damaged` 32 and `readAt` 16;
- `addText` 48, `token` 32, `attr` 16, `optionalValue` 16 and the shared
  `xmlLocalIs` 16 and `xmlLocalName` 16;
- `ReaderStyleWatch::readAt` 32, `is` 32, `step` 16, `startElement` 16 and
  `endElement` 16;
- `ChapterPageCache::stored` 176 and `beginWriting` 32.

The chains these frames form are listed under **Stack** above. Nothing is
raised or removed, and the history pins advance explicitly. Review:
claude_p18_adversarial_r1 and claude_p18_maintainability_r1 reviewed a
pre-rebase draft of this port, and their round-2 findings are applied here.
A fresh adversarial and maintainability review of `768e44b` and its record
(sol_p18_final) found no blocker; its repeated-head finding is fixed in
`303691a`. A delta review of `303691a` and `9afb1ad` (sol_p18_delta) found no
defect.

### Home cover cost

The Home cover (#19, Decision 057: `fbb08ca` to `a95fa9f`) shows the selected
book's prepared NLC2 cover on Home through the shared canonical painter.
`3388b53` reads it in one pass through `PreparedLibraryCovers::readAll` and
paints the slot in whole bytes. `95cfc4e` holds it for the Home session (one
read; focus, battery and forced paints never read) and remembers the cover
book facts for the book key: once they are learned a Home entry stats no book,
and a cover file the card says does not exist (`ENOENT`) makes no card access.
An instrumented regional DEV USB build of an earlier state of this series
measured the book stat it removes at 29.3 to 30.0 ms on every Home entry
(Decision 057, Performance).

**Flash record.** Four fresh clean `--health-report` builds of `a95fa9f`
measure flash 1,441,216 / 1,460,960 / 1,445,440 / 1,465,184 bytes. That is
+1,776 / +1,760 / +1,760 / +1,760 over #18's `9afb1ad` ceilings (1,439,440 /
1,459,200 / 1,443,680 / 1,463,424), with no slack. Each build fails only the
old flash ceiling. The largest image leaves 5,551,264 bytes of the
7,016,448-byte usable partition. The growth was attributed by measurement on
`f008308`, before this series was restacked onto #18, where the same change
cost +1,792 / +1,776 / +1,760 / +1,776 over the `6e31612` ceilings, in stages
of the earlier series:

- the cover before review: +1,344 / +1,328 / +1,344 / +1,328;
- the session memo, `readAll` and the byte painter: +144 / +144 / +128 / +144;
- the exported eligibility and named read: nothing;
- the remembered book facts: +304 / +304 / +288 / +304. That includes
  remembering only `ENOENT` and the selection's generation change for a
  missing card. Books reads no held file. A measured build that did used
  16 / 16 / 32 / 16 more flash. Its Library open paint was 4 to 6 ms above
  base `0184bbb` in the same window. The read runs in `openLibrary()`, before
  `paint()` starts the timed interval (`refreshMs`), so it is not the direct
  cause of the measured paint gap, and that gap's cause was not isolated. The
  read was removed on its merits: it added a card read to every Books entry
  after reading, for decoration only.

On #18 the default, DEV USB and regional DEV USB growth is 16 bytes less than
that sum and the regional growth is equal; the difference is not attributed
further. The probe and documentation corrections after the last review change
one firmware comment and no compiled code.

**RAM.** Linked SRAM is 133,248 / 134,064 / 133,760 / 134,576 bytes, 16 below
the unchanged ceilings in every variant, as it was on `f008308`.
`HomeRefreshPolicy`'s smaller clock cache pays for the 4-byte cover generation
and the 48-byte `HomeCoverRuntime`; the remembered facts take the runtime from
32 to 48 bytes, 16 more static RAM in every variant than without them. The RAM
ceilings are deliberately not tightened. The cover producer (#20), the next
layer, is not yet measured on this one, and stacking can shift alignment, so
tightening here could force a raise there, which the protected history
forbids. The 16 bytes stay available within the old ceilings, as the rule
above allows, and the final stacked layer tightens all four variants to the
measured values.

**Stack.** With a per-paint RAII cover lease, an earlier state of this series
measured the selected `paint` frame at 96 / 112 / 112 bytes in the first three
variants, over its 80 / 96 / 96 ceilings: the lease added an unwinding cleanup
to it. `95cfc4e` gives `paintHome` a single exit with no destructor, and
`paint` measures 80 / 96 / 80 / 96. The companion protected audit:

- keeps static RAM at 133,264 / 134,080 / 133,776 / 134,592 bytes;
- tightens the regional `paint` ceiling from 96 to 80 and the regional DEV
  USB ceiling from 112 to 96; every other selected frame, #18's style guards
  included, is unchanged;
- guards the new or newly reachable out-of-line Home cover frames that #19
  owns at their exact measured sizes, identical in every variant:
  `observeHomeCover` 32, `prepareSelectedHomeCover` 48, `readHomeCoverOnce`
  16, `prepareHomeCover` 16, `PreparedLibraryCovers::begin` 384,
  `PreparedLibraryCovers::learn` 16, `PreparedLibraryCovers::readAll` 16 and
  `renderFrostedHome` 496. `releaseHomeCover` (16) and `renderHomePlane` (0)
  are left unguarded: neither reads the card, and `renderHomePlane` only calls
  the guarded `renderFrostedHome`.

`begin` owns one scratch path and one `stat` for the book and its cover, which
`learn` and the cover open share. The deepest cover chain is regional DEV USB:
`paint` 96, `paintFrostedHome` 80, `paintOverlayHome` 64, `readHomeCoverOnce`
16, `prepareSelectedHomeCover` 48, `prepareHomeCover` 16, `begin` 384 and
`learn` 16, 720 bytes in all (704 in the regional build). That excludes the
file system's own frames, which are not measured. No RAM or stack ceiling is
raised or removed, and the history pins advance explicitly. Individual frames
do not prove whole-task stack safety.

`95cfc4e` also takes `firmware/library_reader.h` from 529 to 537 lines, and
its exact path ceiling with it. The line exception above names the 8 lines,
why the file is not squeezed under 529, and why moving `hash()` is left to its
own reviewed policy change.

**Review.** Both records name the reviews of earlier states of this series,
whose findings are folded into `3388b53`, `95cfc4e` and `a95fa9f`: the round-2
runtime and contracts reviews, the round-3 and round-4 adversarial and
maintainability reviews, the round-5 delta review, which found no firmware
defects, and the round-6 and round-7 delta reviews, which found no blockers;
the round-7 review accepted the line exception. sol_p19_final, an adversarial
and maintainability review of this series restacked on an earlier head of #18,
found no blocker. Its nit, that the paint probe rejected harmless comments and
depended on the release line's comment, is fixed in `95cfc4e`: the probe
strips line and block comments and literals, matches only identifiers, and
finds the release statement by its code. Its should-fix, that the remembered
facts compare the identity record by its 32-bit checksum alone, is kept as a
documented limit in Decision 057, beside the same-name limitation: a collision
between two files of one book can show the placeholder until the book key
changes or the device restarts, decoration only, and a fix needs more static
RAM or another line past `library_reader.h`'s exception. The records bind the
original failing reports and do not claim a fresh gate for the budget-edit
commit.

### Automatic cover producer cost

The automatic cover producer (#20, Decision 059: `1071fe1` to `ce4360e`)
generates Library and Home covers while those screens are idle. It is stacked
on #19's final state (`0e3ff5f`, whose last record is `4730a0d`).

**Flash record.** Four fresh clean `--health-report` builds of `ce4360e`
measure flash 1,470,928 / 1,490,768 / 1,475,152 / 1,494,992 bytes (default /
dev-usb / regional / regional-dev-usb). That is +29,712 / +29,808 / +29,712 /
+29,808 over #19's `4730a0d` ceilings (1,441,216 / 1,460,960 / 1,445,440 /
1,465,184), with no slack. Each build fails only the old flash ceiling. The
largest image leaves 5,521,456 bytes of the 7,016,448-byte usable partition.
Clean builds of each stage's last commit on this base attribute the growth:

| Stage | default | dev-usb | regional | regional-dev-usb |
| --- | ---: | ---: | ---: | ---: |
| Phase 2 producer (to `0b9c0e6`) | 27,408 | 27,488 | 27,408 | 27,472 |
| Read path, restarts and writes (to `833d370`) | 1,552 | 1,568 | 1,552 | 1,584 |
| Review fixes (to `96c3634`) | 144 | 144 | 144 | 128 |
| Per-version interruption key (`13ff713`) | 16 | 16 | 16 | 16 |
| Unbound stops charge no book (`34a1e3d`) | -32 | -32 | -32 | -16 |
| Inline Home identity check (`cb37b3b`) | 0 | 0 | 0 | 0 |
| Home no-art memory (`dd1bfcd`) | 64 | 64 | 48 | 48 |
| Path polls and cache probe (`2186b90`) | 496 | 480 | 496 | 496 |
| Settle period (`8f7e281`) | 96 | 96 | 96 | 80 |
| Every unbound stop charged (`ce4360e`) | -32 | -16 | -16 | 0 |
| Total | 29,712 | 29,808 | 29,712 | 29,808 |

- Phase 2 is bounded ZIP, container and OPF discovery with staging to the
  card; the MIT-attributed baseline JPEG route; verified NLC2 art and no-art
  publication with the session fault latch; and the idle tick with its start
  admission. TJpgDec's `jd_prepare` and `jd_decomp` are now linked. pngle,
  `ImageRenderer` and the progressive decoder still have no linked text in any
  variant, and `probeImage` stays linked for EPUB admission as before.
- The read path is the resume record and its key, the back-off and the resume
  block's lifetime, `CoverReadAhead` with its fit check, and seek-and-read in
  place of the pinned FAT VFS's chain-walking `pread`.
- In the default map the review fixes add 120 bytes of text:
  `cover_idle_detail::noteInterruption` +92 (the per-book count, a changed
  version's new count and the seventh interruption setting the book aside),
  `serviceIdleCovers` +30 (the waiting book's gate and the per-book reset,
  inlined) and `coverFnvName` 30, now shared out of line by `coverResumeKey`
  (-18) and the view identity. `AutomaticLibraryCovers::attempt` is 10 bytes
  smaller and `milliseconds` (4) is inlined.
- The per-version key adds 18 bytes of text to `noteInterruption`.
- Charging no book for an unbound stop removes 24 bytes of text from
  `noteInterruption` (its branch for a keyless record) and adds 12 to
  `AutomaticLibraryCovers::attempt`, where `Session::run` now clears the key.
- Inlining `homeCoverIdentity` removes its 48-byte copy and outlines
  `LibraryReaderSelection::retain` (48) instead; the image size is unchanged.
- The no-art memory adds 48 bytes of text to `PreparedLibraryCovers::begin`.
- The path polls, held temporaries and painter-like cache probe add 400
  bytes of text: `Session::xml` +106 (polled removal and ownership of its
  staged copies), `Session::ensureDirectory` 114 now out of line against
  `Session::attempt` -30, `cached` +46, `Session::commit` +36,
  `coverRemoveTemporary` +30, `CoverFileSource::open` +26,
  `Session::prepareFinal` +24, `stillBound` +22, `stage` +18 (cleanup after a
  failed exclusive create) and `AutomaticLibraryCovers::attempt` +8.
- The settle period adds 90 bytes of text: `serviceIdleCovers` +56 (the gate,
  the quiet clock on screen and view changes and Home's held-art check),
  `noteInterruption` +22 (the failure for a stop that made no progress) and
  `loop()` +12 (the press hook).
- Charging every unbound stop, with the last budgeted attempt paced,
  removes 16 bytes of text: `noteInterruption` -10 (no settled-slot test)
  and `serviceIdleCovers` -6 (+4 in regional DEV USB).

The rest is alignment. The suite split (`0b4f2af`), before the per-version
key, changes tests only, and the stale-mtime no-art tests folded into
`dd1bfcd` add no firmware. Built on an earlier state of #19, before this
series was restacked, the first three stages cost 27,456 / 27,536 / 27,472 /
27,536, 1,568 / 1,568 / 1,552 / 1,568 and 128 / 144 / 128 / 144; the
differences are not attributed further.

**RAM.** Linked SRAM is 133,264 / 134,080 / 133,776 / 134,592 bytes, exactly
the unchanged ceilings. #19 left 16 bytes below each ceiling for this layer
(see "Home cover cost"), and the producer's static state, the 16-byte
`CoverIdleState`, takes them: phase 2 (8 bytes then) adds 16 in the DEV USB
variants and nothing in the others, and the read path's larger state adds 16
in the default and regional variants. Measured and ceiling values are now
equal in every variant, so there is nothing to tighten, and no static RAM is
left below any ceiling.

**Stack.** `paint(bool, bool)` measures 80 / 96 / 80 / 96 and `loop()` 80 /
96 / 80 / 96, both at #19's ceilings: the paint overage the earlier state of
this series inherited is gone with #19's single-exit `paintHome`. On this base
GCC outlined `homeCoverIdentity` once the producer became its second caller,
and `prepareSelectedHomeCover` then kept the workspace across the call and
measured 64 against its 48-byte ceiling. `cb37b3b` makes the helper
`always_inline`; `prepareSelectedHomeCover` is 48 again, `observeHomeCover`
32 and `serviceIdleCovers` 336. The settle gate first measured
`serviceIdleCovers` at 352: `coverIdleStep` took the heap by value, and that
second 24-byte copy is gone now that it borrows the heap. `stage` keeps its
signature: recording temporary ownership through a reference measured 1,216
(one more saved register), so the session records it after `stage` returns.
The companion protected audit:

- keeps static RAM at 133,264 / 134,080 / 133,776 / 134,592 bytes;
- tightens `PreparedLibraryCovers::begin` from 384 to 368 in every variant.
  From phase 2 on it calls the shared out-of-line `libraryCoverLittle` (32)
  instead of an inlined private reader, so the measured Home cover chain is
  unchanged: regional DEV USB `paint` 96, `paintFrostedHome` 80,
  `paintOverlayHome` 64, `readHomeCoverOnce` 16, `prepareSelectedHomeCover`
  48, `prepareHomeCover` 16, `begin` 368 and `libraryCoverLittle` 32, 720
  bytes in all (704 regional), before the file system's own frames;
- guards 77 of the 78 frames that #20's earlier records proposed, at their
  exact measured sizes. Each is identical in every variant and none is larger
  than first proposed. The entry is `serviceIdleCovers` 336 (`noinline`, so
  `loop()` keeps its ceilings, with `coverIdleStep` inlined into it), then
  `AutomaticLibraryCovers::service` 96, `attempt` 176 and `cached` 960 (976
  first proposed; the probe no longer holds a book source). The
  deepest are `parseCoverOpf` 1,632, `locateCoverMember` 1,376,
  `Session::xml` 1,296, `stage` 1,200, the discovery `path` 1,104, `parse`
  1,088 and `sourceCrc` 1,072. The read path adds `CoverReadAhead::readAt`
  64, `cover_idle_detail::noteInterruption` 64, `coverResumeKey` 48,
  `Session::locate` 32 and `cover_idle_detail::forget` 16, and
  `coverRemoveTemporary` is 112. `coverRemoveTemporary`
  gained a parameter in `2186b90`, so it is guarded under its new signature;
  `noteInterruption` has its first proposed signature again. The
  `CoverFaultLatch` overrides measure 16 each and `libraryCoverLittle` 32. The
  78th, `homeCoverIdentity` (16), no longer has a frame;
- guards three new nonzero frames that the earlier records did not list:
  `CoverFiles::readAt` 32, the seek-and-read adapter;
  `LibraryReaderSelection::retain` 16, outlined once `homeCoverIdentity` is
  inlined into both callers (it is not on the deepest Home chain: it returns
  before `prepareHomeCover` is called); and `Session::ensureDirectory` 112,
  outlined by the poll before `mkdir` while `Session::attempt` stays 256.

Every other selected frame, #18's and #19's guards included, is unchanged.
Nothing is raised or removed, and the history pins advance explicitly. The
conservative RV32 component chains (deepest 5,936 bytes, Container/OPF XML
callback) are in Decision 059. Individual frames do not prove whole-task stack
safety, and a device high-water measurement is still required.

**Review.** Earlier states of this series, on an earlier state of #19, were
reviewed in three rounds, whose findings are folded into this series: the
first-round adversarial and maintainability reviews of phase 2 (applied in
`ebcd82e` and `86a23f1`), the second and third rounds of the read path
(applied in `72efe8f` and `2b06ce4`), and the coordinator's review of the
read-path record (applied in `05bd1f4`, `e16c5d4` and `96c3634`). A fresh
review of the previous state of this series and its record, which already
included the suite split, the per-version key, the inlined identity check and
the no-art memory, found no blocker. Its two should-fix findings and two nits
are folded in: an unbound stop charges no book (`34a1e3d`), the stale no-art
tests also cover a same-size record with another mtime (`dd1bfcd`), Decision
059 corrects the write-fault and Home-helper wording (`0b9c0e6`), and the
messages of `ebcd82e` and `86a23f1` are reworded to match. A fresh review of
`dd1bfcd` and its record found no blocker and one should-fix: stops before a
book was bound escaped the visit's total-work limit. `8f7e281` closes it. A
device A/B of `dd1bfcd` then found presses delayed after Home and Library
renders (Decision 059, "Settle period"); `2186b90` and `8f7e281` fix that. A
fresh review of `8f7e281` and its record found no blocker and three
should-fix findings: four stops during ordinary browsing could end generation
for the visit, alternating shelves could renew uncharged stops without limit,
and no test covered the poll before `mkdir`. `ce4360e` fixes all three. A
fresh review of that fix found no blocker or firmware defect. Its two
should-fix findings (tests of the paced attempt's verdict, and a bound stated
in charged ticks, since a refused start admission is not charged) and its nit
(host counts the path polls had changed) are folded into `ce4360e`.

These are compile measurements. The device's stack high-water for this
series is still to be measured; its device timing, and the heap of an earlier
image of this series, are in Decision 059.

## Stage owner-defect correction resource review (2026-10-11)

The owner rejected the first Stage candidate for a visibly flashing/corrupted
focus region and losing Stage across sleep/wake. Frozen implementation
`43c36227c3eb9ba259da93428cf75ac5bdc61364` replaces the gray union update with
immutable, byte-aligned binary button islands and renders the selected Home
under the sleep blur. A DEV-only versioned RTC token retains explicit opt-in
across deep sleep/software reset; cold/unrelated reset keeps Frosted default.
This is a bounded correctness change to the existing stack.

All four clean native measurements were recorded before changing any ceiling:

| Variant | App image B | Growth B | SRAM B | RTC B |
| --- | ---: | ---: | ---: | ---: |
| default | 1,473,168 | 32 | 133,248 | 300 |
| DEV USB | 1,608,720 | 752 | 134,080 | 304 |
| regional | 1,479,040 | 1,680 | 133,744 | 300 |
| regional DEV USB | 1,615,392 | 2,432 | 134,576 | 304 |

These are aggregate frozen-change costs, not separately isolated costs for the
renderer, RTC token or driver. Each image fits the unchanged 7,016,448-byte
usable app limit; the largest leaves 5,401,056 bytes. The flash audit records
all four input/report/output hashes and accepts exactly these measured costs,
without slack. Native resource gates are rerun against the final source/policy
commit; a preliminary ceiling failure is not a final gate pass.

No inherited RAM or selected stack ceiling increases or loses an entry.
Regional SRAM tightens by 16 bytes in both variants; other SRAM ceilings stay
unchanged. The old `renderStageHome` entry is a zero-frame tail wrapper and its
ceiling tightens from 320 to 0. The actual focused renderer gains an explicit
320-byte guard in every variant. Zero-frame layout adapters, the 128-byte
selected-theme sleep renderer, the 128/32-byte sleep transaction/callback, and
the 64-byte outlined DEV sleep owner gain exact native guards. Production
Stage/backdrop remain unlinked even though shared source compilation emits
selected function measurements. There is exactly one shared 365,597-byte font
atlas in each ELF and one 104,544-byte Stage backdrop in each DEV ELF.

The RTC exception is exactly four DEV-only bytes: legacy 68-byte sleep and
140-byte SD records retain their contracts and addresses; a separate aligned
word brings `.rtc_noinit` to 212 and aggregate RTC to 304 in DEV, while
production stays 208/300 and has no token. The native guard enforces exact
layout, absence in production and section costs. Independent disassembly
checks the token read, aligned full-word stores of zero/`0x4e574831`, and reset
eligibility. That proof does not establish physical RTC retention.

Per-function limits do not prove aggregate task stack safety: the outlined DEV
wrapper, selected-theme sleep renderer, callback and Home renderer coexist on
a call chain. The exact installed candidate still requires runtime high-water
measurement and physical sleep/wake and optical acceptance. The Linux gate is
required; the owner explicitly deferred the Mac ImageIO/renderer gate, which
remains untested. No owner acceptance, baseline performance approval, merge or
release follows from this source/resource review.


## Structural cover early-rejection resource review (2026-10-11)

Frozen correction `8a10faab8ae039dc3f8353200c19813675f204d9` promotes only
existing definitive ZIP/XML Unsupported policies to the large-archive early
route and requires the reopened pathname to reproduce that proof. Independent
behavior and maintainability review found no actionable defect; a CRC-valid
large-OPF parser test closes the reported integration coverage gap. Stable
content verdicts, CJB2 negatives, supported-image limits and SDK remain unchanged.

Four clean preliminary builds measured exactly 16 B of flash growth each:
1,473,184 / 1,608,736 / 1,479,056 / 1,615,408 B. Their failures against the old
exact ceilings remain recorded as failures. Independent review matched every
report/input/bin/ELF/map/partition digest and the clean frozen-source DEV IDs.
This manual exception accepts only those measured costs, without slack; the
largest image leaves 5,401,040 B below the unchanged 7,016,448 B usable limit.
All RAM budgets, all 313 / 333 / 315 / 337 inherited selected stack entries,
RTC costs (300 / 304 / 300 / 304), protected budget history, exclusions, adapters
and thresholds remain unchanged. The header retains the 500-line production
limit by shortening explanatory prose to 498 lines. Exact-source Linux and all
four clean resource gates must pass after this policy commit before flashing.

Same-input authored host comparisons read 27,268,986 versus 2,628 B for a
13,633,179 B member-limit archive and 25,177,020 versus 7,530 B for a
12,584,745 B CRC-valid XML-policy archive. Both paths persisted valid no-art
records. Those read counts are host evidence, not card timings or optical
acceptance. Final combined-image device evidence belongs in Decision 060; this resource
audit does not establish hardware or owner acceptance.

## Combined development USB transmit-tail resource correction (2026-10-11)

Clean source `94b22d7c023b99aeb18467ab8ed8fa1eec51126b` was measured with all
four authoritative clean resource builds before this manual exception. The
nonblocking pinned-SDK FIFO/IN_EMPTY re-arm costs exactly 0 / 48 / 0 / 64
additional flash bytes: 1,473,184 / 1,608,784 / 1,479,056 / 1,615,472 B.
Minimum headroom under the unchanged 7,016,448-byte usable limit is 5,400,976 B.
The preliminary DEV gates correctly failed only their inherited flash ceilings.
Core output/report/input hashes are recorded in the four-variant flash audit.

An earlier inlined proposal and a noinline void helper measured an unselected
32-byte USB write frame, versus its prior 16 bytes. They were rejected before
installation. Returning the accepted count through the nonblocking leaf restores
the write frame to 16 B; native assembly confirms the post-write tail jump and
zero-frame helper. Protected history adds those two DEV-only guards and tightens
regional DEV `paintStageHome` from 128 to its repeatedly measured 112 B.
All inherited selected entries, earlier history and tighter values remain.
The explicit compiled history pins advance with this append. SRAM and RTC remain
133,248 / 134,080 / 133,744 / 134,576 B and 300 / 304 / 300 / 304 B; NVS guards
pass. No exclusions, adapters, partition/reserve, RAM/stack increases or other
threshold changes are adopted. Individual static frames do not prove chain
usage, runtime headroom or physical sleep/wake behavior.

Independent GPT-5.6 Sol high review verified semantics, native leaf/tail behavior,
all four hash-bound costs, NVS receipts, the complete protected maps and usable
headroom. This is agent-verifiable resource review, not owner or latency-policy
acceptance. Final exact-source Linux and all four clean gates remain required
after this policy commit; device results belong in Decision 060.

## Durable experimental Home selection (2026-10-11)

The owner rejected the RTC-only Stage restoration on battery. An explicit DEV
Home choice now uses the existing verified SD A/B record workflow independently
of reading and sleep records. Five required startup lines increase the specific
`firmware/main.cpp` source ceiling from 681 to exactly 686: one explicit include
and the guarded durable-restore call. The persistence logic lives in
`firmware/development_home_preference.h`; no wrapper or compressed formatting
hides the startup seam. Fresh independent GPT-5.6 Sol high persistence and Stage
reviews approved this narrow source exception. All other source thresholds,
adapters and generated exclusions remain unchanged.

The actual firmware startup and selector source is preprocessed and compiled in
the regression harness. Removed, preprocessor-disabled and commented restore
or selector calls must fail the cold-memory persistence assertions. Tests also
cover stale RTC, explicit Frosted, mounted missing/corrupt records, unavailable
SD and acknowledgement loss. USB-powered and battery wake remain separate
physical facts; these host checks do not establish battery acceptance.

The clean measured source `97c5899213e68c4200307008dfba3d47ac84a6f8`
and all four private resource receipts establish this exact reviewed flash cost:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,473,184 | 1,473,184 | 0 | 133,248 | 300 |
| dev-usb | 1,608,784 | 1,611,040 | 2,256 | 134,080 | 304 |
| regional | 1,479,056 | 1,479,840 | 784 | 133,744 | 300 |
| regional-dev-usb | 1,615,472 | 1,619,664 | 4,192 | 134,576 | 304 |

Fresh independent GPT-5.6 Sol high resource review (Laplace) recomputed the
proposal's reports/artifact hashes and approved these exact costs. The minimum
usable flash headroom is 5,396,784 B; partition capacity/reserve stay fixed. All
inherited selected stack entries and their previous tighter limits remain.
The protected history adds the emitted durable transaction (144 B), codec/store
helpers, Stage focus (48 B), context (80 B), native regional conversion (16 B)
and bounded focus renderer (0 B). Transaction inlining initially enlarged the
DEV loop to 192 B; the explicit transaction boundary restored its 96 B guard
before review. No RAM or selected stack ceiling was raised. The SDK neutral-gap
function and lambda measure 112/32 B but remain outside the authored selected
stack collector; archived compiler reports and physical task watermark are
separate evidence. Adjacent-mask controller coverage was subsequently expanded
without changing production code; exact final gates bind the frozen candidate.

These resource costs do not accept the historical 50,468 B whole-boot heap
minimum against 65,536 B, prove WW optical quietness over gray, satisfy deferred
Mac checks, or stand in for the owner's battery sleep/wake acceptance.

## Italic overhang resource review (2026-10-14)

Issue #35: italic and bold-italic tails were clipped at exact-fit justified
line ends. `ReaderFont::drawGlyph` now computes each glyph's advance cell. A cell that
fits the text box may keep ink up to half the side margin past that box; an
overflowing cell keeps the text-box clip. Pagination, anchors and the font
identity are unchanged.

The clean measured source `d6ace342cafaec01a2a14d666da89d9ed053a877` and all
four private resource receipts establish this exact reviewed flash cost:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,473,184 | 1,473,296 | 112 | 133,248 | 300 |
| dev-usb | 1,611,040 | 1,611,152 | 112 | 134,080 | 304 |
| regional | 1,479,840 | 1,479,952 | 112 | 133,744 | 300 |
| regional-dev-usb | 1,619,664 | 1,619,776 | 112 | 134,576 | 304 |

Fresh independent GPT-5.6 Sol high resource review recomputed the reports and
artifact hashes and approved these exact costs. The usable limit stays
7,016,448 B and the minimum usable flash headroom is 5,396,672 B. Shared SRAM,
RTC and all selected stack entries (314/343/316/350) are unchanged; no RAM
or selected stack ceiling was raised. These host and compile facts do not establish panel
appearance, latency, or the historical 50,468 B heap minimum against 65,536 B.

## NEWT-10 italic integration remeasurement (2026-10-19)

Clean source `3439849419b48f776eea47fa25104bd25f39bec1` merges pinned
main `73c5ed0` into PR #45 without rewriting either history. Four clean native
health lanes pass at 1,473,296 / 1,611,152 / 1,479,952 / 1,619,776 B,
exactly +112 B per lane against main. Re-chained flash record 23 binds the
fresh report and core output hashes, retaining main's 22 flash records and
18 protected records as exact prefixes. Minimum usable headroom is 5,396,672 B.

SRAM remains 133,248 / 134,080 / 133,744 / 134,576 B; RTC remains
300 / 304 / 300 / 304 B. All 314 / 343 / 316 / 350 selected stack frames
equal their inherited ceilings. No RAM/stack ceilings, exclusions, adapters,
partition or reserve change. Fresh independent GPT-5.6 Sol high resource review recomputed all report/core
output hashes and approved these exact costs. Host and compile facts
do not establish hardware behavior or exact-source Mac ImageIO coverage.

## NEWT-10 battery footer integration remeasurement (2026-10-19)

Clean source `3f980f9d4c8770e4339f5b35d9a5d247349b84bf` normally merges PR #46 with verified italic integration
`14c3d72`. It retains battery percentage digits at 100% and uses the shared
atlas stamp/fit policy for broad glyph and bounded fallback behavior. The
combined cost includes both PR #46 correctness changes; their older parallel
audit records remain in branch history but are replaced in the current chain
by one fresh combined record 24 after the exact predecessor 23-record prefix.

Four clean preliminary builds compile and pass native NVS guards, then fail
only inherited flash ceilings. Their measured images are:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,473,296 | 1,474,384 | 1,088 | 133,248 | 300 |
| dev-usb | 1,611,152 | 1,612,240 | 1,088 | 134,080 | 304 |
| regional | 1,479,952 | 1,481,040 | 1,088 | 133,744 | 300 |
| regional-dev-usb | 1,619,776 | 1,620,864 | 1,088 | 134,576 | 304 |

Fresh independent GPT-5.6 Sol high resource review recomputed all four report
and sixteen core output hashes and approved the exact flash-only adoption. Record 24 accepts only these hash-bound measured
values, without slack, leaving minimum usable flash headroom 5,395,584 B.
All 18 inherited protected records, RAM budgets and 314/343/316/350 selected
stack guards remain unchanged; every selected measurement equals its ceiling.
No exclusions, adapters, anchor, partition or reserve change. Final four clean
passing resource gates are required after policy adoption. Host and compile
evidence does not establish physical or exact-source Mac ImageIO acceptance.

## NEWT-10 cover integration protected-history adoption (2026-10-19)

PR #47's already reviewed protected update at source
`7460c3c26a73c200e6d45b88e7b5f520eeb510e7` extends the predecessor's exact
18-record history. In every variant, `locateCoverMember` tightens from 1,376
to 1,264 B and `AutomaticLibraryCovers::cached` from 960 to 480 B. It adds
exact selected guards for legacy migration (848 B), `directoryMember` (208 B)
and the cache-probe lambda (560 B). RAM and every other selected entry retain
their previous tighter values. The existing reviewed source-policy history
pins accompany that unchanged protected record. The original isolated-source
resource and component call-chain review remains in PR #47's preserved history.

The predecessor's 24 flash records remain an exact prefix; integrated flash
growth will be remeasured before a new record is adopted. Four clean integrated
lanes must confirm all retained guards. Static frames and component sums do not
establish aggregate runtime stack safety or physical cover acceptance.

## NEWT-10 cover admission integration flash review (2026-10-19)

Clean source `007202782a307b326cf11019002ab23d4b23d07b` combines the generic cover ZIP admission and reversible
legacy-negative migration with the verified italic and battery fixes. All four
preliminary native compilations and NVS guards pass; resource gates fail only
their inherited flash ceilings. Fresh combined measurements are:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,474,384 | 1,475,712 | 1,328 | 133,248 | 300 |
| dev-usb | 1,612,240 | 1,613,568 | 1,328 | 134,080 | 304 |
| regional | 1,481,040 | 1,482,368 | 1,328 | 133,744 | 300 |
| regional-dev-usb | 1,620,864 | 1,622,192 | 1,328 | 134,576 | 304 |

Fresh independent GPT-5.6 Sol high resource review verified the complete
source/build identities, reports, output hashes and protected history; it approved
these exact combined costs and retained guards. Flash record 25 appends to the predecessor's exact
24-record prefix and accepts only these measured costs, without slack. Minimum
usable flash headroom is 5,394,256 B. Protected history retains the predecessor's
exact 18-record prefix plus the original reviewed cover update 19; all
317 / 346 / 319 / 353 selected frames equal their current tightened ceilings.
No RAM/stack ceiling increases, missing guards, exclusions, adapters, anchor,
partition or reserve changes. Four clean passing lanes are required after
this record's adoption. Static frames do not prove runtime stack safety; host
and compile evidence does not establish hardware or exact-source Mac ImageIO
acceptance.

## NEWT-10 diagnostic EPUB probe integration resources (2026-10-19)

Clean measured source `bad515f68647298a48ef1b245f48e0be717a46e5` integrates
PR #49 diagnostics with the current cover ZIP/migration route. Shared cover
and navigation extractions preserve production decisions. Fresh adversarial
review and authored cover/admission/navigation differential regressions confirm
behavioral equivalence; host diagnostic reporting adds no product feature.

All four clean resource lanes pass the predecessor budgets:

| Variant | Previous image | New image | Change | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,475,712 | 1,475,696 | -16 | 133,248 | 300 |
| dev-usb | 1,613,568 | 1,613,536 | -32 | 134,080 | 304 |
| regional | 1,482,368 | 1,482,336 | -32 | 133,744 | 300 |
| regional-dev-usb | 1,622,192 | 1,622,160 | -32 | 134,576 | 304 |

Fresh independent GPT-5.6 Sol high resource review recomputed every report,
build identity and core output hash and approved the exact flash/protected
costs and the bounded test-contract followups. Flash record 26 tightens only to these exact
measurements, retaining the predecessor's complete 25-record prefix; minimum
usable headroom is 5,394,288 B. Protected update 20 retains the complete
19-record prefix and every selected entry. The canonical navigation `bind`
facade now has a zero-frame tail wrapper, so its old 96 B guard tightens to 0.
The relocated `bindCanonical` body gains its own measured 96 B guard in every
variant, preserving measurement of the actual work. There are now
318 / 347 / 320 / 354 selected guards; all other RAM/stack limits stay fixed.
Source-policy history pins advance with that reviewed protected append.

No RAM or selected stack ceiling increases, guard removals, resource exclusions,
adapters, anchor, partition or reserve changes. Per-function static frames do
not prove whole-stack safety. All four final clean lanes must pass after policy
adoption; these host/compile facts do not establish hardware or exact-source
Mac ImageIO acceptance.

## NEWT-10 chapter integration resources (2026-10-19)

Clean source `5026ee62a358afe88bed253b560d52ae866f5302` combines canonical
chapter navigation, Home identity and transactional checkpoints with the verified
reading stack. Fresh independent resource review approved these exact costs.
Preliminary clean compilations and NVS guards pass; only inherited flash ceilings
fail before this record is adopted.

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,475,696 | 1,492,752 | 17,056 | 133,232 | 300 |
| dev-usb | 1,613,536 | 1,630,560 | 17,024 | 134,080 | 304 |
| regional | 1,482,336 | 1,499,440 | 17,104 | 133,728 | 300 |
| regional-dev-usb | 1,622,160 | 1,639,216 | 17,056 | 134,576 | 304 |

Flash record 27 retains the exact 26-record prefix and sets exact measured
ceilings, leaving at least 5,377,232 B usable headroom. Protected update 21
retains the exact 20-record prefix and every selected entry. Default/regional
SRAM tightens by 16 B. In each lane, repaint tightens 96→64 B, Home projection
1024→736 B, cache-page capture 112→80 B, metadata bind 112→80 B, navigation
append 128→64 B and section 32→16 B. Thirty measured chapter/identity/transaction
guards are added, including the ownerless checkpoint body at 656 B. The canonical
bind facade/body remain guarded at 0/96 B. Selected counts are now
348 / 377 / 350 / 384. No RAM/stack increase, missing guard, exclusion, adapter,
anchor, partition or reserve change. Four final clean lanes must confirm the
adopted records. Static frames do not establish aggregate stack safety; host
and compilation evidence does not establish hardware or Mac ImageIO acceptance.

## NEWT-10 CSS integration resources (2026-10-19)

Clean source `c08e55e86dd6d445fcd33f0e6bb148b41bd68467` integrates the bounded
CSS tokenizer/cascade projection with the verified chapter stack. Runtime CSS
source matches the reviewed standalone feature; cache policy advances to NST10.
Fresh independent resource and maintainability review approved these exact costs:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,492,752 | 1,501,744 | 8,992 | 133,232 | 300 |
| dev-usb | 1,630,560 | 1,639,536 | 8,976 | 134,080 | 304 |
| regional | 1,499,440 | 1,508,416 | 8,976 | 133,728 | 300 |
| regional-dev-usb | 1,639,216 | 1,648,208 | 8,992 | 134,576 | 304 |

Flash record 28 retains the exact 27-record prefix, with no measurement slack;
minimum usable headroom is 5,368,240 B. Protected update 22 retains the exact
21-record prefix and tightens the existing addText facade from 48 to 0 B in all
lanes. The numeric check and single SDK tail call remain; the measured facade
has no stack frame. All other selected guards and RAM ceilings remain unchanged;
selected counts348/377/350/384. No exclusion, adapter, anchor, partition, reserve
or SDK pin change. Preliminary clean compilations/NVS checks pass, failing only
inherited flash ceilings. Final four clean lanes must pass after adoption.
Per-function static frames do not establish runtime whole-stack safety; host
and compiler evidence does not establish hardware or Mac ImageIO acceptance.

## NEWT-10 glyph coverage integration resources (2026-10-19)

Clean source `6de89179fcd98913f3ad1bf00af78f11688d1562` integrates the pinned
four-face glyph expansion with the verified reading stack. Shared cell-width
clipping retains italic overhang and added bitmap/kerning routes. EPUB diagnostic
coverage now uses the complete validated font profile. Source pins, OFL files,
proof assets and licensing documentation remain intact. Fresh independent
resource and maintainability review approved exact measured costs:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,501,744 | 2,141,008 | 639,264 | 133,232 | 300 |
| dev-usb | 1,639,536 | 2,278,800 | 639,264 | 134,080 | 304 |
| regional | 1,508,416 | 2,147,680 | 639,264 | 133,728 | 300 |
| regional-dev-usb | 1,648,208 | 2,287,456 | 639,248 | 134,576 | 304 |

Flash record 29 retains the exact 28-record prefix and adopts these measurements
without slack, leaving at least 4,728,992 B usable flash headroom. All SRAM and
348/377/350/384 selected static frames are unchanged. The 22-record protected
history and its source-policy pins remain unchanged; no no-op record is added.
No exclusions, adapters, anchor, partition or reserve changes. Preliminary clean
compilations and NVS checks pass and fail only inherited flash ceilings; final
four clean lanes must confirm adoption. Static frames do not prove aggregate
stack safety. Host and compile evidence does not establish device timing,
optical behavior, physical acceptance or exact-source Mac ImageIO coverage.

## NEWT-10 cache-producer integration resources (2026-10-19)

Clean source `20dfed66503a00875750693966e904a159fec3b6` integrates the bounded
chapter-cache producer with the verified reading stack. Each firmware lane owns
one provenance-checked SDK copy: producer patch for default/USB; regional then
producer patches for regional lanes. Earlier CSS/font changes add no SDK patch.
Bounded interrupted I/O, invalid-cache classification, chapter transactions and
cover/reading idle ownership remain covered by host failure controls. Fresh
independent resource and maintainability review approved these exact costs:

| Variant | Previous image | New image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,141,008 | 2,152,864 | 11,856 | 133,232 | 300 |
| dev-usb | 2,278,800 | 2,290,688 | 11,888 | 134,080 | 304 |
| regional | 2,147,680 | 2,159,552 | 11,872 | 133,728 | 300 |
| regional-dev-usb | 2,287,456 | 2,299,392 | 11,936 | 134,576 | 304 |

Flash record 30 retains the exact 29-record prefix and adopts these measured
values without slack. Minimum usable headroom is 4,717,056 B. RAM, RTC and all
348/377/350/384 selected stack ceilings remain unchanged. The complete 22-record
protected history and its source-policy pins stay unchanged; no no-op append.
No exclusion, adapter, anchor, partition or reserve change. Preliminary clean
compilations and NVS checks pass, failing only predecessor flash ceilings.
Final clean four-lane resource gates and full Linux host validation remain
required after adoption. Static frames and authored component sums do not
establish runtime call-chain safety or unmeasured provider peaks. Host and
compile results do not establish device timing, optical or physical acceptance,
or exact-source Mac ImageIO coverage.

## NEWT-21 identity census and DEV telemetry flash cost (2026-10-19)

Fresh independent gpt-6-astra xhigh source/resource review
(`newt21_step1_review_r1`), 2026-10-19, covers the exact four clean
measurements from source `9e6a11b20847860d7642448f681b32d0da82507c`. The
Lead approved these measured flash-only costs under the AGENTS.md
reviewed-growth rule; this is not owner performance or physical acceptance. The
implementation re-read confirms the shared writer/census identity grammar and
protected metadata accounting. Neither non-DEV ELF contains cache telemetry
symbols; neither non-DEV image contains PC census/attempt row strings.
Their +208 B product cost belongs to the census/name-grammar integration; DEV
lanes additionally fund bounded numeric telemetry. Simpler omission would
restore the denial defect or remove the requested measurement. No product
deadline, retry policy, I/O schedule or display-completion code changes here.

| Variant | Previous image | New image | Growth | Usable headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 2,152,864 | 2,153,072 | 208 | 4,863,376 |
| dev-usb | 2,290,688 | 2,293,008 | 2,320 | 4,723,440 |
| regional | 2,159,552 | 2,159,760 | 208 | 4,856,688 |
| regional-dev-usb | 2,299,392 | 2,301,696 | 2,304 | 4,714,752 |

Flash record 31 retains the exact 30-record prefix and adopts measurements
without slack. The generated partition still leaves 7,016,448 usable bytes
after the unchanged reserve. SRAM 133,232/134,080/133,728/134,576 B and RTC
300/304/300/304 B are unchanged. All 348/377/350/384 selected stack guards
remain: paint is 80/96/80/96 B, retry 16 B in every lane. The complete
22-record protected history, RAM/stack ceilings, selected measurements,
exclusions, adapters, anchors and policy pins are unchanged. The original
flash-only failures remain historical failures; fresh clean builds must
confirm adoption. Static frames do not establish runtime stack peaks.
Hardware timing, USB/battery/cold-power behavior and owner acceptance remain
unestablished.

## NEWT-21 R6 diagnostic removal and flash tightening (2026-10-19)

Clean source `2afe139ea5d5543ab6cac5ba6c0b3d905af7231b` removes the DEV idle
diagnostic row/visit bit and restores the `e50a999` handoff. Census and attempt
rows remain. Fresh independent resource and maintainability review
(`r6_resource_review`) verified the four clean PASS reports and core artifact
hashes; no owner performance or physical acceptance is claimed.

| Variant | Record 31 | Exact new image/ceiling | Change | Usable headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 2,153,072 | 2,153,072 | 0 | 4,863,376 |
| dev-usb | 2,293,008 | 2,292,784 | -224 | 4,723,664 |
| regional | 2,159,760 | 2,159,760 | 0 | 4,856,688 |
| regional-dev-usb | 2,301,696 | 2,301,488 | -208 | 4,714,960 |

Flash record 32 preserves the exact 31-record prefix and records all four
measurements without slack. SRAM 133,232/134,080/133,728/134,576 B, RTC
300/304/300/304 B and all 348/377/350/384 selected stack ceilings are unchanged.
Open-reader frames are 144/560/144/560 B, paint 80/96/80/96 B, retry 16 B. The
22-record protected prefix, source-policy pins, exclusions, adapters, partition
and reserve are unchanged. Four post-adoption clean lanes at `7721030` pass
the exact tightened ceilings and unchanged protected guards. Quick Linux
(136 phases), full Linux (236 phases) and the pinned full-adoption release gate
pass through `b57aae7`; the evidence-only final commit is rechecked in the
task handoff. Static frames do not establish runtime stack peaks;
hardware timing, power journeys and owner acceptance remain unestablished.

## NEWT-21 step 3 checked cache reads (2026-10-19)

Runtime source `767716c8468760f138e1a2456f6891c24b9061da` batches native
cache reads inside checked immutable scopes and snapshots checkpoint identity
slots once. A final scope-composition correction removes duplicate checks while
retaining the checked exit of every independent publication boundary. The
unchanged ordinary-turn lookup bounds now run in the quick gate. Fresh
independent GPT-5.6 Sol high review
(`newt21-step3-composed-scope-review-r3`) is clean and approves this final
correction and exact resource costs; no owner performance or physical
acceptance is claimed.

| Variant | Previous ceiling | Exact image/ceiling | Growth | Usable headroom |
| --- | ---: | ---: | ---: | ---: |
| default | 2,154,544 | 2,157,104 | 2,560 | 4,859,344 |
| dev-usb | 2,294,320 | 2,296,864 | 2,544 | 4,719,584 |
| regional | 2,161,248 | 2,163,792 | 2,544 | 4,852,656 |
| regional-dev-usb | 2,303,024 | 2,305,568 | 2,544 | 4,710,880 |

One flash record (34) preserves the complete 33-record prefix. The usable
partition remains 7,016,448 bytes. SRAM stays 133,232/134,080/133,728/134,576 B;
RTC stays 300/304/300/304 B. Protected record 23 preserves the 22-record prefix,
retains every selected entry, tightens `validate` from 1088 to 16 B and ownerless
`saveChapterIdentityCheckpoint` from 288 to 48 B in all lanes, and adds an 80 B
guard for the extracted `validatePayload` CRC loop. Selected counts become
349/378/351/385. No RAM, RTC or selected stack ceiling increases. Exclusions,
adapters, SDK provenance, generated partition and reserve are unchanged.

The consumer borrows 1024 B after the 8192 B index arena inside the existing
48 KiB parse loan; it returns the loan before publication. Checkpoint snapshots
own 538 B of transient nothrow heap storage; cold identity lookup still uses
223 B. Full CRC scans own a transient 4096 B heap batch; read-scope timing owns a
transient 16 B heap budget. Allocation refusal uses ordinary reading or optional derived-checkpoint failure after the durable
progress commit. No resident page index or additional static buffer is added.

`artifacts/newt21-step3-preaudit-*.json` retain the pre-adoption flash-only
resource failures. They passed every unchanged or tighter protected guard. Four post-adoption
clean lanes at `dcdf839` pass the exact final ceilings and linked NVS/sleep
guards; current `artifacts/code-health-*.json` retain those receipts. The final
commit receives quick Linux, full Linux and pinned release gates before handoff. Static frames do
not establish runtime stack peaks or minimum heap. Modelled host I/O costs do
not establish hardware timing, USB/battery/cold-power behavior or owner acceptance.


## NEWT-21 step 5 round 2 cancellation authority (2026-10-20)

Clean source `ae80b526ae483e9c4a57ae5f2c2217762af7115a` removes competing
retry-refusal/status rules and captures the cancellation latch once after unwind.
Admission retains its latch in existing Poll flag padding. Every live cancellation
observation and the concrete producer boundaries are covered by 2,943 successful
trace replays, with independent seek/finish failures on `31a23c4` and uncancelled
terminal controls. See docs/065 for exact limits, accounting and evidence scope.

The four clean resource lanes pass at 2,159,168 / 2,299,312 / 2,165,872 /
2,308,016 app bytes, tightening round-1 ceilings by 256 / 240 / 256 / 240 B.
One exact flash-only record preserves the complete prior audit prefix and binds
the four archived raw reports/core image hashes. Review status is recorded in
that audit; no owner review or physical acceptance is claimed. The 7,016,448 B
usable limit and reserve are unchanged. SRAM remains 133,232 / 134,080 /
133,728 / 134,576 B; RTC remains 300 / 304 / 300 / 304 B. Every selected stack
entry/ceiling and all protected history, exclusions and adapters are unchanged.
The admission frame remains 48 / 128 B (non-DEV / DEV). The 256 B heap-owned
ledger remains live during builds: approximately 544 B remains from the
reported approximately 800 B margin, before allocator differences. The
50,468 B exception floor is unchanged; no physical heap-minimum proof is added.


Fresh independent GPT-5.6 Sol high review `newt21-step5-r2-review-1` accepts
record 38 with zero actionable findings in `31a23c4..b25ac5e`. It rehashes all
four raw reports and sixteen live core outputs, confirms clean `ae80b52` build
identities, and retains all 37 preceding flash records and 23 protected updates.
The audit's reviewer field records this actual review, without an owner or
physical-acceptance claim. The later test-only boundary-helper extraction
retains every assertion and keeps both test files below the existing threshold.

## NEWT-21 step 5 round 3 stop reasons (2026-10-20)

Clean pre-adoption source `a1639b16d264dcca009528d90a1871ceb4cd5598` compiles
all four lanes and passes every protected guard. The reasoned stop latch adds
176/160/176/160 B of flash: exact app sizes are 2,159,344 / 2,299,472 / 2,166,048 /
2,308,176 B. Flash record 39 binds all four raw reports and sixteen core outputs
under `artifacts/newt21-step5-r3-raw-code-health-*`, preserving the entire prior
flash history and all 23 protected updates. Independent review is pending as
recorded in that audit; post-review clean builds must pass the exact ceilings.
Pre-adoption failures leave old firmware-identity receipts unchanged by design.

SRAM remains 133,232 / 134,080 / 133,728 / 134,576 B; RTC is 300/304/300/304 B.
Every selected stack measurement and ceiling is unchanged (349/378/351/385
selected entries), including admission 48/128 B and DEV open/paint 560/96 B.
The 7,016,448 B usable partition, reserve, exclusions, adapters and protected
budget history are unchanged. No new heap allocation is added; the existing
256 B ledger and approximately 544 B arithmetic margin retain their round-2
caveats. Host replay and static compile measurements do not establish device
heap minima, stack peaks, I/O latency or USB/battery/cold-power behavior.

The first review found a pause failure-first ordering defect and default scratch
retention. Typed pause outcomes and acknowledgement-first settlement fix the
ordering; scoped temporary-directory lifetime fixes retention. Record 39 now
binds the revised source above, replacing only this unpublished round's earlier
measurement. A fresh bounded review must accept these revised exact costs.

Fresh GPT-5.6 Sol high review `newt21-step5-r3-review-2` is CLEAN on
`47d5cdc..19be6fe` and accepts revised record 39. It independently verifies the
four raw hashes, all sixteen live core outputs, expected clean DEV IDs and the
complete unchanged protected history/resource actuals. The reviewer field now
records this actual engineering review. Final clean lanes must still publish
new firmware identities; host and physical evidence remain separate.


## NEWT-21 step 5 round 4 returned-result ordering (2026-10-20)

Record 40 is the sole round-4 flash update, measured on clean `43629f1`.
Exact app sizes are 2,160,080 / 2,300,192 / 2,166,752 / 2,308,896 B, growth of
736/720/704/720 B. Four byte-exact archives under
`artifacts/newt21-step5-r4-raw-code-health-*` and all sixteen live core hashes
bind these costs. The preceding 39 flash records and all 23 protected updates
are unchanged. The audit names the actual independent reviewer below;
engineering measurements are separate from owner and hardware acceptance.

SRAM remains 133,232 / 134,080 / 133,728 / 134,576 B and RTC 300/304/300/304 B.
Every inherited selected stack actual and ceiling is unchanged (349/378/351/385
entries). The first candidate's 32-B validation frame failed its existing 16-B
ceiling; those receipts and preliminary uncommitted costs were withdrawn without
adoption. Keep the existing immutable checked exit out of line and settle its
failure directly: validation and that checked exit each measure 16 B. No
measurement is removed or ceiling raised. Per-function measurements do not
prove call-chain peaks, physical task-stack safety, heap minima or SD/input
latency. Existing ledger and power-acceptance caveats remain unchanged.

Plain and producer commit probes retain their exact 32-B guard. The historical
capturing-closure comparison also measures 32 B now, so it is retained as a
compiler comparison rather than falsely claimed as a rejected 48-B frame.
Enlarged compiled census controls and the resource ratchet still reject stack
growth. Final passing clean lanes must refresh old firmware identities left by
pre-adoption failures before handoff.


Fresh bounded GPT-5.6 Sol high adversarial/maintainability/resource review
`newt21-step5-r4-review-2` is CLEAN on exact target
`6e1344eca5b52d720200705442ee8f4c8ccd89c6` (`f1c25e8..6e1344e`). It accepts
record 40, all four raw-report hashes, all sixteen live core hashes and the
unchanged complete protected history and resource actuals. Two fresh review
rounds were used: the first withheld final acceptance while evidence was being
finalized; the second reports zero actionable findings and accepts the completed
packet. No reviewer-requested fix or unresolved finding remains. This is
engineering/resource review only. Post-review clean lanes and final host gates
still follow; hardware and owner acceptance remain unestablished.


Post-review clean native lanes PASS on
`3f61facecb5f7b082f2e0795cfc215ec7863bed0` in all four variants, each using
`firmware-build.py --clean --health-report` and its variant flags. Refreshed
firmware identities name that clean commit (`dirty: false`). Exact flash sizes
remain 2,160,080 / 2,300,192 / 2,166,752 / 2,308,896 B; SRAM, RTC and all
349/378/351/385 selected stack actuals are identical to round 3. All sixteen
live core hashes match their new reports. The first 39 flash records, all 23
protected updates and every non-flash resource budget remain byte-equivalent.
This receipt-only handoff adds no firmware changes after the reviewed target.
The final receipt commit is the target for quick/full Linux and pinned-history
gates; their actual outcomes are reported at handoff. Physical and exact-source
Mac ImageIO evidence remain pending.


## NEWT-22 durable descriptor and warned-exit resources (2026-10-20)

Record 42 (originally 41 before integration) is the first NEWT-22 flash update, measured with four clean
`firmware-build.py --clean --health-report` lanes on source
`389dfd0` (DEV USB and regional flags as appropriate). The source tree was
clean apart from the excluded output artifacts throughout these measurements.
All four final-source measurement lanes passed the linked native guard and
RAM/flash/selected-frame resource checks. The four
byte-exact reports are archived at `artifacts/newt22-raw-code-health-*.json`.
Record 42 binds their exact hashes, inputs and sixteen emitted core-artifact
hashes. No resource growth is inferred from the smaller PlatformIO code summary.

| Variant | Exact app image | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,160,464 | 384 | 133,232 | 300 |
| dev-usb | 2,300,592 | 400 | 134,080 | 304 |
| regional | 2,167,152 | 400 | 133,728 | 300 |
| regional-dev-usb | 2,309,296 | 400 | 134,576 | 304 |

The added flash pays for releasing optional cached Pages before durable state,
bounded lost-acknowledgement readback, independent acknowledged/live checkpoints,
and warning-and-proceed Back/Sleep behavior. The footer changes only its token
to `CHECK`; it retains the original nominal `UNSAVED` column. Detailed warning
text is on Sleep and the existing Your X3 Storage line. Removing the preliminary
Home overlay restores Frosted Home to 496 B and Stage Home to 320 B, exactly their
existing protected frames. No measurement boundary was moved or removed.
The existing `abandonSleep` boundary remains explicitly out of line so its
selected 16-B measurement survives the reduced number of callers.

All inherited selected entries remain: 349 / 378 / 351 / 385. The only inherited selected
actual changes are identical in all four variants:

| Selected function | Previous actual/ceiling | Final actual/ceiling |
| --- | ---: | ---: |
| `loadHomeProgress<NvsProgressBackend>` | 64 | 48 |
| `LibraryReaderSelection::load(ReadingProgress&)` | 336 | 288 |
| `saveOwnerlessIdentityCheckpoint<SampleReader>` | 656 | 640 |

Protected record 24 tightens exactly those three frames, preserving every selected
entry and every previous tighter value. It also adds a guard for the new shared
`PendingExactResume::armCheckpoint` capture boundary, measured at 96 B in every
lane. The checkpoint owner uses it after every failed live save; Settings reflow
selects its live retry without overwriting acknowledged progress. A final
unconfirmed reflow commit retains that retry instead of publishing it as acknowledged.
The save boundary follows `hasProgress()`, including a valid last-good tracker
after chapter failure; page readiness cannot suppress capture or retirement. The protected
`dispatchAction` frame remains 576 B and `openReader` remains 144 B in the default
lane; all inherited per-variant limits pass. Selected counts become
350 / 379 / 352 / 386 with that added measurement. These per-function frames do
not bound their combined call chain. RAM ceilings and actuals, RTC, all other
selected measurements and ceilings, exclusions/adapters, and the mount's three
file slots remain unchanged. The usable partition limit remains 7,016,448 B;
the partition layout and reserve are unchanged. Prior forty flash records and
twenty-three protected records are preserved.

Fresh independent GPT-5.6 Sol high adversarial/maintainability/resource review
`newt22_review_r3` is CLEAN on `213c28f..4bde287`, with the complete Linux gate
passing on that exact target. Three fresh rounds fixed three actionable findings:
reflow recovery must survive its final unconfirmed commit; a verified Home
commit must determine reboot-warning wording; and a valid failed-chapter
tracker must own save recovery independently of page readiness. Two test-only
repairs preserve the Home negative control and distinguish read-only recovery
from durable acknowledgement. Both were included in the final review and gate.
Record 42 and protected record 24 now record that engineering approval.
No owner or hardware acceptance is claimed. The reviewer-field/history-pin
update requires fresh clean receipts; final evidence is recorded in
`artifacts/newt22-validation.json`. Individual compiler frames do not
prove call-chain peaks, heap minima, task-stack safety or physical SD/display
latency. See [reading acceptance](reading-acceptance.md#newt-22-durable-save-and-exit-contract)
for warnings and the separate USB, battery and cold-power checks.

Post-review clean native lanes PASS on `7e43ec81528ddac7b60d51d951b2eade60a07a36` with the exact
image lengths above, unchanged SRAM/RTC and every selected cap satisfied.
Live source identities are clean and all sixteen emitted core hashes match
their fresh receipts. The final handoff changes only documentation and evidence;
no firmware or build-procedure change follows those measurements.


### NEWT-22 warm acknowledgement correction (record 43; originally 42)

Source `2951c20d752cc6fb20153acadf3d5fd873f0b8ab` measures exactly
2,161,808 / 2,302,000 / 2,168,512 / 2,310,704 B after four clean builds.
Record 43 is the correction record relative to accepted NEWT-22 record 42;
its growth is 1,344 / 1,408 / 1,360 / 1,408 B, below the unchanged usable limit.
The symbol/section explanation, raw hashes, selected-frame actuals and evidence
limits are in [the round-3c report](NEWT22-round3c-report.md).

No RAM, RTC, selected entry, ceiling, exclusion, adapter, partition or protected
history change is made. DEV openReader is back at its 560 B ceiling; setup is
384 B in every lane. Home remains 496/320 B. New method boundaries preserve the
complete selected measurements; they do not establish combined call-chain peaks.
Fresh Claude Opus 5.5 high review of `683fe1d..e411ddc` is CLEAN and explicitly
approves the original record 42 (now 43) for source `2951c20`. It independently verified the raw
report/core hashes, exact flash sizes, preserved history and all selected frames.
Final clean PASS receipts and acceptance limits are recorded in
`artifacts/newt22-r3c-resource-validation.json`; no physical acceptance is implied.

Post-review four clean native lanes PASS on `29f4b583f02c93e3a1611c62a0f258eec565e058` with the exact
original record-42 (now record-43) sizes, clean identities, unchanged SRAM/RTC and every selected cap
satisfied. All sixteen live core hashes match their fresh receipts, and every
compiler-input hash equals the reviewed `2951c20` measurement. The complete Linux
gate also PASSes on this checkpoint. Later handoff commits are documentation and
evidence only; hardware and exact-source Mac ImageIO remain pending.

## NEWT-23 reader footer legibility (2026-10-20)

Record 41 is the sole NEWT-23 flash update, measured on clean `d39401b`.
Exact app sizes are 2,162,192 / 2,302,320 / 2,168,880 / 2,311,024 B, growth of
2,112/2,128/2,128/2,128 B. Four byte-exact archives under
`artifacts/newt23-footer-raw-code-health-*` bind these costs. In the default
image the growth is the label speller `uiFoldText` (1,424 B), its letter and
Roman numeral tables (257 B) and the ellipsis cut `uiEllipsize` (380 B). Usable
partition headroom is 4,854,256 / 4,714,128 / 4,847,568 / 4,705,424 B; ratchet
headroom is zero. The preceding 40 flash records and all 23 protected updates
are unchanged.

SRAM remains 133,232 / 134,080 / 133,728 / 134,576 B and RTC 300/304/300/304 B.
Every inherited selected stack actual and ceiling is unchanged (349/378/351/385
entries). The unselected `renderReaderChrome` frame shrinks from 304 to 272 B;
the new helpers measure 96 B (`uiFoldText`) and 80 B (`uiEllipsize`), both
unselected. No measurement is removed or ceiling raised. Per-function frames do
not prove call-chain peaks; host footer frames do not establish panel legibility.

Fresh bounded GPT-6 Astra xhigh review `newt23-footer-review-r2` of
`213c28f..e9105b2` accepted the first record 41 (measured on `c185517`) and
found one minor fold defect and two nits. Fixing the defect shrank every image
by 64 B, so record 41 was rebound to `7a445fe`, replacing only this
unpublished round's earlier measurement. Review `newt23-footer-review-r3`
accepted that record and found that an inkless character spent a waiting space.
The fix in `d39401b` keeps every exact size; only the map and image hashes
change, so record 41 now binds `d39401b`.

Fresh bounded GPT-6 Astra xhigh review `newt23-footer-review-r4` is CLEAN on
`213c28f..252532a`. It accepts record 41 as bound to `d39401b`, all four raw
report hashes, the core artifact hashes, the unchanged prior flash and protected
history and the SRAM/RTC and selected stack actuals. The reviewer field records
this review. This is engineering/resource review only; post-review clean lanes
and host gates follow, and hardware and owner acceptance remain unestablished.

## NEWT-22/23 integration (record 44, 2026-10-20)

Merge `37c14e6a3bff501727231806a2fe1dcaa1c88fff` preserves parents
`44453bc` and `7cc92ab`. Main's first 41 flash records, including the complete
NEWT-23 record 41, remain unchanged. The historical NEWT-22 records are now
42 and 43. Record 42's `before` names main record 41's `after` to keep the
merged ledger chain valid; its original source, measured `after`, reviewer and
raw evidence remain historical unintegrated measurements. Record 43 retains
its original measurement and predecessor. All 24 protected updates, every
non-flash resource ceiling, policy, exclusion, adapter and selected entry are
unchanged from `44453bc`.

Record 44 alone binds the integrated source, measured clean on `37c14e6`:

| Variant | Exact app image | Growth over main | SRAM | RTC | Selected entries |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,163,920 | 1,728 | 133,232 | 300 | 350 |
| dev-usb | 2,304,128 | 1,808 | 134,080 | 304 | 379 |
| regional | 2,170,640 | 1,760 | 133,728 | 300 | 352 |
| regional-dev-usb | 2,312,816 | 1,792 | 134,576 | 304 | 386 |

The first measurement lanes failed only their preceding flash ceilings; all
selected frames, RAM and RTC passed. `setup()` is 384 B in all lanes;
`openReader()` is 144 B in default/regional and 560 B in both DEV lanes.
Frosted/Stage Home remain 496/320 B. Record 44 raises only flash to the exact
measured values, below the unchanged 7,016,448-B usable partition limit.

A separate local clean four-lane rebuild of pinned `7cc92ab` reproduces main's
record-41 image sizes. The full comparison is
`artifacts/newt22-r4-symbol-delta.json`, with both raw symbol tables and reports
archived beside it. In default, `saveReaderProgress` adds 812 B, out-of-line
`SleepSession::restore` adds 678 B while `setup` loses 494 B, and
`publishReaderOpen` adds 136 B while `openReader` loses 256 B. The Home warning
helper adds 178 B, recovery selection 142 B and destination checkpoint 138 B;
removed `ProgressStore::save` specialization saves 740 B. The integrated footer
adds just 38 B over main. Default flashed text/rodata/exception-frame deltas are
1,218/56/468 B; image alignment/padding accounts for the remainder. All four
variants' section and symbol deltas are recorded, rather than summing features'
unintegrated budgets.

The Lead authorized this integration measurement and will independently review
the whole integrated range. No new implementation-agent review is claimed.
Clean passing receipts and complete validation outcomes are recorded in the
round-4 report. Per-function compiler frames do not bound combined call chains;
Linux rendering and native compilation do not establish physical behavior or
owner acceptance. USB, unplugged battery, cold power-on and exact-source Mac
ImageIO checks remain separate and pending.


Post-audit four clean native lanes PASS on `7cb5567`, with exact record-44
sizes, unchanged SRAM/RTC, all 350/379/352/386 selected actuals identical to
`44453bc`, and all sixteen live core hashes verified. The twelve regenerated
receipts carry clean source identities and compiler inputs matching `37c14e6`.
Quick, focused and full Linux gates PASS; full parity covers 18 fixtures and
13,408 comparisons, and the fresh full turn matrix covers 24 fixtures/3,456
observations. Complete results and limitations are in
[the round-4 report](NEWT22-round4-report.md) and
`artifacts/newt22-r4-validation.json`. Later handoff changes contain only
documentation and evidence. The Lead's independent integration review and
physical/Mac acceptance remain pending.

## NEWT-22 storage ownership record 45 (2026-10-20)

Round 5 replaces competing display writers with a three-byte typed notice owner
and separates verified progress from destination results. Exact clean images
from `be58565` are 2,166,432 / 2,306,288 / 2,173,136 / 2,314,992 B
(default / DEV / regional / regional DEV). Record 45 grows only flash by
2,512 / 2,160 / 2,496 / 2,176 B over unchanged record 44. All are below the
unchanged 7,016,448-B usable limit. Records 1–44, including main's NEWT-23
record 41, every protected-history entry, prior tighter value, selected entry,
exclusion, adapter, partition and policy pin remain unchanged.

The isolated clean `6cb251b` comparison reproduces record 44. In default,
the typed checkpoint coordinator adds 790 B while `saveReaderProgress` loses
672 B. The diagnostic priority function grows 156 B; typed reader diagnostic
text costs 198 B and its classification helper 164 B. Destination checkpoint
changes net 52 B, source/open retirement adds 140 B to `openReader`, and warm
restore grows 116 B. Library rollback now snapshots the owner; its renamed
closure changes net 14 B. Default section deltas are 1,538 B text, 424 B rodata
and 536 B exception frames; image headers/alignment account for the remainder.
The review-2 Library refusal correction keeps its local cause independent of
the shared priority. Its typed resume text adds 20 B, the inlined caller shrinks
4 B, and exception frames add 20 B; alignment yields 48/32/48/48 B image growth
over the earlier round-5 source. All four lanes' symbol and section deltas, including removals and signature
renames, are in `artifacts/newt22-r5-symbol-delta.json`, with raw tables and
clean base reports archived beside it. Audit report hashes cover decompressed
raw report bytes.

SRAM stays 133,232 / 134,080 / 133,728 / 134,576 B and RTC stays
300 / 304 / 300 / 304 B. All 350 / 379 / 352 / 386 selected entries fit their
existing ceilings. `setup()` stays 384 B in all lanes, `openReader()` stays
144 / 560 / 144 / 560 B and Frosted/Stage Home stay 496/320 B. The initial
measurement fails only the old flash ceilings; record 45 authorizes the exact
measured growth. Final clean receipts and five fresh cross-family review rounds are recorded
in the [round-5 report](NEWT22-round5-report.md). Round 5 is CLEAN; the measured
engineering growth is approved. Compiler frames do not bound complete call-chain peaks;
none of this is hardware or owner-acceptance evidence.

## NEWT-24 UI label consistency record 46 (2026-10-21)

Record 46 is the sole NEWT-24 flash update, measured on clean `8958bf1`.
Exact app sizes are 2,166,848 / 2,307,024 / 2,173,552 / 2,315,728 B
(default / DEV / regional / regional DEV), growth of 416 / 736 / 416 / 736 B
over unchanged record 45. Four byte-exact archives under
`artifacts/newt24-labels-raw-code-health-*` bind these costs. In every lane the
growth is the fold's drop mode and its two wrappers, `uiChapterLabel` (74 B)
and `uiBookTitle` (64 B), and their calls in the Chapters rows, the reader
menu title line and Home's words. Only the DEV lanes link Stage Home; its
folded words, the shared `homeChapterProgress` rule and the out-of-line book
block add 320 B more than the matching non-DEV lane. Usable
partition headroom is 4,849,600 / 4,709,424 / 4,842,896 / 4,700,720 B against
the unchanged 7,016,448-B limit; ratchet headroom is zero. Records 1–45 are
unchanged.

SRAM remains 133,232 / 134,080 / 133,728 / 134,576 B and RTC 300/304/300/304 B.
`homeText(const HomeData&)` keeps its signature and 32 B, and
`renderFrostedHome` stays 496 B because Home's words now hold the folded title
in place of its old first-line buffer. Home's words, now 124 B larger, move
from the selected `renderStageHomeFocused` into the new out-of-line
`drawStageBook`, so protected update 25 keeps them guarded: it selects
`drawStageBook` at its measured 432 B and tightens `renderStageHomeFocused`
320→176 B in all four lanes. Every earlier entry and tighter value is kept.
The Stage book chain is still about 288 B deeper than at `2a7e1da`, which the
per-function guards do not bound. The unselected `renderReaderOverlay` grows
320→352 B and `renderReaderUi` 208→240 B. The fold measures 96 B and each
wrapper 16 B, all unselected. No measurement is removed or ceiling raised.
Per-function frames do not prove call-chain peaks. Host frames do not
establish panel legibility.

Review round 1 (fresh Opus 5.5 high, `2a7e1da..07b3e02`) found no correctness
blocker. It found that the move hid Home's words from selected-stack coverage
(fixed by update 25), that both themes wrote the chapter progress line
separately (now `homeChapterProgress`) and that producers repeated the
fallback title literal (now `kSdBookTitle`); two comment nits are fixed.
Round 2 (fresh Opus 5.5 high, `07b3e02..102f39e`) is clean and rechecked
record 46, update 25 and the pins. `src/phased_reader_metadata.h` keeps its own
copy of the fallback literal; that reader metadata layer does not depend on
the UI header.

Independent review r1 (GPT-6 Astra xhigh, `2a7e1da..a673295`) found two low
issues, fixed in `8958bf1`: a left-out title character joined its neighbours
into a different title ("now∕here" read "nowhere"; a left-out run now reads
as one space), and `uiBookTitle` wrote its fallback into a null destination.
The fix adds 16 B in the default and both DEV lanes and none in regional;
record 46 is remeasured on `8958bf1`. Update 25 and every stack and RAM value
are unchanged.


## NEWT-26 read-only Home clock record 48 (2026-10-21)

Integration merge `25062c3f85e16d3375ed06832349e9d4c1abbe44` retains parents
`7cf109c` and main `5493f5d`. Main's NEWT-28 Library cover/cursor fix and
NEWT-29 memory-floor, flash-history and adoption rules remain intact. The
read-only DS3231 Home clock has no runtime changes from its standalone review.
Fast focus and status passes preserve the shown time; full compositions read
once, with no clock writes, retries, settings or reader clock.

Clock flash record 47 is renumbered to 48 after main's accepted record 47.
Every main flash record 1-47, protected record 1-25 and the floor object is
byte-identical. The complete flash-history and protected-history pins use
SHA-256 of their compact sorted-key JSON, as required by NEWT-29.

Four clean measurements on the integration merge are archived in
`artifacts/newt26-integration-raw-code-health-{default,dev-usb,regional,regional-dev-usb}.json`.
Exact flash sizes are 2,169,584 / 2,309,952 / 2,176,272 / 2,318,624 B, growing
by 1,232 / 1,440 / 1,216 / 1,424 B above main. The smallest remaining usable
partition headroom is 4,697,824 B. One independent source/resource review
approved these exact measurements before adoption; reason spacing was fixed
before pinning. No additional clock flash record is appended.

RAM is 133,232 / 134,064 / 133,728 / 134,560 B: unchanged production lanes and
16 B less in each DEV lane. RTC before/after is default 300->300, DEV USB
304->304, regional 300->300 and regional DEV USB 304->304 B. Protected update
26 preserves every existing entry and tighter clock ceiling. Existing stack
tightenings remain: resume 112->96 B in all lanes, DEV setup 400->384 B and
paint 48->32 B. These were previously measured headroom, not new clock savings.
Six new clock guards remain exact: BCD helpers and source begin/paint 0 B,
plausibility 80 B, read 80 B production / 128 B DEV. NEWT-29 growth-style
evidence covers exactly these added frames in all four variants. No existing
RAM or stack ceiling rises. The clock is always enabled, so there is no
allocation held by a disabled feature and `idle_owner_approval` is null.

Floors, exclusions, adapters, source ceilings, partition and reserve are
unchanged. Per-function reports do not prove runtime whole-chain stack or heap
headroom, clock retention or physical input latency. The integration's final
clean receipts, Linux gates, four `--adopt 5493f5d` checks and pinned release
history gate are recorded separately in `/var/tmp/newt26-evidence/integration/`.
The pinned reviewer reference `integration/review-r1.md` is archived in
[`artifacts/newt26-integration-review-r1.md`](../artifacts/newt26-integration-review-r1.md);
the clean closing verdict is archived in
[`artifacts/newt26-integration-review-r2.md`](../artifacts/newt26-integration-review-r2.md).
The older `artifacts/newt26-clock-raw-code-health-*.json` files retain superseded
standalone measurements; record 48 uses the integration reports named above.
The standalone software and Lead review results do not substitute for those
integration gates or for separate Mac ImageIO and physical acceptance.


## NEWT-27 standalone round-1 measurement (historical, 2026-10-21)

Before integration, the NEWT-27 branch proposed its own record 47, measured on clean firmware source
`0bbec2e`. Exact app sizes are 2,169,952 / 2,310,528 / 2,176,672 / 2,319,232 B
(default / DEV / regional / regional DEV), growth of 3,104 / 3,504 / 3,120 /
3,504 B over unchanged record 46. Four byte-exact archives under
`artifacts/newt27-next-chapter-raw-code-health-*` bind the image, ELF, map,
partition and input/report hashes. The cost is inactive canonical next
selection, bounded style/totals certification plus one capped write, an explicit
view verdict independent of retry ledgers, and verified cached boundary
capture. DEV adds target/result traces. The final trace isolation costs 144 B
per DEV image versus the first proposal and cuts its long-lived producer
attempt frame 448→224 B, below the 368-B base.

The usable partition limit remains 7,016,448 B (7,798,784 B capacity less the
782,336-B reserve). SRAM stays 133,232 / 134,080 / 133,728 / 134,576 B and RTC
300 / 304 / 300 / 304 B. Every inherited selected stack entry, measurement and
ceiling is unchanged (351/380/353/387 entries). The C3 session remains 1,952 B
and the cache owner 96 B. Records 1–46, all protected budget history and tighter
values, the 50,468-B heap-min-free floor, partition/reserve, exclusions and
adapters are preserved. No RAM/RTC or selected-stack exception is introduced.
Additional unselected boundary/admission frames and the separate DEV formatting
frames are listed in the [candidate report](067-next-chapter-prefetch.md);
per-function evidence does not bound total task depth.

The complete Linux gate, heterogeneous turn matrix, every-page parity and
removal controls pass; later DEV formatting changes have focused runtime and
serial coverage. Fresh source/maintainability/resource review and final clean
receipt identities are recorded in that report. Neither software nor resource
evidence grants owner, device, battery or ImageIO acceptance.

Fresh Opus 5.5 high round 2 of `f6fba84..a818c71` is CLEAN: all three low
round-1 items are resolved; the exact four-lane flash costs, preserved histories,
unchanged protected resources and clean `ea23b1c` receipts are verified. The
measured flash cost is accepted as an engineering trade. This close-out changes
only documentation and the review field; no source or budget changes follow it.


Round 2 integrates main `5493f5da72e5763264e5fbe2550ffa832d5e5f58`,
including its NEWT-28 record 47 and NEWT-29 memory policy. Main's complete flash,
protected-memory and floor histories are retained verbatim. The standalone
NEWT-27 proposal above remains historical evidence in this document, its raw
reports and the merge parent; it is not an additional record in the integrated
audit chain. One final integrated NEWT-27 record follows main's chain after
fresh four-lane measurement and review. No RAM, RTC, selected-stack ceiling or
floor growth is authorized for this task.

## NEWT-27 round-two record 48 (historical, before clock integration)

This superseded branch record 48 followed main record 47 before clock integration. Clean source `9cd7315` measures
2,172,656 / 2,315,184 / 2,179,376 / 2,323,888 B (default / DEV / regional /
regional DEV), growth 4,304 / 6,672 / 4,320 / 6,688 B. The four
`artifacts/newt27-r2-raw-code-health-*` reports bind the exact inputs, reports,
images, ELF, maps and partitions. The final integrated audit retains every
main flash record, protected update and floor unchanged; its flash pin covers
all 48 records. Usable headroom remains 4,843,792 / 4,701,264 / 4,837,072 /
4,692,560 B. The fixed 50,468-B heap exception is not extended.

SRAM is 133,232 / 134,080 / 133,728 / 134,576 B; RTC is 300/304/300/304 B.
Every inherited selected stack actual equals main's receipt, with all
351/380/353/387 entries retained and no ceiling growth. `loop` stays
80/96/80/96 B. The unselected frames and limits of per-function evidence are
listed in [the candidate report](067-next-chapter-prefetch.md).

Fresh Opus 5.5 high review of the integrated change found no blocker. Its
bounded final follow-up through `f9530c6` is CLEAN, resolves both low findings
and accepts these exact costs; see `artifacts/newt27-r2-review-r2.md`.
Clean passing receipts and adoption against main `5493f5d` are verified.
The complete Linux gate passes at `7822cce` in 1,171.309 seconds, including
18-fixture full page parity and the corrected extracted queue fixture. The
quick invocation passed earlier at `f9530c6`. Pinned full candidate history
passes. Fresh Opus 5.5 high close-out review through `7822cce` is CLEAN; see
`artifacts/newt27-r2-review-r3.md`. Device latency, whole-task and heap peaks,
ImageIO and power-condition acceptance are pending.

Final clean native builds of `b50e3cd` pass all four lanes at these exact costs,
with clean compiled identities. All four resource-adoption checks against
`5493f5d` pass. The final resource audit is
`artifacts/newt27-r2-final-resource-audit.json`; later queue-fixture changes
contain no firmware or build-procedure changes.

## NEWT-27 clock integration record 49 (2026-10-21)

Merge `667a41abe7142aacdd9de7c452fb15555b6e0ed1` preserves parents `8a4e9e3`
and main `7c6dec6`. Main's read-only Home clock flash record 48, protected
entry 26, every earlier record and all floors remain unchanged. The superseded
NEWT-27 record 48 above remains historical in its merge parent and raw reports;
record 49 is the sole final NEWT-27 record in the current chain. No task-specific
protected update is needed. Only the complete flash-history pin changes, using
SHA-256 of compact sorted-key JSON; main's protected and floor pins remain exact.

Four clean measurements of `667a41a`, archived as
`artifacts/newt27-integration-raw-code-health-{variant}.json`, measure flash
2,173,904 / 2,316,624 / 2,180,576 / 2,325,312 B (default / DEV / regional /
regional DEV), growth 4,320 / 6,672 / 4,304 / 6,688 B over main. The four-lane
reason states RTC before→after: 300→300 / 304→304 / 300→300 / 304→304 B.
SRAM stays 133,232 / 134,064 / 133,728 / 134,560 B. Every selected stack actual
matches main; all 357/386/359/393 entries and ceilings remain intact. Usable
headroom is 4,842,544 / 4,699,824 / 4,835,872 / 4,691,136 B. No reserve,
partition, exclusion, adapter, floor or memory ceiling is loosened. The
50,468-B heap exception is not extended. Individual frames do not prove total
runtime stack or heap margins.

Fresh Opus 5.5 high merge-resolution review is CLEAN and accepts the exact
record-49 costs; see `artifacts/newt27-integration-review-r1.md`. Focused clock,
trace, queue, producer power/pause and retained-cache regressions pass. The
quick Linux gate passes on the merge in 300.409 seconds. Final clean passing
receipts cite `b7f7c1b`. Full matrix and the complete Linux gate pass; Linux
ran at `04ad323` in 899.234 seconds, including full 18-fixture parity. All four
post-full adoption checks against `7c6dec6` and pinned full candidate history
pass. Physical and exact-source Mac ImageIO
acceptance remain separate. Pre-existing progress-save input blocking (#89)
and the post-save CRC (#63) are unchanged and out of scope.

The four final clean lanes pass on `b7f7c1b`, and their committed identities
cite that clean compiled revision. All four adoption checks against `7c6dec6`
and pinned full candidate history pass. The preservation/measurement audit is
`artifacts/newt27-integration-final-resource-audit.json`. The full heterogeneous
turn matrix also passes. Complete Linux/parity coverage passes on the clean
receipt commit `04ad323`: 13,408 comparisons, both orientations/directions and
state journeys. No firmware or build-procedure changes follow these builds.


Fresh Opus 5.5 high closing review through `82698ef` is CLEAN, confirming the
applied record 49, complete inherited history, unchanged memory ceilings and
actuals, clean receipts and all gate/data hashes. The full verdict is archived
in `artifacts/newt27-integration-review-r2.md`. Final-HEAD history and all four
adoption checks are rechecked after this metadata-only close-out.

## NEWT-30 Home clock time zone (2026-10-21)

The Home clock zone is a durable product preference
([068](068-home-clock-time-zone.md)). Source thresholds, adapters and
generated exclusions are unchanged.

### Record 50

Four clean measurements of `a8eea46` are archived in
`artifacts/newt30-raw-code-health-{default,dev-usb,regional,regional-dev-usb}.json`.
Exact flash is 2,177,328 / 2,320,000 / 2,184,000 / 2,328,672 B
(default/DEV/regional/regional DEV), growing by 3,424 / 3,376 / 3,424 /
3,360 B above main's record 49. The usable limit is unchanged.

RAM is unchanged at 133,232 / 134,064 / 133,728 / 134,560 B. RTC before/after
is default 300->300, DEV USB 304->304, regional 300->300 and regional DEV USB
304->304 B. The zone has no RTC copy. The published zone fills existing
`HomeClockSource` padding, which a `static_assert` pins, and the Settings
zone-choice flag fills existing `ReaderUiState` padding, so nothing is held
while the feature is unused and `idle_owner_approval` stays null.

Every selected stack ceiling and actual is unchanged. `readHomeClock` and
`plausibleClock` keep their signatures and frames (80 B, DEV read 128 B). The
conversion runs in a separate `localClockText`, and DEV telemetry uses two
`printf` calls because the extra variadic arguments raised the DEV read frame
to 144 B. New unselected frames are `localClockText` 48 B, `localClockTime`
64 B, `transitionMinute` 48 B, `commitTimeZone` 128 B, `restoreTimeZone`
32 B, `handleReaderUi` 96 B, `dispatchReaderUi` 32 B and the `newt_zone`
`DurableRecordStore` load/read instances at 80 B. Restore runs once at boot,
through `beginHomeClock`, which is inlined into setup (setup stays 384 B);
commit runs only from Settings Save. Per-function frames do not bound
whole task-stack or heap peaks. Floors, exclusions, adapters, partition and
reserve are unchanged; records 1-49 are byte-identical.

Fresh GPT-6 Astra xhigh bounded reviews cover `ab2f910..HEAD`. Round 1
required changes; their fixes moved flash by 16 B per lane. Round 2 was CLEAN
on the earlier sizes (2,177,312 / 2,319,984 / 2,183,984 / 2,328,672 B). The
Lead's follow-up then scoped the Settings note by row (+16 B on three lanes)
and moved the boot wiring into `beginHomeClock`; this record was re-measured
on `a8eea46`. Round 3 reviewed `98cbcee..e796aea`, verified these exact costs
and raised two documentation fixes, applied in the next commit. Round 4
confirms them. Reviews are archived as `artifacts/newt30-review-r1.md` to
`artifacts/newt30-review-r4.md`.

## NEWT-31 Part A Library-open attribution (2026-10-21)

Record 51 (originally unmerged record 50) bound the four clean `1470349` measurements in
`artifacts/newt31-raw-code-health-{variant}.json`. A2 refreshes its evidence to
the header-dependency repair `007c5a5` using
`artifacts/newt31-a2-raw-code-health-{variant}.json`; exact costs are unchanged.
The complete earlier candidate record is retained in
`artifacts/newt31-a1-flash-record-50.json` and Git history. Main records 1-50
and every protected value remain intact. Fresh bounded A2 resource review is
CLEAN (artifacts/newt31-a2-resource-review.md). App images are
2,173,872 / 2,317,040 / 2,180,560 / 2,325,728 B (default / DEV / regional /
regional DEV), deltas -32/+416/-16/+416 B from main `ab2f910`. The DEV-only
phase row reuses the 76 B action owner and empty scopes; the immediate phase
prints/call-count row are replaced. Release timing is absent and the previous
unconditional phase-lambda clock calls are removed. No performance fix lands.

SRAM remains 133,232 / 134,064 / 133,728 / 134,560 B. RTC before->after is
300->300 / 304->304 / 300->300 / 304->304 B. Every 357/386/359/393 selected
frame and ceiling stays unchanged; the full reports only remove the obsolete
32 B DEV phase-print lambda. All prior flash/protected records, floors,
partitions, reserve, exclusions and adapters remain intact. No unused-feature
memory or idle-state approval is introduced. Individual compiler frames do not
prove whole-task or runtime margins. Fresh independent bounded resource review is CLEAN and accepts the exact
costs (artifacts/newt31-resource-review-r1.md). Post-review clean receipts are
recorded in docs/069 before handoff; full
adversarial/maintainability review and optimization remain Part B. The Lead
still must measure physical residual and book-open timing; no hardware was used.


## NEWT-31 Part B first-page verification (2026-10-22)

Append-only flash record 52 (originally unmerged record 51) and protected record 27 bind the four clean
`b2e2a6b` measurements. Exact flash is 2,186,256 / 2,330,736 / 2,192,928 /
2,339,424 B, increases of 12,384 / 13,696 / 12,368 / 13,696 B from accepted
Part A. SRAM remains 133,232 / 134,064 / 133,728 / 134,560 B and RTC
before->after remains 300->300 / 304->304 / 300->300 / 304->304 B.
The digest context overlays the existing producer ledger; full-source reads
borrow existing frame/import scratch. Cache-only state uses tail padding,
selection debts use existing bitfield space, and blocked-save recovery uses
existing pending RAM state. No feature retains additional memory while unused.

All inherited stack keys remain, with 34 new selected keys per lane, including
canonical helper bodies and the newly selected legacy exit wrapper.
Selected counts are 391/420/393/427; 6/8/6/8 inherited frames rise and eight
per lane tighten. The legacy exit wrapper and the extracted 112 B exit body
are both selected: moving the body does not prove a reduced task peak.
The largest inherited increase is 32 B in verified checkpoint peer carrying.
Existing floors, 16 KiB loop stack, partition reserve, exclusions, adapters
and oversized-file allowances do not change. Full reports and exact resource
review are archived under `artifacts/newt31-b-*`; see docs/070 for scope,
regression evidence, residuals and pending Lead device measurements.


## NEWT-31 main integration (2026-10-22)

Merge `b06a20a` retains completed Part B `4f06932` and main `9a6753e` as
its two parents. Main’s records 1–50 are retained exactly, including its
record-50 object key order. Part A’s unmerged record 50 is renumbered 51;
its policy predecessor is rechained to main 50, with source/after/evidence
unchanged and the entire original archived. Part B’s unmerged record 51 is
renumbered 52 with exact measurements retained. Original objects live in
`artifacts/newt31-original-part-a-record-50.json` and
`artifacts/newt31-original-part-b-record-51.json`. Protected records 1–27
remain exact. Documents 068/069 become 069/070; main keeps its 068 time-zone
document. No history, floor, exclusion, adapter or file allowance is removed.

Record 53 uses the existing strict linear predecessor (record 52), with formal
flash increases 3,392 / 3,376 / 3,392 / 3,376 B. The owner’s main-record-50
comparison is explicit in its reason: 2,177,328 -> 2,189,648 / 2,320,000 ->
2,334,112 / 2,184,000 -> 2,196,320 / 2,328,672 -> 2,342,800 B, increases
12,320 / 14,112 / 12,320 / 14,128 B. All four exact clean merged measurements
are source-bound to `b06a20a`; the usable limit remains 7,016,448 B.

SRAM before->after remains 133,232->133,232 / 134,064->134,064 /
133,728->133,728 / 134,560->134,560 B. RTC before->after remains
300->300 / 304->304 / 300->300 / 304->304 B. Every inherited selected frame
is emitted at its existing exact size. Record 28 adds five actual time-zone
boundaries per lane: commit 128 B, restore 32 B, local clock text 48 B,
durable zone load 80 B and slot read 80 B. Selected counts become
396/425/398/432. No separate zone-save body is emitted. These frames are
not a whole-call-chain peak; no stability margin is redefined. The zone
fields occupy existing padding and source verification reuses existing
owners, so no additional memory is retained while unused.

Exact reports, reviewed records and source/ownership review are archived under
`artifacts/newt31-merge-*`. All four final clean native resource gates pass
with clean `6af311f` identities and exact reviewed compiled-input/resource
matches. All four adoption checks against `9a6753e` and the pinned full-history
release gate pass. On clean `7ecf3de`, quick Linux passed in 273.607 seconds,
full Linux in 925.308 seconds, and full 18-fixture every-page parity in
592.327 seconds. The full matrix passes 1,248 cells and negative controls.
Fresh closing merge-resolution adversarial/maintainability review and final
installed-resource review are CLEAN. Main’s zone behavior and Part B’s
verified book-write boundary are retained. Exact-source Mac ImageIO and the
Lead's separate physical power-condition checks remain outstanding; no device
was used and no physical acceptance is claimed.


## NEWT-31 final-probe correction (2026-10-22)

Lead review found final-probe input could consume the completed SHA and
permanently mark verification Failed. Correction source `79f0eb4` pauses
before consuming SHA and resumes at EOF without another payload read. Its
focused unchanged/changed-source regression and frozen-old-code negative pass.
Four clean measurements are 2,189,648 / 2,334,128 / 2,196,320 / 2,342,816 B:
0 / +16 / 0 / +16 B versus `701855c`. At the Lead's direction, flash record 53
is refreshed exactly and its complete original is archived under
`artifacts/newt31-finalize-original-flash-record-53.json`. Protected record 28
retains identical before/after and all five measurements, refreshing only
source/evidence/review metadata so adoption binds the corrected inputs. Its
complete original is likewise archived. Earlier records, all selected frames,
SRAM, RTC, floors and other policy remain exact. The read-only closing review
accepts source, regression and values; its F1 source-pin installation item is
resolved and closing R2 is CLEAN. All four final clean native resource gates
pass with clean `21ede33` firmware identities and exact raw measurement/input
matches. All four adoption checks against `9a6753e` and the pinned full-history
release gate pass. On clean `30a0f55`, quick Linux passed in 255.942 seconds,
full Linux in 841.243 seconds and full 18-fixture every-page parity in
530.569 seconds. The final-probe regression and frozen-old-code negative run
inside the full gate. `gate_stop_test.py` passed in 13.913 seconds without the
Lead's timeout reproducing. Evidence is archived under
`artifacts/newt31-finalize-*`. No device or physical acceptance claim is made.


## NEWT-31 pending uncached turn correction (2026-10-22)

Source `64583e0` completes the bounded foreground digest after a pending
cache-only turn misses, then retries the same Next/Previous action. Verified
reading remains independent of refused durable selection, with writes still
guarded. A genuine chapter failure remains visible. Live pending pages skip
redundant presence probes; actual cache validation and the opening probes stay.
Flash record 53 is refreshed at the Lead's instruction to 2,190,016 /
2,334,560 / 2,196,688 / 2,343,248 B, changes +368 / +432 / +368 / +432 B
from `9d0a0b6`. Its complete previous record is archived under
`artifacts/newt31-turn-original-flash-record-53.json`, alongside every earlier
original. Flash records 1–52 and protected records 1–28 remain exact. New
protected record 29 tightens pageGate32->16 B and selects the transient
finishPendingReaderTurn96 B frame in all lanes. Counts are 397/426/399/433;
every other selected frame is unchanged and no key is removed. SRAM remains
133,232 / 134,064 / 133,728 / 134,560 B; RTC remains300/304/300/304 B.
No idle memory, buffer, floor, reserve, exclusion, adapter or allowance changes.
These static frames do not establish peak stack or hardware margin. The fresh
bounded closing review R2 is CLEAN, including the exact four-lane proposal.
All four final clean native resource gates pass with clean `52dc5f6` firmware
identities and exact reviewed measurement/input matches. All four adoption
checks against `9a6753e` and the pinned full-history release gate pass. On clean
`a91ebad`, quick Linux passed in 273.278 seconds and full Linux passed in
858.450 seconds, including the focused regression and old-code negative. Full
every-page parity passed all 18 fixtures in 541.039 seconds. `gate_stop_test.py`
passed in 13.945 seconds without the reported timeout reproducing; no other
suites or reviewer tests ran concurrently in this worktree. Evidence is under
`artifacts/newt31-turn-*`. Exact-source Mac ImageIO and Lead device acceptance
remain separate. The Lead should remeasure pending cached-turn v[6] and verify
uncached turns during the pending window on the device. No device was used
and nothing was pushed.


## NEWT-31 foreground turn input ordering (2026-10-22)

Clean source `c884c07` measurements are 2,190,176 / 2,334,720 / 2,196,864 /
2,343,408 B (default / DEV USB / regional / regional DEV USB), changes
+160 / +160 / +176 / +160 B from `0f22ae6`. Flash record 53 is refreshed
exactly; its complete previous version is archived as
`artifacts/newt31-input-original-flash-record-53.json`. Flash records 1–52
and protected records 1–29 remain exact, with prior archives preserved. New
protected record 30 retains every selected key: the old no-argument verifier
becomes a zero-frame tail call, its core remains 64/80/64/80 B and the new
cancellation callback is 32 B. Selected counts are 399/428/401/435. SRAM stays
133,232 / 134,064 / 133,728 / 134,560 B; RTC stays 300 / 304 / 300 / 304 B.
All other selected frames, floors, reserve, exclusions, adapters and allowances
are unchanged. No replay flag, retained RAM or new idle buffer is added. These
compiler frames do not establish peak stack or hardware margin. Fresh bounded
GPT-5.6 Sol high closing review R1 is CLEAN for source, tests and the exact
four-lane resource proposal. All four final clean native resource gates pass
with clean `520e57a` firmware identities and exact reviewed measurements and
compiled-input matches. All four adoption checks against `9a6753e` and the
pinned full-history release gate pass. The first full run found a DEV event-type
mismatch in the shared host fixture; `5071aee` makes that fixture use the actual
DEV `ButtonEvent` or release byte payload. Firmware inputs and measurements
remain unchanged, as confirmed by all four subsequent adoption checks. Fresh
fixture follow-up review R2 is CLEAN. On clean `5071aee`, quick Linux passed
in 292.945 seconds and full Linux passed in 984.933 seconds. Attribution passed
in 113.840 seconds and the eight-case input runtime plus old-policy negative
in 106.504 seconds. Full every-page parity passed all 18 fixtures in 639.235
seconds. The stop test passed in 14.062 seconds; the reported timeout did not
reproduce, with no other suites or reviewer tests running in this worktree.
Evidence, including the first failed invocation and its correction, is under
`artifacts/newt31-input-*`. The Lead should verify burst Next presses, opposite
directions and Back/Sleep during foreground verification on the device.
Exact-source Mac ImageIO and physical acceptance remain separate. No device
was used and nothing was pushed.


## NEWT-31 timeout outcomes and saturated exits (2026-10-22)

Clean correction source `9dffd8d` measures 2,190,496 / 2,335,024 / 2,197,184 /
2,343,712 B (default / DEV USB / regional / regional DEV USB), deltas
+320 / +304 / +320 / +304 B from `83c9804`. Record 53 is refreshed exactly,
with its complete previous version preserved in
`artifacts/newt31-r4-original-flash-record-53.json`. Flash records 1–52 and
protected records 1–30 remain exact. Protected record 31 adds the transient
64 B timeout renderer; every previous selected key/frame remains unchanged.
Counts are 400/429/402/436. The selected cancellation-consumer boundary remains
measurable through noinline after its overflow caller was removed. SRAM stays
133,232 / 134,064 / 133,728 / 134,560 B; RTC stays 300 / 304 / 300 / 304 B.
No idle memory, queue capacity, floor, reserve, exclusion, adapter or allowance
changes. Compiler frames do not establish peak stack or hardware margin.
Fresh bounded closing source/resource review R2 is CLEAN, including exact
four-lane reports, hashes, history and pins.
All four final clean native resource gates pass with clean `8335b78` firmware
identities and exact reviewed measurements and compiled-input matches. All
four adoption checks against `9a6753e` and the pinned full-history release gate
pass. On clean `c6d1367`, quick Linux passed in 322.126 seconds and full Linux
passed in 1,152.200 seconds, including the focused combined queued-timeout and
32-slot Back/Sleep cases, both old-code negative controls and the shared UI
tests. Full every-page parity passed all 18 fixtures in 796.898 seconds. The
stop test passed in 13.983 seconds; the reported timing failure did not
reproduce. No separate suites or reviewer tests ran concurrently in this
worktree. Evidence is archived under `artifacts/newt31-r4-*`. The Lead should
check the timeout notice followed by a queued turn, and saturated navigation
followed by Back/Sleep on the device. Exact-source Mac ImageIO and physical
acceptance remain separate. No device was used and nothing was pushed.

## NEWT-32 device Settings (2026-10-22)

Home's third section is named Settings (formerly Your X3) and opens the
Settings page; the time zone row moves there from Reading settings
([071](071-device-settings.md)). Select on the Time zone row opens the zones
as a list over a frosted copy of the page, through the shared `ListOverlay`
component. The zone record, its codec and its commit are unchanged. Source
thresholds, adapters and generated exclusions are unchanged, and
`firmware/main.cpp` is 686 lines (683 on main), at its 686-line ceiling.

### Merge with NEWT-31

`5c09403` merges main `e3913e2` (records 51-53, docs 069 and 070). NEWT-31's
Reading settings and source-verification paths drop the zone argument, since
Reading settings no longer has a zone row. Its deferred-open case now checks
that Reading settings Save and Cancel never open or write the zone record,
and keeps its checks on full-source verification before book-state
publication.

### Card-only zone list moves

On the device at `53f443f`, each Next or Previous inside the open list took
about 1.23 s (`refresh_ms` 1,186), as long as opening it, against 0.70 s for
a Settings row move. The owner decided (2026-10-23) that a move redraws only
the card over the frost already on screen. `renderListOverlay` with a null
backdrop redraws just the card; `loop()` keeps list moves apart from other
changes, and after a pass of moves only, `paint`'s existing partial flag
reaches `paintZoneListMove`. It draws the card into the logical frame and
requests the card's native window, Fast (x 128, y 40, 536 x 448), or the
whole screen at the unchanged `UiRefreshPolicy` cleanup. The pinned X3
drivers run that window as a whole-panel Fast update. Nothing new is
retained: the frost is proven present by the paint order (doc 071), not
stored. `paint(bool, bool)` and `paintNonHomeScreen(bool)` keep their
signatures: the protected history cannot drop a selected entry, and a first
version that added a parameter grew the DEV `paintNonHomeScreen` frame from
96 to 112 B, so it was not kept.

### Record 54

The number is provisional; the Lead renumbers it at integration. Four clean
measurements of `e495afa` (the merge `5c09403` plus card-only moves) are
archived in
`artifacts/newt32-raw-code-health-{default,dev-usb,regional,regional-dev-usb}.json`.
The current receipts, rebuilt clean at `e438f33` after the latest policy pin
change (protected update 33) changed the build procedures, measure the same
values. Exact flash is 2,192,976 / 2,337,648 /
2,199,664 / 2,346,336 B (default/DEV/regional/regional DEV), growing by
2,480 / 2,624 / 2,480 / 2,624 B above main's record 53. Card-only moves add
688 / 832 / 704 / 832 B of that over the merge's 2,192,288 / 2,336,816 /
2,198,960 / 2,345,504 B. The usable limit is unchanged.

DEV images embed the 12-character build revision, so one DEV image can
differ by a few bytes between commits with identical build inputs: regional
DEV measured 2,346,352 B at `ab8a8ad` and 2,346,336 B at `e495afa`,
`b86d6fb` and `e438f33`. Exact adoption compares the record with the retained receipts, so
the record takes its values from clean receipts, and the receipts retained
after it were checked to match them exactly.

RAM is unchanged at 133,232 / 134,064 / 133,728 / 134,560 B. RTC before/after
is default 300->300, DEV USB 304->304, regional 300->300 and regional DEV USB
304->304 B. The Settings page flags, focused row, notice, the list's open
flag and its focus fill existing `AppState` padding, which a `static_assert`
pins at 24 B. The opening focus is the zero row, so a default `AppState`
stays all zero: an earlier nonzero default moved the `app` global from `.bss`
to `.data` and cost 16 B of SRAM. The zone labels are a const table in flash.
Card-only moves add no RAM or RTC: the move flag is a `loop()` local for one
pass.

Nothing is held while the list is closed, so no idle-state approval is
requested. An open list draws the Settings page into the display's native
buffer, frosts it into the logical frame and then `toNativeFrame` overwrites
the native buffer, as Sleep already does. `renderSettingsScreen` takes the
display rather than its buffer: passing the buffer kept that pointer live in
`paintNonHomeScreen` and grew its selected DEV frame from 96 to 112 B, so that
version was not kept.

No selected frame grows. `dispatchAction` falls from 896 to 880 B on the
DEV USB and regional DEV USB lanes, because Reading settings no longer saves
a zone draft. With the owner's approval, protected update 32 tightens those
two ceilings 896->880 B; its `before` keeps 896. `dispatchAction` stays 464 B
at its 464 B ceiling on the other two lanes, as on main. `paintNonHomeScreen`, setup and NEWT-31's
five time-zone boundaries are unchanged. Unselected frames:
`renderSettingsScreen` 48 B, `renderDeviceUi` 160 B (was 144), `renderSettings`
192 B, new `renderListOverlay` 160 B, new `paintZoneListMove` 48 B (DEV 80 B),
and the existing `frostSleepBackdrop` 912 B; `nextTimeZone` is gone. A move's
paint runs `paintZoneListMove` and `renderListOverlay` with no frost. The DEV
`paintHomeScreen` frame measures 80 B on both DEV lanes, down from 96 B with
its body unchanged. With the owner's approval (2026-10-23), protected update
33 tightens those two ceilings 96->80 B; its `before` keeps 96. An open list's paint runs
`renderSettingsScreen`, `renderDeviceUi`, `renderListOverlay`, then
`frostSleepBackdrop` (1,280 B of frames below `paintNonHomeScreen`).
Per-function frames do not bound whole task-stack or heap peaks. Floors,
exclusions, adapters, partition and reserve are unchanged; records 1-53 and
protected updates 1-31 are byte-identical.

Fresh GPT-6 Astra xhigh bounded reviews cover `9a6753e..HEAD`. Round 1
required changes: an unconfirmed save notice cleared on focus moves, the
wiring test extracted only the Save call, and two doc lines were stale. The
fixes in `ceaf671` add 16 B per lane. Round 2 (`9a6753e..6a7fa74`) is CLEAN.
Phase 2 (the Settings name and the frosted zone list) round 3
(`9a6753e..5180140`) is CLEAN. Round 4 (`5180140..86e0999`), of the merge and
this record, found two minor text errors: a stale non-DEV `dispatchAction`
size and doc 070's superseded Reading settings rows. Both are fixed, and
round 5 (`86e0999..f2c0d53`) confirms the fixes CLEAN. Round 6
(`53f443f..0113a8b`), of card-only moves and this record, found that record
54 named the `ab8a8ad` regional DEV measurement, 16 B above the retained
receipt, so exact adoption failed, and that the reason omitted the DEV
`paintHomeScreen` frame. Both are fixed in `b86d6fb`; receipts rebuilt clean
after it match record 54 exactly, and `--adopt e3913e2` passes for all four
variants. Round 7 (`0113a8b..539d48f`) confirms both fixes and found one
stale sentence on receipt provenance, now fixed. Round 8 (`539d48f..c1d9186`)
confirms that fix and the cover repaint census CLEAN. Round 9
(`a9961d3..e438f33`) finds protected update 33 CLEAN. Reviews are archived as
`artifacts/newt32-review-r1.md` to `artifacts/newt32-review-r9.md`.


<!-- Historical standalone NEWT-34 measurements; integrated record55 follows. -->


## NEWT-34 foreground envelope credit correction (r3, 2026-10-23)

Clean source `ae8b882` measures 2,194,848 / 2,339,392 / 2,201,536 /
2,348,080 B (default / DEV USB / regional / regional DEV USB), +48 / +64 /
+48 / +64 B versus `d8d987a`, and +4,352 / +4,368 / +4,352 / +4,368 B
versus `e3913e2`. The Lead-requested correction debits foreground page writes
for their CRC directory and 64-byte manifest (up to 4,160 B). Producer writes
keep their existing maximum reservation and are not charged twice.

Provisional record 56 stays standalone flash entry 54 / protected entry 32.
The exact combined record is refreshed against the same base, with its complete
r2 originals and prior tighter values preserved in
`artifacts/newt34-r3-original-records.json.gz` and Git history. All earlier 53 flash
and 31 protected records, floors, caps, reserves, exclusions and adapters stay
unchanged; the Lead renumbers at merge. SRAM stays 133,232 / 134,064 /
133,728 / 134,560 B; RTC stays 300 / 304 / 300 / 304 B. Every selected stack
frame is unchanged (413/442/415/449 selected, including the same 13 added
frames with fresh evidence). There is no new idle memory or descriptor.

Fresh GPT-5.6 Sol high focused review of `d8d987a..ae8b882` is CLEAN and
approves the exact four-lane resource record. It checks source, foreground
1/4096/4097/4MiB debit regressions and removed-charge control, unchanged producer
reservation, all report/input/output hashes, selected maps and adoption against
`e3913e2`. The approved proposal, matching raw reports and review are preserved
under `artifacts/newt34-r3-*`; the complete previous evidence remains intact.
The first clean measurement lanes compile and pass NVS/RTC guard checks but
stop at the old flash ceilings; final reviewed ceilings and lane receipts
follow this exact measurement. This is no relaxation of a memory floor.

Document 072 amends merged 069's `v[6]` attribution and describes the newly
widened cancellation window of full `pc-current` verification. Follow-up #98
tracks that exposure; retry/admission scheduling policy is unchanged here.
Final clean lanes on `6e252c6` pass all resource/NVS/RTC checks with the exact
approved sizes and compiled-input hashes. Adoption against `e3913e2` passes
for all four variants. Focused producer storage (including removed-debit
failure) and all 38 envelope fault/crash boundaries pass. Quick Linux passes
in 373.828 seconds; the pinned full-history release gate passes. The short
focused review is CLEAN. Complete evidence and hashes are listed in
`artifacts/newt34-r3-evidence.json`.

No device was used. Lead hardware timing, heap/stack and separate power checks
remain pending, as does exact-source Mac ImageIO. The historical r2 record
and validation narrative follows.

## NEWT-34 consumed-block page envelopes (r2, 2026-10-23)

Clean source `1d890e9` measures 2,194,800 / 2,339,328 / 2,201,488 /
2,348,016 B (default / DEV USB / regional / regional DEV USB), exactly
+4,304 B per lane from `e3913e2` and -80 / -96 / -80 / -96 B from r1.
At the Lead's direction, unmerged standalone flash entry 54 and protected
entry 32 are refreshed exactly; these are NEWT-34's provisional record 56,
which the Lead must renumber and remeasure when integrating sibling work.
Their complete previous versions remain in
`artifacts/newt34-r2-original-records.json.gz`; flash entries 1–53 and protected
entries 1–31 remain exact. Refreshing the single combined record binds the
current added-frame evidence to adoption against `e3913e2`; an additional
no-growth tightening record cannot replace that evidence under the gate.

SRAM remains 133,232 / 134,064 / 133,728 / 134,560 B; RTC remains
300 / 304 / 300 / 304 B. The 13 added query/codec/readback/admission frames
yield 413/442/415/449 selected measurements. Every inherited selected key
and frame remains unchanged. Versus r1, only attach tightens from 112 to
32 B; readback remains 80 B. The C3 verifier is 8,344 B (host 8,352 B),
under the 8,448 B compile-time bound inside the existing parse loan.
There is no new idle memory, descriptor, pointer or buffer. Producer buffers
remain work-scoped. No floor, reserve, exclusion, adapter or payload/page/open
cap changes. Static frames do not establish runtime stack or heap margins.

Fresh Claude Opus 5.5 closing review r2 approves the exact combined resource
proposal and reports no source defects. It independently checks all four
report/input/output hashes, unchanged SRAM/RTC, all selected frames and the
13 exact additions, growth-record validation, pinned policy validation, and
adoption against `e3913e2`. Raw reports matching the refreshed record and the
approved proposal are archived as `artifacts/newt34-r2-raw-code-health-*.json.gz`
and `artifacts/newt34-r2-resource-approved.json.gz`. The complete review is
`artifacts/newt34-r2-review-result.txt.gz`. Its documentation/provenance and
pending-gate findings are addressed by these r2 artifacts and the final
validation receipts below. Previous r1 artifacts remain available.

Retained producer admission uses the complete immutable verifier without the
foreground attempt/time budget. Interrupted work is Unknown; only confirmed
corruption permits repair. A rejected source-good guard revokes the query
before raw query I/O. Shared codec CRC/wire primitives and scalar incremental block
hashing remove duplicate algorithms. Reading the adjacent CRC directory and
manifest together saves one read per cache hit in the measured matrix. Native 63/64/147-block probes,
last-block damage, transient route refusal, bad source-good entry, cap/extent/
count geometry and combined-read controls expose the exact `b9cdb32` defects.
The independent probe cancel stops within the first block; dropping its
forwarding fails the oracle. All 38 candidate fault/crash boundaries remain.

The corrected 572-action matrix uses equal SD roots for both revisions.
A longer review-baseline root had exceeded FilePort's unchanged 256 B
semantic-metadata candidate path limit, causing misses and page reopens in
87 retained cells. Equal-path replays save exactly one read, with identical
bytes and opens in all 572 cells. Both the matrix and native I/O suite assert
same-chapter descriptor reuse. The first full r2 gate's long-named checked-exit
control exposed that guard; short control paths and an explicit native-path
headroom check make failures immediate and clear. The oversized-root control
requires that diagnostic. All 72 large displaced-turn cells retain a worst
byte ratio of 0.4852234 versus `e3913e2`. See
`docs/072-cache-envelope.md` for the full CSV and the Lead's DEV warm-up,
`pc-current` timing and cancellation script. The three-interruption prefetch
try-accounting question remains for Lead coordination with NEWT-33;
this implementation does not change admission scheduling or retain a
validation memo. Device timing, heap/stack margins, USB/battery/cold-power
checks and exact-source Mac ImageIO remain separate and pending.

The following paragraphs retain the r1 validation history. R2 final gate
results are recorded in the handoff and appended below.

The producer operation witness adds a
wire-byte term for full readback/directory work; the existing deadline and
no-buffer/no-receipt/unbounded-work negative controls remain effective.
See `docs/072-cache-envelope.md` for all 572 measured actions, small/retained
read costs, and exact Lead DEV telemetry and warm-up steps. Hardware timing,
free reading heap, stack margin, USB sleep/wake, battery sleep/wake and cold
battery power-on are pending separate Lead checks. No device was used and
nothing was pushed.

All four final clean native resource gates pass on clean `a316c37` firmware
identities, with exact approved sizes, selected frames and compiled-input
matches. All four adoption checks against `e3913e2` pass. The closing bounded
adversarial/maintainability review R2 is CLEAN for `e3913e2..a316c37`; its one
Low CSV line-ending finding is fixed. Quick Linux passed in 731.504 seconds.
The first quick invocation found the producer operation witness needed the
wire-byte term described above; its corrected focused suite and quick run
pass with all negative controls. Final full-gate evidence follows separately.

The first full invocation passed full every-page parity across all 18 fixtures
in 548.415 seconds, then reported an augmented reader-open profile compile
failure: its shared envelope test helper needed the firmware include path.
The one-line fixture command fix passes the focused profile suite. Firmware
inputs and approved measurements are unchanged. Both the failed invocation
and focused correction are retained under `artifacts/newt34-*`.

The second full invocation completed every phase, with one native owner fixture
failure: its manually constructed cache omitted the read authority now required
for checked envelope consumption. Production already supplies that authority.
The role fixture now supplies it and adds an otherwise-valid unwired-authority
role requiring ordinary pixel-identical fallback with no cache hit. Isolated
POSIX/FAT-lag runs and all exact archived-owner controls pass; bounded reviewer
follow-up is CLEAN. Firmware inputs and approved resources remain unchanged.
That full invocation also passed 18-fixture every-page parity in 530.561 seconds.
Its 1,944.772-second log and the isolated correction are preserved.

On clean `291242d`, the final complete Linux gate passed in 1,079.053 seconds
with four gate-runner jobs and zero failed phases. Both corrected fixture
suites pass inside that invocation; full every-page equality passed all 18
fixtures in 665.139 seconds. The stop test passed without a timing-only rerun.
No source edits or independent suites ran alongside that final gate. Final
Linux evidence and composed native owner input/execution receipts are archived
as `artifacts/newt34-full.log.gz` and `artifacts/newt34-receipt-fault-*.json.gz`.
Firmware inputs remain identical to the four clean `a316c37` build receipts.
Exact-source Mac ImageIO and the Lead's physical acceptance remain separate.


R2 final validation on clean source `84d46fb` passes all four clean firmware
lanes with exact approved flash, SRAM, RTC and selected frames. Their compiled
input hashes equal the reviewed `1d890e9` record; final canonical receipts and
`artifacts/newt34-r2-final-code-health-*.json.gz` preserve the fresh build.
All four `--adopt e3913e2` checks pass. The final quick Linux gate passes in
404.456 seconds and the complete Linux gate in 1,230.306 seconds, with zero
failed or skipped phases. Full every-page equality passes all 18 fixtures in
688.172 seconds; the turn matrix, I/O/removal controls, next-chapter prefetch
and admission-lease suites all pass within that run. The first 944.067-second
full run's long-root fixture failure and three dependent skips remain archived;
its corrected focused I/O suite passes before the final full run. The stop
suite passes without an isolated timing rerun. The pinned full-history release
gate passes. No device was touched, nothing was pushed, and no history was
rewritten. Exact-source Mac ImageIO and the Lead's physical checks remain
pending. Complete r2 evidence and hashes are listed in
`artifacts/newt34-r2-evidence.json`; r1 evidence remains intact.

## NEWT-34 main integration (record 55, 2026-10-23)

Normal two-parent merge `e37c1ba` preserves NEWT-34 `8bf8744` and main
`2f78920` (NEWT-32). Device Settings retains document 071; cache envelope
documentation and its counts CSV move to [072](072-cache-envelope.md).
The earlier standalone provisional record 56, stored as flash 54/protected 32,
remains historical in both-parent Git history and
`artifacts/newt34-integration-standalone-records.json.gz`; it does not replace any
accepted main record. Integrated flash record 55 follows all 54 main records.
Protected update 34 follows all 33 main updates, preserving every selected
entry and all earlier tighter values. Floors, reserves, exclusions and adapters
are unchanged.

Four clean measurement lanes at merged source `e37c1ba` establish:

| Variant | Main record 54 | Integrated record 55 | Delta | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,192,976 | 2,197,936 | +4,960 | 133,232 | 300 |
| DEV USB | 2,337,648 | 2,344,016 | +6,368 | 134,064 | 304 |
| regional | 2,199,664 | 2,204,624 | +4,960 | 133,728 | 300 |
| regional DEV USB | 2,346,336 | 2,352,704 | +6,368 | 134,560 | 304 |

SRAM and RTC are unchanged from main. No inherited selected stack bound
grows. The same 13 cache query/codec/admission frames are added per lane,
yielding 413/442/415/449 selected measurements. Scratch remains within the
8,448-byte compile-time limit in the existing loan, with no new idle state.
Raw reports, compiled inputs, exact added frames and core hashes are bound in
`artifacts/newt34-integration-record55.json.gz`. The first measurements compile
and pass NVS/RTC guards, then correctly fail only main's old flash limits.
Clean builds against the exact record and integration gate results follow.

The Lead reports USB timing on `8bf8744`, after matched warm-up: cached-open
total/v[6] is 1.90–1.99 s/371–409 ms at position 0, 1.59–1.63 s/340–348 ms
at position 1, and 1.59–1.61 s/322–326 ms at position 3. CQ probes are about
232–265 ms, zero, zero. Retained turns are within about 30 ms of baseline;
the last position-1 turn is about 105 ms in v[6] versus about 80 ms before.
This is Lead-supplied device evidence on the earlier head, separate from
merged compilation and host evidence. Heap/stack margins, other power states
and exact-source Mac ImageIO remain separate. At this integration handoff,
the Lead's fresh review was pending; no cross-family review was launched by
the implementation worker in that integration round.

Final integration validation at clean firmware source `3f3dafd` passes all four
clean resource/NVS/RTC lanes at the exact record-55 sizes and matching compiled
input hashes. All four adoption checks against main `2f78920` pass. Quick
Linux passes in 399.123 seconds; the full 24-fixture turn matrix passes 1,248
summary cells and 3,744 raw samples in 502.408 seconds. Full Linux passes in
1,094.946 seconds with no failed or skipped phases, including full every-page
parity for all 18 fixtures. The pinned full-history release gate passes at
the build source; the final artifact head is checked again at handoff.

The first quick invocation failed because the task's cleaned scratch directory
had not been recreated: Clang could not create a temporary file. Recreating it
was sufficient; the isolated trace suite and complete quick/full reruns pass
without a source change. The failed invocation and dependent skips remain
archived. All exact measurements, clean identities, history audit, CSV/raw
matrix samples, full parity receipts and gate logs are listed in
`artifacts/newt34-integration-evidence.json`. Lead review round 3 (Claude Opus
5.5, xhigh) of `d8d987a..ce2ebfa` is CLEAN. The subsequent P3 closure and its
focused review are recorded below. No device was touched, nothing was
pushed, no serviceTier was set, and both merge parents remain intact.

## NEWT-34 failed-exit closure (record 55, 2026-10-23)

Lead review round 3 (Claude Opus 5.5, xhigh), CLEAN on
`d8d987a..ce2ebfa`, is now recorded in flash record 55 and protected update
34. The focused second-family review of `ce2ebfa..56a6526` (Claude Opus 5.5,
high) is CLEAN and approves the exact refreshed four-lane proposal. These are
engineering reviews, with no owner or hardware acceptance claim.

An actual failed page descriptor/path check now withdraws its own generation's
descriptor at either the outer or nested checked exit. A typed Fresh/Refused/
PathFailed result keeps sustained and transient authority refusal from closing
a usable lease. The next presence hint cannot keep reporting the failed lease,
and the next turn cannot repeat reads through it. Explicit close permits a
fresh open. The regression fails on `ce2ebfa`; all nine removal controls fail
at their expected boundaries. Healthy syscall counts and DEV CQ rows remain.

Four clean measurement lanes at `56a6526`, independently approved before the
record refresh, establish these final record-55 values:

| Variant | Main record 54 | Refreshed record 55 | Delta from ce2ebfa | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,192,976 | 2,198,112 | +176 | 133,232 | 300 |
| DEV USB | 2,337,648 | 2,344,192 | +176 | 134,064 | 304 |
| regional | 2,199,664 | 2,204,800 | +176 | 133,728 | 300 |
| regional DEV USB | 2,346,336 | 2,352,896 | +192 | 134,560 | 304 |

All 54 main flash records, 33 protected updates and floors remain exact. Only
unmerged record 55/protected update 34 is refreshed. SRAM and RTC are unchanged.
No inherited selected frame grows: the retained stable wrapper tightens from
128 to 16 bytes; the single complete readFreshness checker is selected at
128 bytes, producing 414/443/416/450 selected entries. The checked-exit helper
has a 32-byte frame and other stable callers add the 16-byte wrapper. These
individual frame measurements do not establish an unchanged aggregate stack
chain or a physical stack margin. Scratch remains within the existing
8,448-byte loan; no idle memory, fields, buffers or descriptors are added.

The previous complete `ce2ebfa` record 55/protected update 34 is preserved in
`artifacts/newt34-r7-original-records.json.gz`. Five byte-identical raw archive
duplicates were removed normally, retaining the earlier gzip files and their
indexed hashes. No new raw duplicate is added. Main's prose before the NEWT-32
heading is restored exactly. Document 072 describes entry-path deferral as
well as presence hints and the failed-exit/cancellation behavior.

Final clean source `5d8fd60` passes all four resource/NVS/RTC builds at exactly
the approved compiled-input hashes and totals, plus adoption against `2f78920`
in every lane. Quick Linux passes in 284.824 seconds; complete Linux
passes in 950.514 seconds with full every-page parity for all 18 fixtures
and no failed or skipped phases. Its turn-cost, query-reuse, admission and
producer fault/removal suites pass. NEWT-36 subsequently moved per-fixture gate
parity evidence into `.build/reader-turn-parity/shard-N/` (`raw.json`,
`equality-raw.json`, `preparation.jsonl`); parent-level copies are historical
standalone output, not current sharded-gate evidence. See docs/CI.md for layout. The previous integrated full24-fixture
turn matrix remains preserved. The pinned full-history release gate passes;
final artifact-head release/adoption checks follow at handoff. All source,
review, rejected intermediate measurements, exact resource records, audits,
identities and gate evidence are indexed by `artifacts/newt34-r7-evidence.json`.
No device was touched, nothing was pushed, no history was rewritten, and no
serviceTier was set. Exact-source Mac ImageIO and the Lead's device stack
margin/new-head physical checks remain separate.

## NEWT-33 integrated idle saves (2026-10-23)

Flash record 56 and protected update 35 append to accepted main `8e882c3`.
Main's flash records 1–55, protected updates 1–34, floors and all other policy
history remain exact. The two-parent merge is `56ae241e`, with parents
`3f28fdea` and `8e882c3`. NEWT-34's page route revision 4, envelope, finalization
and document 072 stay intact. The save policy is now document
[073](073-idle-progress-save.md). Earlier unlanded NEWT-33 record-55 artifacts
remain historical; the current accepted append is in `code-health-baseline.json`.

| Lane | Flash record 55 → 56 | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,198,112 → 2,205,680 | +7,568 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,344,192 → 2,355,344 | +11,152 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,204,800 → 2,212,368 | +7,568 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,352,896 → 2,364,032 | +11,136 | 134,560 → 134,560 | 304 → 304 |

Selected keys grow 414→454 / 443→488 / 416→456 / 450→495, with no main key
removed or replaced by a wrapper. The shared store, admitted writable-begin
check and idle prepare/completion bodies remain measured. The branch-added
32-byte `ReaderRevisionSession::live` selection disappears because the helper
is always inline. Its measured callers grow: `applyMetadata` 32→48 bytes,
`open` 64→80 and `hashCompleteSource` 64→80. The DEV main save body grows
112→144 bytes for tracing/shared-save lifetimes; release stays 112. Exact
added keys and earlier unlanded consolidation removals are listed in record 56.
The four inherited identity lambda keys remain real bodies. Their stopped
counterparts retain 1,934 bytes of identity duplication in every lane under
the Lead's key-preservation decision; current symbols are archived.

The real Back/Sleep authored stack path is 2,480→2,320 bytes in release
(-160), and 2,512→2,544 bytes in DEV (+32). Settings' bounded changed-source
accepted-open reentry is 2,736→2,640 release (-96), and 3,216→3,248 DEV (+32).
These comparisons include outlined save functions, actual SD virtual bindings,
tail calls and source-constrained reopen paths. They exclude other translation
units, SDK/libc descendants and task/dispatch caller frames; they are not
runtime stack high water. There is no whole-chain DEV stack saving claim.

Admission/stop pointers are per-call; metadata debt uses an existing spare bit.
No resident buffer or memory held while a feature is unused is added. Identity
checkpoint stop scratch still adds 8 bytes of short-lived heap. Both durable
syncs, bounded lost-ack readback, record formats and the three-descriptor limit
remain. No floor, partition, reserve, adapter, exclusion or file allowance moves;
the minimum free-heap exception floor remains 50,468 bytes.

The exact proposal and static-chain evidence are under
`/var/tmp/newt33-evidence/impl/r8/`. The integration manifest binds compressed
external evidence. Existing ca70b85 tarballs and published history are untouched.
Host/native checks and Lead-provided USB observations are separate facts.
Exact-source Mac ImageIO, battery sleep/wake, cold power-on, minimum heap and
matching runtime stack margins remain physical/platform acceptance work.

The producer-pause authored stack path (while its parser is live) is
2,480→2,384 bytes in release (-96), and 2,528→2,560 in DEV (+32).
Its `Poll::pause` frame grows 32→112 bytes and `serviceProducerPause`
48→64; their growth is visible in the full compiler report and chain audit.
These are static authored contributions on top of a live parser, not a
whole-task stack bound or a runtime margin claim.

## NEWT-47 clock format (2026-10-25)

Flash record 57 appends to main `fe0b4b6c` (record 56); the Lead renumbers at
merge. It measures `1cbca21b`, which adds Settings' Time format row and its
durable `newt_clock` SD record ([074](074-clock-format.md)). Records 1–56,
protected updates and floors are unchanged.

| Lane | Flash record 56 → 57 | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,205,680 → 2,207,952 | +2,272 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,355,344 → 2,357,856 | +2,512 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,212,368 → 2,214,624 | +2,256 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,364,032 → 2,366,560 | +2,528 | 134,560 → 134,560 | 304 → 304 |

About 1,164 bytes are the new record's store instantiation, codec, commit and
restore. The rest is the third Settings row and its notes, the shared 12-hour
formatter in three renderers and dispatch; DEV adds the CLOCK `format=` field.
SRAM, RTC and every selected stack ceiling are unchanged: the format is one bit
in the clock latch's byte, Home's flag is a spare `HomeData` bit, and
renderers reuse their buffers. Nothing is held while the feature is unused.
Only unselected frames move: `renderDeviceUi` 160→176 bytes on the Settings
path; the new commit is 144 bytes. No floor, partition, reserve, adapter,
exclusion or file allowance moves; the minimum free-heap exception floor stays
50,468 bytes. The record's reviewer field states that no independent review
was run; Lead resource review is pending.

## NEWT-41 slow action feedback (record 58, 2026-10-25)

Flash record 58 appends to main `4b81d57f` record 57 (NEWT-47 clock format).
It measures the review r2 fix source `dfcad91f`, which also merges main
`87efd84c` (no firmware change). Records 1–57, protected updates 1–35 and
floors are unchanged. The feature is document
[075](075-slow-action-feedback.md).

| Lane | Flash record 57 → 58 | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,207,952 → 2,208,976 | +1,024 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,357,856 → 2,358,928 | +1,072 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,214,624 → 2,215,632 | +1,008 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,366,560 → 2,367,616 | +1,056 | 134,560 → 134,560 | 304 → 304 |

The growth is the working-frame renderer and copy, the shared paint helper,
the press record and their call sites. Against the r2 measurement at
`0473d8d6`, letting the refresh checks decide paints inside a press changes
0, 0, −16 and 0 bytes. No selected stack key or ceiling changes. The new
frames are unselected: `acknowledgePress` 48 B release and 240 B DEV (the
trace line buffer), `renderReaderWorkingUi` 64 B, `settlePress` and
`pressContinuesInReopen` 16 B each. DEV `paint(bool, bool)` falls 32→16 B;
its ceiling is not tightened here. The frame borrows the existing display
buffer, and the press record is one global byte, so nothing is held while no
press is pending. No floor, partition, reserve, adapter or exclusion moves;
the heap floor remains 50,468 B. Raw reports are under
`/var/tmp/newt41-evidence/`. There is no hardware claim.

## NEWT-40 reading statistics (record 59, 2026-10-25)

Flash record 59 and protected update 36 append directly to main `78e7046d`
record 58 (NEWT-41) and update 35. They measure `154aea94`, which adds
durable reading statistics ([076](076-reading-stats.md)) with review r2's
fixes and merges main. Records 1–58, protected updates 1–35, floors and pins
other than the two history pins are unchanged.

| Lane | Flash record 58 → 59 | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,208,976 → 2,231,872 | +22,896 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,358,928 → 2,382,464 | +23,536 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,215,632 → 2,238,528 | +22,896 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,367,616 → 2,391,168 | +23,552 | 134,560 → 134,560 | 304 → 304 |

The flash is the three statistics codecs and ledger, their SD store with two
repairs, the seed scan, the wiring, the RTC date read and two screens; DEV
adds the STATS row. It is about 2.3 times the design estimate. The r3 fixes
(one repeated write and an unconfirmed result instead of the readback
reconcile and the identical-record skip, the session exit wrapper and the
dated range) save about 500 bytes per lane against r2 plus record 58's
growth. Nothing is held while statistics are idle: the condition uses spare
save-notice bits, the per-open marks reuse the tracker's freed word, the page
reuses the Settings row flag, the carried session uses existing
`ReadingProgress` padding and the dated range uses former flag bytes of the
stored ledger.

Update 36 adds 32 selected frames at their measurements (fold, handover, seed,
saves, backend additions, RTC date read and screens) and raises four existing
frames by 16 bytes each, all measured: `finishReaderExitAction` 112→128,
`attemptIdleReaderSave` 192→208 (DEV 144→160), `SdReadingState::begin` 32→48,
and `serviceReadingProducerDeadline` 48→64 in release or `readHomeClock`
128→144 in DEV. The selected `SdReadingState::Active::directory()` frame is
unchanged, and `SampleReader::leaveFailedChapter` tightens 112→96. Against r2
(`f61dd846`), in all lanes: `storeFinalFacts` returns `StatsSave`, so its key
is renamed (224→240); seven added frames rise 16 bytes (`foldStats`,
`handoverStats`, `foldLeased`, `seedStats`, `loadLedger`, `readStatsPage` for
the exhaustive result handling, `foldLedger` for the dated range); the three
`stats_detail::save` frames and `summarizeStats` fall 16 bytes. From these
deltas, the fold paths below `finishReaderExitAction` are about 2,256 bytes
when a handover creates a book's facts file (+32) and about 2,166 bytes when a
fold saves the existing ledger (+16), against the unchanged 2,320-byte
Back/Sleep save path; about 2,464 bytes only if the ledger file vanishes
between its load and save. These are static compiler frames, not a runtime
margin claim. Main's merge moves no selected ceiling. No floor, partition,
reserve, adapter, exclusion or file allowance moves; the minimum free-heap
exception floor stays 50,468 bytes. No independent review was run; Lead
resource review is pending.

## NEWT-42 system UI consistency (record 62, 2026-10-25)

Flash record 62 appends directly to main `4606a896` record 59 (NEWT-40).
Records 60 and 61 are reserved for NEWT-43 and NEWT-39; the Lead renumbers at
integration. It measures `bfbf276b`, which draws every non-reader screen as
one shared `SystemPage` ([078](078-system-ui-consistency.md)). Records 1–59,
protected updates 1–36, pins other than the flash history pin, and floors are
unchanged.

| Lane | Flash record 59 → 62 | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,231,872 → 2,231,184 | −688 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,382,464 → 2,381,808 | −656 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,238,528 → 2,237,840 | −688 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,391,168 → 2,390,512 | −656 | 134,560 → 134,560 | 304 → 304 |

Flash falls because one page and row style replace each screen's own header,
footer, text hints and the Settings clock block. Record 62 tightens the flash
ceilings to these measurements. No selected stack key or ceiling changes, so no
protected update is needed. Two layouts were measured and not kept:
- Changing `renderBookOpeningUi`'s parameter dropped its selected key. It
  keeps `(const DeviceInfo&, uint8_t*)`, and a `SystemBar` overload shares
  its content.
- Holding the bar in `paintNonHomeScreen` grew its DEV frame from 80 to
  112 B, and building the book-opening bar in its caller grew that frame from
  48 to 80 B.

Instead, each system page reads its bar inside its own call
(`renderSettingsScreen`, `renderPreparedLibrary`, a tiny-app lambda), and the
clockless bar is built inside `SystemPage`'s constructor. Nothing is held
while a page is not shown. No floor, partition, reserve, adapter, exclusion or
file allowance moves; the minimum free-heap exception floor stays 50,468 B.
Raw receipts are under `/var/tmp/newt42-evidence/raw-receipts/`. No
independent review was run; Lead resource review is pending. There is no
hardware claim.

## NEWT-39 historical r2 standalone evidence (record after 59, 2026-10-25)

This section records the r2 measurements; integrated record 64 below supersedes its resource and stateless-runtime description.

This flash record appends directly to main's record 59 (NEWT-40). Its final
number is assigned by the Lead at landing: NEWT-51 takes record 60 and NEWT-43
record 61; if either lands first, this record must be re-chained after it
and re-measured. It measures `3e1912e7`, which adds Settings' Restart into
CrossInk ([077](077-return-to-crossink.md)) with cross-family review r1's
fixes. Records 1–59, protected updates 1–36, floors and pins other than the
flash history pin are unchanged. No protected update is needed: no RAM or
selected stack ceiling moves.

| Lane | Flash record 59 → this | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,231,872 → 2,235,520 | +3,648 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,382,464 → 2,386,224 | +3,760 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,238,528 → 2,242,176 | +3,648 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,391,168 → 2,394,928 | +3,760 | 134,560 → 134,560 | 304 → 304 |

The flash is the shared slot rule (slot heads, the bootloader's selection
record with its pending-abort rule, a whole-image check with SHA-256 through
mbedtls), the confirm page and its states, the exit-outcome gate, the
selection write with bounded readback reconciliation, the restart, and Newt's
16-byte image mark. The r2 fixes (the outcome gate, the readback outcomes and
two notes) add 560 / 560 / 544 / 560 bytes over r1 (`fb4abf65`). Nothing is
held while the feature is unused: the slot ports keep no state, the shown
state uses existing `DeviceInfo` padding, the page reuses `rowOpen`, and an
owed statistics fold uses the save notice's spare bit.

The switch runs out of line so `dispatchAction` keeps its 464 / 880-byte
ceiling (release / DEV); inlined, it grew to 496 / 928. Its own static frames,
none selected: `dispatchCrossInk` 32 / 48 (release / DEV), `EspAppSlots::verify`
144, `appImageIntact` 432, `select` 32, `commitOtaSelection` 160,
`readOtaRecords` 16, `locate` 48 and `read` 544 bytes. Below
`dispatchCrossInk`, the deepest static chain is `verify` → `locate` → `read`,
144 + 48 + 544 = 736 bytes (`verify` → `appImageIntact` is 576); `select` →
`locate` → `read` is 32 + 48 + 544 = 624, and `select` → `commitOtaSelection`
→ `readOtaRecords` 208. So the deepest switch path is `dispatchAction` +
`dispatchCrossInk` + 736 bytes, 1,232 / 1,664 bytes (release / DEV), plus the
callees of `esp_partition_read`. The save before it is the existing reader
exit path below `dispatchCrossInk`. These sums are static compiler frames from
the stack reports; they are not measured runtime stack margins, and none is
claimed. No floor, partition, reserve, adapter, exclusion or file allowance
moves; the minimum free-heap exception floor stays 50,468 bytes. No independent
review was run; Lead resource review is pending.

## NEWT-39 final integration (flash record 64, 2026-10-25)

Merged main `b90d6b7c` with both parents. NEWT-50 effective-sync proof and
reader-exit outcomes are unchanged; the switch consumes `SaveOutcome` directly.
The CrossInk row and confirm states use main's shared `SystemPage` layout.
Flash records 1–63 and protected updates 1–40 remain exact history prefixes.
Record 64 measures `13ea0b78`; no protected update 41 is needed.

| Lane | Flash before → after | Delta | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,232,352 → 2,236,080 | +3,728 | 133,232 → 133,232 | 300 → 300 |
| dev-usb | 2,386,576 → 2,390,400 | +3,824 | 134,320 → 134,320 | 304 → 304 |
| regional | 2,239,008 → 2,242,736 | +3,728 | 133,728 → 133,728 | 300 → 300 |
| regional-dev-usb | 2,395,792 → 2,399,616 | +3,824 | 134,816 → 134,816 | 304 → 304 |

The r3 boot-lifetime uncertainty bool survives page exit/re-entry and only a
safe readable OTA selection resolves it. It adds no measured SRAM capacity in
any lane; RTC is unchanged. Every selected static stack entry remains at its
main measurement; no protected ceiling changes. The exact nonselected switch
frames and section/alignment deltas are in record 64's reason and the receipt.
All floors, adapters, exclusions, partition and reserve remain unchanged.
The reviewer statement is retained verbatim in record 64. Raw four-lane reports
and logs are under `/var/tmp/newt39-evidence/final/measure/`; final clean-HEAD
gates and candidate identity are recorded in `RECEIPT-final.txt` in the parent
evidence directory. Static frames give no runtime stack-margin or hardware
acceptance claim.

## Heap at first book open (flash record 65, protected update 41, 2026-10-26)

The owner's daily build fell to 50,064 B heap-min-free during the first book
open, below the scoped 50,468 B floor (docs/061). The app package store (two
691 B NVS record copies) held 1,400 B of `.bss` for the whole boot but is used
once, by the boot app load. `loadBootApp` now allocates it on the heap
(nothrow) and frees it before Home's first paint and any boot-restore open. A
refused allocation reports the app unavailable and the boot goes on to Home.
No reserve, admission rule, workspace, fragmentation check or floor changes.

Measured at `b310654a` against base `96384402` (one build, `regional-dev-usb`):

| Flash before → after | Delta | SRAM before → after | Delta | RTC before → after |
| ---: | ---: | ---: | ---: | ---: |
| 2,399,616 → 2,400,912 | +1,296 | 134,816 → 133,440 | -1,376 | 304 → 304 |

Protected update 41 tightens the RAM ceiling to the measurement. Every selected
stack entry stays at its ceiling (setup 384 B, loop 112 B). The flash growth
funds the heap-owned store and a DEV line per book open,
`Open heap: low=<B> stage=<stage> largest-min=<B> xml-peak=<B>`
(`firmware/open_heap_watch.h`), which reuses IDF's local-minimum monitor and
prints `busy` when the cover observer holds it. The strict 1 KiB DEV static
assertion had 3 B left, so the watch's 12 B live in the DEV diagnostic heap
owner (84 → 96 B) and only the cover monitor's 1 B held flag is static. IDF's
monitor bookkeeping exists only during a watched open. Owner approved
2026-10-26: the DEV per-open heap watch holds 12 B for the whole boot.

The cross-family review (GPT-6 Astra, xhigh) found that a background
changed-source reimport escaped the watch: it runs after the loop pass's
action has finished, so its open marker was ignored. That reimport is now its
own DEV action, like the boot open. It is marked before the reimport and
finished after its page's paint, before the next Runtime report. The fix costs
64 B of flash (2,400,848 B at `ec51852d`); two out-of-line calls keep
`serviceChangedSource` at its 32 B frame. `tests/open_heap_watch_test.py` runs
the actual watch wiring against a model of IDF's monitor (complete, held,
taken and background-reimport opens).

Expected heap gain at the open low is about +1,350 B; the device measures it.
Host tests and this build give no hardware claim.

## RGB LED path removed (flash record 72, protected update 45, 2026-10-28)

The X3 has no RGB LED, but Arduino's `digitalWrite` keeps a branch for the C3
dev board's (`RGB_BUILTIN`, pin 30). That branch linked `rgbLedWrite`, the RMT
driver, FreeRTOS event groups and `timers.c`. `timers.c` replaces tasks.c's
weak no-op `xTimerCreateTimerTask`, so every boot started the FreeRTOS timer
task, with a 4,096 B stack and a 348 B TCB from the heap. Nothing in Newt or
the SDK uses the LED, RMT, event groups or FreeRTOS timers: no source names
them, the 842f364b map's cross references reach them only through this branch,
and every X3 profile pin is a real GPIO below 22. Arduino's own
`-DESP32_ARDUINO_NO_RGB_BUILTIN` (`esp32-hal-gpio.c:20-27`) removes the branch
([082](082-ota-updates.md) phase 2 step A found it).

Measured at `8f63be1e` against base `842f364b` (one build, `regional-dev-usb`):

| Flash before → after | Delta | SRAM before → after | Delta | RTC before → after |
| ---: | ---: | ---: | ---: | ---: |
| 2,414,592 → 2,391,392 | -23,200 | 133,440 → 126,704 | -6,736 | 300 → 300 |

Both ceilings tighten to the measurement; protected update 45 lowers only RAM
and every selected stack entry is unchanged. Static memory takes heap one for
one, and the missing timer task frees about 4.5 KB more, a heap gain that no
ceiling sees. `tools/verify-nvs-guard.py` therefore fails the health build if
`timers.c`'s `xTimerCreateTimerTask` or the `rgbLedWrite`/`rmtInit` entry
points come back; `tests/native_timer_task_test.py` holds its controls, and the
check fails on the 842f364b image.

Bench X3, USB power, one book and one 36-page journey per build (installed
40f0bb9e, then `c56cfc66`): boot free heap 134,608 → 146,016 B (largest block
114,676 both); reader admission free 131,656 → 143,132 B; lowest free heap
while reading 51,292 → 62,472 B, against the 50,468 B floor; loop stack low
6,412 → 6,400 B. One journey does not set a general minimum, and USB sleep/wake,
battery sleep/wake and cold power-on were not run with this build.

## Wi-Fi base, image (c) (flash record 74, protected update 46, 2026-10-28)

Update mode's Wi-Fi networks page links Wi-Fi (no `esp_netif` or lwIP).
Wi-Fi's static data sits in `.dram0.newt_wifi`, which every boot but update
mode lends to the heap; libphy and libesp_ringbuf IRAM code run from flash
([084](084-wifi-memory.md)). The owner approved this design on 2026-10-28
("You can build wifi on (c)"); `idle_owner_approval` quotes it, because update
mode keeps the range while Wi-Fi is idle there.

Measured at `b438bb29` against base `ae405596` (one build, `regional-dev-usb`).
Fix round 1 replaced both records in place before merge: the lend's priority,
the DEV big-block rows and a 5,104 B zero pad that lets the range hold the SD
card's FATFS context add 5,344 B of flash and change no resident RAM. Fix
round 2 replaced them again: the fence that keeps Wi-Fi from writing flash
and the scan row's counts of what it refused add 272 B of flash and change no
RAM, RTC or selected stack.

| | Before | After |
| --- | ---: | ---: |
| Flash | 2,391,536 | 2,857,792 (+466,256) |
| Resident SRAM (`ram_bytes`) | 126,704 | 125,280 (-1,424) |
| Lent range (`wifi_lent_bytes`) | none | 22,528 (17,424 Wi-Fi, 5,104 pad) |
| RTC | 300 | 300 |

Flash and RAM ceilings equal the measurement; the lent range enters at its
measurement through growth record 46. All 522 selected stack ceilings are
unchanged. The lend returns the whole range, after the RAM heap, and Wi-Fi
holds no heap in a reading boot (its startup log-tag entries are freed before
`setup()`; 084). The pad comes out of the RAM heap; update mode, which does
not lend, leaves it idle (5,104 B less heap there). The reviewer is pending:
the Lead fills it after review. `tools/verify-wifi-link.py` checks
the linked image after every health build; its limits are in 084. Host and
compile evidence only; the bench check is the Lead's receipt.

## Wi-Fi setup (flash record 76, 2026-10-29)

Update mode picks a network, takes its password on FreeInk's keyboard and
keeps it as an AES-256-GCM sealed card record plus an NVS key
([085](085-wifi-setup.md)). Nothing joins a network yet.

First measured at `ce5bc747` against base `32626189` (one build,
`regional-dev-usb`). After #157 merged, it follows #157's flash record 75 and
was re-measured on the merged source at `b81fbdce` (main `e0615585`).
A code-health fix round re-measured it in place before merge at the same
flash. It also admits FreeInkUI's `components/keyboard/keyboard.h` as an
external adapter for `src/update_screen.cpp`: the keyboard's layout and focus
model only, no hardware or storage. The adapter is for review.
Review fix round 1 re-measured it in place again (+64 B): one update-mode
exit wipes the password and the entropy pool before every restart, and an
explicit save or Forget may replace a damaged record. RAM, RTC and the
selected stacks are unchanged; `leaveUpdateMode` is a new 16 B frame.
The merge round re-measured it: before is record 75's after, resident SRAM
comes from #157's protected update 47, and the only new frames are #157's.
#157's crash output guard now names a console-register word outside every
object by its input section in `firmware.map`; only C++ exception tables
(`.gcc_except_table.*`) are reviewed that way. The merge moved one reading
function's table so four of its bytes read as a UART0 register address.
A last fix before merge re-measured it in place at `4eafc4da` (+384 B):
Forget erases the NVS key instead of replacing it, so a full NVS cannot keep
it. RAM, RTC and the selected stacks are unchanged; `eraseWifiKey` is a new
80 B frame. The reviewer is pending again until the Lead checks the fix.

| | Before | After |
| --- | ---: | ---: |
| Flash | 2,858,944 | 2,878,512 (+19,568) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

From this build's map: FreeInkUI's keyboard is about 7.2 KB and mbedTLS's
cipher, cipher_wrap, gcm and esp_aes_gcm about 11.1 KB; the pages, the store
and update mode's restore, save and Forget make up the rest. No RAM, RTC or
selected stack ceiling moves, so there is no protected update. The new
frames are unselected: `MbedWifiCipher::seal` 464 B and `open` 480 B,
`saveWifiNetwork` and `restoreWifiNetwork` 352 B, `forgetWifiNetwork` 176 B.
`runUpdateMode` grows 704 → 912 B: its view holds the password and keyboard.

mbedTLS's allocator hooks and AES DMA state already sit in the lent range, so
`tools/verify-wifi-link.py` gains two reviewed library entries, the cipher's
`seal` and `open` (never inlined or cloned), each allowed only from
update-mode code (`tests/wifi_link_test.py`). This is a rule change for
review, not an exclusion: any other caller still fails. The reviewer is
pending: the Lead fills it after review. Host and compile evidence only.

## Wi-Fi join and heap clear (flash record 77, 2026-10-29)

Update mode's Test row joins the saved network once (esp_netif and lwIP),
and every boot clears the heap's RAM before IDF registers it
([085](085-wifi-setup.md) "Slice B, as built"). First measured at `44e08145`
on `feat/wifi-setup` (2,876,960 → 3,023,648 B), then re-appended after slice
A's record 76 and re-measured at `777de18c` on the source merged with main
`c77f0644` (one build, `regional-dev-usb`). Before is record 76's after;
resident SRAM comes from #157's protected update 47.

| | Before | After |
| --- | ---: | ---: |
| Flash | 2,878,512 | 3,025,200 (+146,688) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 (pad 5,104) | 22,528 (pad 1,856) |
| RTC | 300 | 300 |

From this build's map: lwIP is about 127 KB and esp_netif about 11 KB; the
join, the Test page, the heap clear and the driver code the join reaches make
up the rest. lwIP's and esp_netif's `.data`/`.bss` (3,172 B) went into the
lent range, so no RAM, RTC or selected stack ceiling moves and there is no
protected update. The new frames are unselected: `joinWifiNetwork` 400 B,
`forgetWifiBeforeRestart` 208 B, `connectUpdateNetwork` 144 B, the heap
clear's wraps 16 and 48 B. `runUpdateMode` grows 912 → 928 B.

`tools/verify-wifi-link.py` gains two reviewed entries, `joinWifiNetwork`
(from `connectUpdateNetwork` only) and `forgetWifiBeforeRestart` (from
update-mode code), and one precision rule: in Newt's own objects, a data word
that equals a function start is a pointer only where the object relocates it
(`tools/object_relocations.py`). One word of the reader font's pixels equals
lwIP's first function's address. Archive members are unchanged, and an
unreadable object stops the check. The merge also links lwIP into #157's
console register check: `ip6_output_if_src` builds the IPv6 header's first
word (version 6 << 28) with `lui 0x60000`, which equals UART0's base, so
`tools/crash_output_guard.py` names it as one reviewed value, never an
address. These are rule changes for review, not exclusions. The reviewer is
pending: the Lead fills it after review. Host and compile evidence only.

## Wi-Fi Settings row (flash record 78, 2026-10-30)

Settings gets its own Wi-Fi row, which restarts into update mode on Wi-Fi
networks; the Update page loses its Wi-Fi row, and `updateUsbPower()` counts
a computer's USB by `HWCDC::isPlugged()` instead of an open serial port
([085](085-wifi-setup.md) "Flow"). Measured at `5784a529` on
`feat/wifi-settings-row` against main `72cad783` (one build,
`regional-dev-usb`). Before is record 77's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,025,200 | 3,025,280 (+80) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The new row, its two notes and `openNetworks` cost slightly more than the
removed Update-page row, note and networks focus. `isPlugged()` calls
`usb_serial_jtag_is_connected`, which the link already held. The update
request reuses the retained SdResumeRecord words, so RTC does not grow.
Selected stacks are unchanged: `runUpdateMode(bool)` stays at 928 B,
`renderUpdate` shrinks 368 → 352 B and `openNetworks` is 0 B. Reviewer: GPT-6
Astra (xhigh), 2026-10-30: MERGE, no P0–P2; its one P3 (the host preview) is
fixed. The Lead's bench check passed on USB; the owner's battery check is
pending.

## GitHub Releases over Wi-Fi (flash record 79, 2026-10-30)

Update mode checks GitHub's latest release over HTTPS and downloads a newer
Newt to the card, where the card installer takes it
([088](088-ota-wifi-install.md)). Measured at `470a7492` on
`feat/ota-wifi-install` against main `d4698e9d` (one build,
`regional-dev-usb`). Before is record 78's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,025,280 | 3,228,624 (+203,344) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The flash is the generated certificate bundle (121 roots, 56,069 B),
`esp_http_client`, esp-tls, `tcp_transport` and `http_parser`, the mbedTLS
TLS, X.509 and crypto code a verified handshake needs, and Newt's check,
download and status pages, with each file's one time limit (a 1 s
`esp_timer` that shuts its sockets once the limit passes). Resident SRAM stays flat because
`tools/wifi-sections.py` puts the TLS and HTTP archives' static state, and
esp-tls's DS lock, in Wi-Fi's lent range (now 80 B from full), which every
reading boot lends to the heap. The DS lock's constructor, a heap mutex,
leaves boot's list and runs only as update mode starts.
`heap_caps_malloc_prefer` moves from IRAM to flash: its one caller,
`esp_ds_rsa_sign`, is flash code, and `tools/verify-wifi-link.py` fails a
future IRAM caller. Selected stacks are unchanged. Reviewer: pending (the
Lead fills it after review). Device checks are pending.

## TLS on the lent frame (flash record 80, 2026-10-31)

Check lends the idle 52,272 B logical frame to mbedTLS while each TLS session
is open, so GitHub's handshake and record buffers fit beside Wi-Fi in update
mode ([088](088-ota-wifi-install.md), "Memory"). Measured at `8aa1ef86` on
`fix/ota-tls-frame` against main `a7c63ca6` (one build, `regional-dev-usb`).
Before is record 79's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,228,624 | 3,229,728 (+1,104) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The flash is the frame loan (a standalone `multi_heap` over the frame,
`mbedtls_platform_set_calloc_free` and a calloc/free pair that routes by task
and by address), its reclaim and restart-to-list path, the download bar grown
on the panel's buffer (`fillNativeRect`, through `paintUpdate`'s one panel
call), and update mode's DEV failed-allocation row. Resident SRAM stays flat:
the loan's handle and lock live in the frame itself and the per-check state on
the stack. Selected stacks are unchanged. A second `displayBinaryFrame` call
site had moved its trace lambda out of line and grown `paintNonHomeScreen`
96 → 112 B, so the bar goes through `paintUpdate` instead. Unselected
update-mode frames grow on the loop task: `fetchUpdateOnline` 304 → 336 B and
`openFollowing` 224 → 240 B. Reviewer: pending (the Lead fills it after
review). Bench numbers are in 088's "Memory".

## Derived records beside Pages (flash record 84, 2026-10-31)

Progress estimates, semantic tables and checkpoint identities move from
`/Newt/Cache/Pages` to `/Newt/Cache/Meta`, so the census's 128-name window
holds only page-cache files ([065](065-chapter-cache-producer.md), "Derived
records beside Pages/"). Measured at `a0fbfe88` on `fix/derived-records-folder`
against main `f34f7dc0` (one build, `regional-dev-usb`). Before is record 83's
after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,229,648 | 3,230,256 (+608) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The flash is the folder choice, FilePort naming a second folder, the legacy
`Pages/` read after a confirmed `Meta/` miss, the one-time copy of a
validated legacy record into `Meta/` (`adoptLegacy`, `noinline cold`) and
the census ranking legacy names before pages. The new `legacy_` flag fits
existing padding. Selected stacks are unchanged; `fileSize` stays 32 B because
its lease stays a register local. No heap is held: adoption allocates one
transient block (the record plus 128 B, at most about 4.5 KB) when a pre-#179
record is first read, and frees it before returning. Reviewer: GPT-6 Astra
(xhigh), Lead-run cross-family review of #184; its one P3 (a retained legacy
copy can return after a later cut leaves neither `Meta/` name) is answered by
the repeated-save cold-cut test and 065's qualified adoption claim.

## Segmented chapter cache (flash record 85, protected update 48, 2026-11-01)

Chapters over 128 pages are cached as 128-page segments
([065](065-chapter-cache-producer.md), #175). Measured at `dafce2ed` on
`perf/segmented-chapter-cache` against main `4df25f83` (one build,
`regional-dev-usb`). Before is record 84's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,230,256 | 3,232,384 (+2,128) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |
| `ChapterPageCache::show()` frame | 208 | 224 (+16) |

The flash is the producer's segment boundary and resume, the segment name,
the segment read and its totals checks, per-byte producer card credit, the
census and stored-hint probes over segment sets, and the census grant of new
producer files below an 8-entry reserve, which storage enforces per file.
Under the reserve the census reclaims pre-#179 derived records left in
`Pages/` before pages. The grant shares one byte with the two-slot
absent-name index, so it and #179's `legacy_` flag stay in the session's
existing padding. No heap is held while reading; the producer stays in the
display tail. `show()` grows because one more value stays live across the
new segment call. Inline, the attempt measured 240 B; out of line, `show()`
is 224 B and `readSegment()` 64 B (not selected). Every path through
`show()`, including the parse fallback, is 16 B deeper, and a segment read
80 B deeper. The loop task's high-water on a segmented chapter is a device
check. Reviewer: Claude Opus 5.5 (Lead) review 2026-11-01 of the #177 rebase onto #184 (a67abfa7..ac56d20e), after GPT-6 Astra (xhigh) review and confirm rounds of #177 on 2026-10-31 found MERGE: the rebase's one behavior change, reclaiming pre-#179 names oldest-first before pages once Pages/ holds 120 names, keeps the 8-entry reserve and the census's 128-deletion bound; the storage tail stays 1,952 B with both fields packed in one byte zeroed by the constructors; on the bench, main a67abfa7 and #177 matched on boot heap low (89,796 B against the 50,468 B floor), open heap low and stack min free; the segment benefit is not yet hardware-proven because the census did not finish on the bench card. Bench-checked 2026-11-01 (#191, 35fd1806): reading heap low 58,824 B, 8,356 B above the 50,468 B floor; loop stack low 6,412 B; owner device acceptance is pending.

## Census converges on a full Pages/ (flash record 86, 2026-11-01)

The idle census reads the cache folder through FatFs entries and deletes up
to 8 victims per scan ([065](065-chapter-cache-producer.md), #175). Measured
at `33832c95` on `perf/census-converges` against main `123a7405` (one build,
`regional-dev-usb`). Before is record 85's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,232,384 | 3,232,400 (+16) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

No selected frame changes. The census path, which is not selected, moves
from `pageCacheEntryCensus` 1,184 + `scan` 16 + `scanNames` 368 B to
`pageCacheEntryCensus` 1,072 + `scan` 112 + `readPageEntries` 592 +
`visit` 48 B. It runs only in the reader loop task's idle census. The FatFs
folder read keeps its directory on that stack, where the VFS directory took
a transient heap block. No heap is held while reading. Reviewer: GPT-6 Astra (xhigh), Lead-run cross-family review of #190 at 33832c95: MERGE, no P0-P2 findings. Its two P3s are documentation (the convergence fixtures prove progress across repeated census runs, not completion within one reading visit, a limit that predates this change; the doc's test command no longer passes a Python --converge flag) and are fixed in this PR. Bench-checked 2026-11-01 (#191, 35fd1806): reading heap low 58,824 B, 8,356 B above the 50,468 B floor; loop stack low 6,412 B; owner device acceptance is pending.

## A crowded census frees entries per segment written (flash record 87, 2026-11-01)

On a crowded `Pages/` the census freed one entry per attempt, so the second
segment of a long chapter was refused (#191). The target probe now walks every
segment of the shown chapter: it counts the missing ones, up to a batch of 4,
and records each segment's name in a conservative set. The census frees an
entry per counted file and never evicts a page in that set
([065](065-chapter-cache-producer.md)). The set shares one type with the
existing backup filter. Measured at `344b19bd` on `fix/segment-oom` against
main `aad14912` (one build, `regional-dev-usb`). Before is record 86's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,232,400 | 3,232,800 (+400) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

No selected frame changes. The idle admission path, which is not selected, is
deeper: `admitPageCache` 208 → 256 B, its census `backgroundWork` lambda
48 → 64 B, `pageCacheEntryCensus` 1,072 → 1,088 B and `pageCacheStored`
224 → 256 B. It runs only in the reader loop task's idle admission. No heap is
held while reading. Reviewer: GPT-6 Astra (xhigh), Lead-run cross-family review of #193: FIX FIRST at 41251c0f (P2: admission reserved the chapter's total segment count, over-evicting on resume and past the attempt limit), then MERGE at 72b5c35e with no P0-P2 findings. Its P3 (present-but-invalid segments count as new names, over-reserving up to the 4-file batch) is tracked in #194. Bench-checked 2026-11-01 (#191, 35fd1806): reading heap low 58,824 B, 8,356 B above the 50,468 B floor; loop stack low 6,412 B; owner device acceptance is pending.

## CrossInk history imports once at Home idle (flash record 89, 2026-11-01)

#104 imports CrossInk's reading statistics once, read-only, in Home idle time
after the ledger is seeded ([076](076-reading-stats.md) section 6). Measured at
`841da073` on `t3/30da7afa-104` against main `aa2535f9` (one build,
`regional-dev-usb`). Before is record 88's after. Expect to renumber at rebase.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,232,768 | 3,245,104 (+12,336) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

No selected ceiling changes. `summarizeStats` keeps its selected
four-parameter body (256 B), used when nothing was imported, because a key
cannot migrate yet (#100); the CrossInk overload inlines the same body and
measures 272 B. `readStatsPage` measures 32 B (ceiling 240) because
`summarizeStoredStats` (384 B, not selected) is no longer inlined into it.
The import's frames are not selected and run only in the loop task's Home
idle: `serviceCrossInkImport` 256, `importCrossInk` 432, `bindBooks` 224,
`bindBook` 224, `importSource` 384, `hashBook` 832, `importGlobal` 512 B.
`loadCrossInk` (688 B) runs only for the stats page. Nothing is held while
reading or after the import. The crash output guard's reviewed data gains the
embedded certificate bundle (`EMBEDDED_DATA`): four of its DER bytes spell an
address inside `panic_print_registers` or `print_assist_debug_details`,
depending only on link layout. Reviewer: GPT-6 Astra (xhigh), Lead-run
cross-family review of #200 at 50fcf5a4: MERGE with no P0-P2 findings,
including the guard exception. Its P3 (no importer revision for a re-import
after a fix) is tracked in #201. Device checks are pending.

## The system bar says Wi-Fi while Wi-Fi may run (flash record 90, 2026-11-01)

#198: `readSystemBar` reads the Wi-Fi flash fence (`firmware/nvs_guard.h`), so
every SystemPage paint during a Wi-Fi run says "Wi-Fi · N%", and one formatter
with one copy constant (`include/newt/system_page.h`) serves Home, every
SystemPage and Airspace's landscape bar. Measured at `2ba4c461` on
`t3/198-wifi-status-bar` against main `5b4e2dee` (one build,
`regional-dev-usb`). Before is record 89's after. Expect to renumber at rebase.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,245,104 | 3,245,168 (+64) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

No selected frame changes. `AirspaceFrame`'s constructor (not selected, not
yet on the device) measures 208 B (was 176) for the formatter's 24 B buffer.
No memory is retained or held. Reviewer: GPT-6 Astra (xhigh), Lead-run
cross-family review of #206 (5b4e2dee..4a670595): MERGE with no P0-P2
findings. Its P3 (the Scan, Test and update-check join frames paint before the
fence rises) is tracked in #207. Device checks are pending: the owner sees the
bar during the next OTA update.

## Anchors read a chapter's segment map (flash record 91, 2026-11-01)

An open, wake, Chapters jump or crossing into a long chapter ran the ordinary
parse (#192). The producer now writes a small per-chapter map, and `show()`
reads an anchor from the segment it names, after one memoized lookup of the map
([065](065-chapter-cache-producer.md)). Measured at `8bfe97e9` on
`feat/segment-anchor` against main `cfd92007` (one build, `regional-dev-usb`).
Before is record 90's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,245,168 | 3,249,280 (+4,112) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

A mapped segment read takes the ordinary read allowance at a cold open: a
segment of prose exceeds the 128 KiB whole-file opening cap, which whole files
keep. Admission refuses a segmented chapter whose style slot is unsettled (an
open served from a segment) before the census evicts anything. No selected ceiling changes. `readCached` keeps its guarded signature over one
`always_inline` body that `readMapped` inlines with the map's segment: a new
parameter would rename the selected key, which the history cannot record. The
second copy is about 620 B of the growth. The SD page storage remembers three
absent names, not two, so an anchor's absent map does not evict its chapter's
own name; packed flags keep the storage at its size (368 B on the host), and
`identityFileSize` stays inline so the selected identity-record callback keeps
its 32 B frame. Measured selected frames are tighter: `show` 224 → 208 B and
`readCached` 192 → 176 B (ceilings kept). New frames, not selected:
`readMapped` 112 B plus its lambda 176 B, `locate` 80 B,
`ChapterSegmentMap::find` and `check` 80 B each, `encode` 64 B, `name` 32 B,
`writeMap` 48 B, `GuardedStore::present` and `beginRead` 32 B each,
`finishRead` 16 B and the `VerifiedPageStore` constructor 16 B;
`ChapterCacheProducer::produce` 80 → 96 B, its `Attempt` constructor 16 → 32 B
and `ReaderActionProfile::boundary` 16 → 32 B. A mapped open is
208 + 112 + 176 = 496 B before the read's callees, 16 B deeper than main's
segment turn (480 B). The map record and the producer's segment starts live
in the leased parse workspace; no heap is held while reading. Reviewer: GPT-6
Astra (xhigh), Lead-run cross-family review 2026-11-01 of #210
(cfd92007..7391e9dd): MERGE with no P0-P3 findings; it checked anchor parity,
all nine map guard-removal controls, oversized-segment reopening, crowded-cache
accounting and the baseline retry controls, and found that 86afca7c does not
weaken the baseline assertions. Device checks are pending.

## Opt-in Sleep screen extras (flash record 93, 2026-11-01)

#202: a Settings "Sleep screen" page after "Time format" holds three rows, all
Off by default, each an acknowledged durable SD choice: "Book progress", "Last
7 days" and "When you read" (Night inverts the whole sleep frame). The witch
hat (Oct 31) and the Christmas hat (Dec 24-25) are always on. The streak is a
cache in its own 28 B `NWTK` record, so `NWTS` stays v1 and a rollback to
alpha.6 keeps its statistics. Home idle rebuilds it once per boot and writes
nothing when a month or the CrossInk history cannot be read. Measured at
`ab99c60b` on `t3/202-sleep-extras-build` against main `ab73fc22` (one build,
`regional-dev-usb`), after the review's three P2 fixes. Before is record 92's
after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,249,360 | 3,265,056 (+15,696) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The growth is the extras renderers and glyphs, the Sleep screen page and its
three records, the sleep-time stats read and the `NWTK` record, plus second
copies of the sleep paint and renderers. The copies let the all-Off path keep
its measured stack keys, because a key cannot migrate yet (#100). No selected
ceiling changes. The all-Off renderers are the pre-#202 bodies:
`renderSleepScene` and `renderSleepSceneWithHomeStyle` 128 B,
`paintSleepScreen` 160 B, `paintDevelopmentSleep` 32 B and the 4-argument
`summarizeStats` 256 B. The sleep path gains `paintSleepFrame` (48 B, not
selected) between `paintNonHomeScreen` and `paintDevelopmentSleep`. New
frames, not selected: the extras `paintDevelopmentSleep` 48 B, the extras
`renderSleepSceneWithHomeStyle` 144 B and `renderSleepScene` 128 B,
`finishSleepFrame` 64 B, `renderSleepExtras` 80 B, `portraitSlip` 272 B,
`readSleepStats` 432 B, `sleepWeek` 112 B, `refreshSleepStreak` 464 B (Home
idle), the 5-argument `summarizeStats` 272 B, `loadStreak` 96 B and the 28 B
record's save 96 B. `SleepExtras` lives only on the sleep path's stack, and no
memory is retained or held.

Sleep reads: none with every extra Off or Book progress alone; When you read
alone reads `NWTS` (180 B); Last 7 days reads `NWTS` and `NWTK` (28 B), plus at
most one archived month (104 B) in a month's first 6 days.

The crash output guard now reviews the five sleep mark run tables as glyph
data, as it already does `kHomeRuns`: in this layout, a word of `kSunRuns`
equals a code address by chance, and run bytes are never loaded as a pointer.

Reviewer: GPT-6 Astra (xhigh) cross-family review of PR #212; three P2 fixes
confirmed by the Lead. Device checks are pending: sleep paint timing,
ghosting after a dark sleep screen, and battery sleep/wake on the main X3 are
owner hands-on checks.

## When you read counts two months (flash record 95, 2026-11-02)

#202 follow-up: on Nov 1 one afternoon session outweighed a month of nights,
because the time-of-day mark counted only this month. The mark is now the
most-read bucket over this month and last month, plus today's unsaved
seconds, with the 30-minute floor over both. Last month comes from the ledger
while it is still on last month, else from its archive. The archive is read at
most once per sleep and shared with Last 7 days: every day while When you read
is On, else in a month's first 6 days. A month with no file holds nothing; one
that cannot be read shows no mark. The Settings note gains "once you've read
30 minutes" (owner-approved copy). Measured at `7424f73d` on
`t3/202-habit-two-months` against main `76e6d253` (one build,
`regional-dev-usb`), after the review's P2 fix and the live sleep progress
fix. Before is record 93's after. #199 holds record 94; this
record becomes 94 if it merges first.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,265,056 | 3,265,344 (+288) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,296 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

No selected ceiling changes. Not selected: `readSleepStats` 432 → 448 B
(+16), `readingHabit` 64 B, `sleepArchiveMonth` 16 B, `sleepTodayArchived` 16 B; `sleepWeek` 112 B and
`refreshSleepStreak` 464 B are unchanged, as is `finishReaderExitAction` 128 B. No memory is retained or held.

Sleep reads (record bytes): none with every extra Off or Book progress alone;
When you read alone reads `NWTS` (180 B), plus last month (104 B) when it is
archived; Last 7 days alone reads `NWTS` and `NWTK` (28 B), plus last month in
a month's first 6 days; both read `NWTS`, `NWTK` and last month when it is
archived, at most 312 B.

A clock behind the ledger (today's month archived) shows no dots, count or
mark, since today's month is unread (the review's P2).

Owner-approved fix: Sleep from the reader painted Home's last snapshot in Book
progress (the old chapter and percent after crossing into the next chapter),
because the sleep paint runs before the exit refreshes Home. The exit now
notes the live page's chapter and percent into Home first: no card read,
nothing held.

Reviewer: GPT-6 Astra (xhigh) cross-family review of PR #217; one P2 fix and
the live sleep progress fix confirmed by the Lead. Device check pending: the mark on the main X3 across a
month boundary is an owner hands-on check.

## Instant resume picture (flash record 96, protected update 49, 2026-11-02)

The reader saves the page it leaves as its exact panel image in one card slot,
and the next wake or Home → Read shows it, verified, before the reader is
rebuilt (#199, [089](089-resume-picture.md)). Measured at `8bdfe779` on
`newt-199-instant-resume` against main `f5dc28c6` (one build,
`regional-dev-usb`). Before is record 95's after.

| | Before | After |
| --- | ---: | ---: |
| Flash | 3,265,344 | 3,272,256 (+6,912) |
| Resident SRAM (`ram_bytes`) | 125,296 | 125,312 (+16) |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The panel mark is 8 B of `.bss` (the panel generation after the picture and
the shown frame's CRC), which fits existing padding. The open's pending
state is a new `PressState` value, `Deferred`, not a flag. The first build
grew `paintNonHomeScreen` from 96 to 112 B: a second instantiation of
`displayBinaryFrame` moved its panel lambda out of line. The picture now makes
its own panel call, and the frame is 96 B again. No selected stack ceiling
changes. Protected update 49 records the SRAM growth: the confirm round's
fix answers a held press before a lost or corrupt receipt makes the open
hash or import the book, through `beforeSlowSourceWork`, one 4 B function
pointer in `.bss`. With alignment it costs 16 B: `.dram0.bss` +8 B and the
heap start +8 B. It is used by every open; the picture has no off state.
New frames, not selected: `saveResumePicture` 880 B,
`ResumePictureSlot::load` 960 B, `header` and `write` 560 B each,
`resumePictureCandidate` 480 B, `showResumePicture` 464 B,
`traceResumePictureMiss` 432 B (DEV, only after a differing page),
`ResumePictureRecord::decode` 416 B and `readHeader` 128 B. The review's
three fixes add the show's gate (the ordinary open's rule, sharing the page
cache's file query) and the fast path's hint read:
`LibraryReaderSelection::sourceHintsMatch` 640 B and
`publishesBeforeVerification` 240 B. The gate and `stored()` share
`ChapterPageCache::presentIn` (48 B), into which `presentWithin`'s 16 B frame
is now inlined; `answerHeldPress` is 16 B. The static `ChapterPageCache::identity`
is 256 B; the instance one it replaces (224 B) is now inlined. Nothing is
held while reading; the save and the show borrow the logical frame.
Reviewer: GPT-6 Astra (xhigh) cross-family review and confirm round of PR #215; five P2 fixes confirmed by the Lead. Device checks are pending.

## Back paints Home first (#132, flash record 110)

Back and Menu → Home now write their optional chapter identity and estimate
records after Home is shown ([093](093-back-paints-home-first.md)). Flash
record 110 moves the ceiling to the measured image, measured clean at
`c56a94b1`:

| Measure | Before (record 109) | After |
|---|---|---|
| Flash | 3,301,264 | 3,304,032 (+2,768) |
| Resident SRAM (`ram_bytes`) | 125,312 | 125,312 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The selection's 4 B copy pointer fits existing `.bss` padding. Back
allocates the 392 B copy (`new(std::nothrow)`) for its save and frees it in
the same loop pass; a failed allocation writes the records inside the save as
before. Nothing is held while reading or idle. No selected stack ceiling
changes: the first build grew five selected frames by 16-32 B, so the copy's
`keepIdentity` is out of line, the estimate is copied once, the defer call
sits in `finishReaderExitAction` (keeping `finishReaderExit` at 0 B and
`dispatchAction` at 896 B) and `FilePort`'s constructor is marked `noinline`,
the compiler's earlier choice, which keeps the SampleReader ownerless identity
frames at 640 B. New frames, not selected: `publishDeferredExitMetadata` 720 B
(it runs from `loop()` after the paint, never inside a save), the copy's
`saveOwnerlessIdentityCheckpoint` 640 B, `saveChapterIdentityCheckpoint` 48 B,
`ChapterIdentityRecord::bindPeer` 96 B and `prepareCheckpoint` 48 B,
`ReaderExitMetadata::keepIdentity` 64 B, `sectionAt` 32 B and `copy` 16 B.

**Line exception.** `firmware/main.cpp` goes from 680 to exactly 684 lines:
the publication before each queued press (one line) and the publication after
Home's paint, which sits between the DEV report and the DEV idle gate, so it
splits that `#if` block (three lines). `firmware/library_reader.h` goes from
537 to exactly 546 lines: the copy's include, its owner and the two entry
points with their comment, and the estimate's one extra line. The logic lives
in `firmware/reader_exit_metadata.h` and `firmware/library_reader_save.h`; no
compressed formatting hides the seams. Thresholds, adapters and generated
exclusions are unchanged.

## Quick start (NEWT-83, flash record assigned at merge)

A new card shows a short quick start before Home, once; Settings' last row
runs it again ([095](095-quick-start.md)). Its flash record moves the ceiling
to the measured image, measured clean at `df91557c` (main `d415ba75` merged):

| Measure | Before (#269's NEWT-80 record) | After |
|---|---|---|
| Flash | 3,320,944 | 3,328,752 (+7,808) |
| Resident SRAM (`ram_bytes`) | 125,312 | 125,312 |
| Lent range (`wifi_lent_bytes`) | 22,528 | 22,528 |
| RTC | 300 | 300 |

The quick start's state sits in `AppState`'s padding, and update mode's
`QuickStartWifi` request rides in the SD resume record's existing RTC storage.
Nothing is held before, during or after a run, or while reading. No selected
stack ceiling changes: the first build's press route inside `dispatchAction`
grew its frame 896 -> 912 B, so the route sits in `loop()` instead
(`dispatchAction` 896 B, `loop` 192 B). New frames, not selected, all on a
quick start press, its boot decision or its paint: `wifiNetworkSaved` 160 B
(Wi-Fi and Done steps), the record's `DurableRecordStore` `save` 112 B, `load`
and `Slots::read` 80 B, `dispatchQuickStartAction` 96 B (the commit-then-publish
`pressQuickStart` is inlined into it), `beginQuickStart`, `resumeQuickStart`,
`commitQuickStart`, `quickStartFacts` and `paintQuickStart` 32 B each,
`quickStartPress` and `enterUpdateMode` 16 B each (`dispatchUpdateEntry` stays
16 B); the shared `renderStep` is 336 B.

**Line exception.** `firmware/main.cpp` goes from 684 to exactly 692 lines,
each a one-line seam: the store's and the shared screen's includes (two), the
paint's declaration and its branch in `paintNonHomeScreen` (two), the
runtime's include (one), setup's `beginQuickStart` (one) and the press route
in `loop()` with its comment (two; it sits there to keep `dispatchAction`'s
selected frame). The logic lives in `firmware/quick_start_runtime.h` and
`firmware/quick_start_store.h`; no compressed formatting hides the seams.
Thresholds, adapters and generated exclusions are unchanged.
Reviewer: Pending: Lead review of PR #272.
