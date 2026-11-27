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
Swift under `tools` and `tests`; tracked deletions are skipped. An authored
symlink fails, so moving source behind an alias cannot hide it from the line
count. The versioned policy sets a line guide: 500 for tools, 700 for tests.

File length is advisory. A file over its guide, and over its reviewed size in
`large_files` if it has one, prints a `WARN` line in the gate output (on GitHub
Actions, a warning annotation on the pull request). It never fails the gate.
Reviewers judge the growth. Line count is a review signal, not a request to
squeeze formatting or scatter helpers. Extract a cohesive owner only when it
makes the design simpler.

The Rust firmware's own structure is checked by `rust-health/rust_health.py`.
Its 500-line size check is advisory in the same way; `--update` only lowers a
recorded size. Its unsafe, `SAFETY:`, suppression, panic, crate-edge, external
package and native checks fail the gate.
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
   scoped 50,468 B whole-boot exception, which is never extended.
   No registered stack margin floor exists yet. The deleted C++ Newt used a
   16,384 B loop task; Rust uses the owner's 20,480 B choice of 2026-11-14.
   Rust's 5,212 B bench stop and native-inclusive model remain separate device
   acceptance checks; this image transition does not move a floor or task size.
2. **Above the floor is budget.** A feature or correctness fix may grow flash,
   RAM, RTC or a selected stack ceiling only with one measured record
   for the build: before/after, reason and reviewer. No silent creep: a
   flash, RAM or selected stack ceiling edit without its record fails the gate.
   RTC has no gate ceiling yet; its growth is reviewed, and the record's reason
   states RTC before and after. The reason also says what the growth buys: what
   the reader gains, or the reliability, security or maintainability problem it
   solves. A correct change can still be declined when its cost outweighs that.
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
  ([UPDATES.md](UPDATES.md#wi-fi-memory)). Reading does not pay for it, so `ram_bytes` is
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
