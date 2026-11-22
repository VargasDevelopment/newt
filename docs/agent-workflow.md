# Make something for your X3

Newt's intended loop is **describe an idea → an agent creates it → preview and test → install → share**. The X3 spike starts with one small, built-in app so its shape can be learned on a real device. There is no package format to learn yet.

Try an instruction like: “Make the welcome screen show a little reading nook, with a button to continue the offline sample and another to check the device.” The agent owns the Rust and build details. Previewing needs no device, account, network service, or Python package installation.

## Instructions for the agent

1. Read `AGENTS.md` and `rust/README.md`. Preserve the shared state/action boundary: the screen state changes on semantic actions, rendering draws the frame, and the firmware adapter handles hardware.
2. Edit the Rust screens under `rust/`. Use the platform-owned drawing primitives and semantic actions. Keep reading local; show unknown telemetry honestly. Do not introduce a runtime, manifest, credentials, or a packaging system for a trivial screen.
3. Update the meaningful state or rendering expectations in the Rust tests when behavior intentionally changes. Avoid tests that merely mirror draw calls.
4. Run the complete local gate from the project root:

   ```sh
   python3 tools/check.py
   ```

5. Inspect the lossless previews in `artifacts/`. They come from the **real firmware, compiled for the host** over C fakes of the board, card and radio, and its exact packed framebuffer. A host preview proves pixels and state transitions; it cannot establish panel initialization, physical buttons, battery accuracy, power consumption, or successful sleep/wake on hardware.
6. Run `python3 tools/firmware-build.py` with the approved installed toolchain, then return the change, preview, checks, and limits in plain language. It builds the actual C3 target and accepts no upload action. Installation follows the backup and recovery procedure with the owner present. Neither command flashes a device.

## Prove a working reading slice before polish

For the next reading feature, first make one complete path work: Library → open
→ turn → exit → reopen at the saved passage, including an honest failure and
recovery route. Review its real shared-renderer previews before expanding the
visual treatment. Keep the existing 20-state UI contract and deliberate pixel
approval; no command may approve a changed renderer on the owner's behalf.

1. Use focused checks during development, then run the complete host gate and
   bounded independent reviews on the combined candidate. Freeze the source and
   tool inputs before collecting final clean native resource receipts for every
   affected variant. Changes after freeze require fresh affected evidence; no
   automatic ceiling increases or acceptance-proof reuse are introduced here.
2. Select that candidate explicitly with `tools/reading-ready.py installable`.
   Retain its source/input identity, variant, build ID and exact image hash, plus
   a separately verified recoverable known-good artifact. A passing compile or
   `installable` state supplies no device-access or installation authorization.
3. At the next separately authorized owner hardware session, follow the existing
   backup/recovery and exact-image readback procedure, then run the existing
   `reading-ready.py journey` against that installed image before further polish.
   Its fixed 23 stages, one connection, raw DIAG binding and first-failure stop
   already cover this path and the rejected-book recovery; do not create a second
   shorter acceptance layer. Keep private books, identities and evidence private.
4. If the journey fails, pause feature expansion and fix the first reproducible
   failure. Keep the failed/partial evidence bound to its candidate. A replacement
   image needs its own readback and fresh journey. Passing this early reading
   check still leaves battery wake, optical/buttons and owner acceptance pending;
   only the existing `owner-accepted` state can claim ready to use.

If hardware access is not authorized, report the blocked physical check and the
exact software-verified candidate. Host success cannot close it. See
[installed reading admission](reading-acceptance.md) for the commands, evidence
schema, freshness bounds and acceptance limits.

## One owner for combined integration

Assign one integration owner before parallel runtime, UI and acceptance work.
Workers own nonoverlapping source; the integration owner combines reviewed
commits in an isolated checkout and preserves each worker's canonical gate
additions. Agree the existing executable boundaries before changing both sides:
DIAG/HELLO fields and record bounds, parse-storage lease lifetime, cancellation
and frame-publication outcomes, semantic actions and physical button routes.

Enforce those agreements with the production-wiring and fault tests already run
by `tools/check.py`: the Rust host tests, USB protocol tests,
`reading_acceptance_test.py` and `system_ui_contract_test.py`. Update the actual
producer and consumer checks together when an agreed boundary changes; a written
handoff or clean child review cannot substitute for the combined gate. Run fresh
bounded integration review on the exact combined diff before candidate freeze.

## Preview a particular interaction

```sh
python3 tools/preview.py --screen home
python3 tools/preview.py --screen epub --actions next,sleep,wake
python3 tools/preview.py --screen device --telemetry unknown
python3 tools/preview.py --screen device --telemetry sample
python3 tools/preview.py --screen sleeping
```

Each invocation writes `artifacts/preview.pbm` and `artifacts/preview.png`. PBM is the canonical 1-bit frame; PNG preserves those pixels exactly. `sample` telemetry is invented data solely for checking layout. Use `--output` to save a separate set. Valid button actions are `previous`, `next`, `activate`, `back`, `sleep`, and a simulated `wake`, in order and separated by commas.

The local gate verifies the official FreeInk submodule at `cd6f5b831c8fad899723b6d406586f00c575f021` and requires its tracked source to be clean, using local Git reads only. It builds the real firmware for the host, runs the Rust and host tests, exercises CLI rejection, checks lossless PNG encoding, and exports reproducible screens. Build outputs stay in `.build/`. It does **not** compile an ESP32 binary.

## What install and share mean today

Preview, tests and real firmware compilation work locally. The first installation is a supervised hardware bring-up, after a recoverable backup. App installation and sharing are product goals, not features claimed by this spike. The earlier Newt R&D remains available to inform that next step; the X3 is its constrained proving ground.

## Reader experiment boundary

Host fixtures never read the user's files. Change the embedded sample through
`tools/make-reader-fixture.py`; regenerated bytes also generate its content identity.

For development feedback, use `python3 -B tools/check.py --suite ui`. This
serial focused suite reports phase durations but does not satisfy the complete
gate. `--suite quick` is a parallel pre-gate: the complete gate's tooling and
policy tests, build boundaries, generator checks and preview contracts
(`QUICK` in `tools/gate_plan.py`), with their prerequisites, under the same
`-jN` runner. It never replaces the complete gate. The default
`python3 -B tools/check.py` runs every tier check.
`tools/host_common.py` reuses unchanged host objects only under the
complete compile key: actual preprocessed translation-unit bytes, effective
flags, transitive dependency names/bytes (including system headers), compiler
identities and explicit compiler environment, with verified object hashes.
Standalone gate host tests (`tools/host_compile.py`) use that cache; a
mutation changes its own unit's key, and every test is still linked and run.
Standalone fresh witnesses
(`compile_host` fresh directories, including trusted helpers whose receipts
state `fresh_per_invocation`) compile every object anew. Concurrent lookups of
one unit, in any process, compile it once. These caches reuse compiler
work, not test results, native receipts or physical evidence; they install
nothing. Keep test outputs isolated rather than parallelizing shared paths.

The complete gate runs independent phases concurrently. `-jN` bounds its
concurrent phases and, through gate-wide slots, the compilers, linkers and
compiled fixture runs started through the shared host helpers
(`tools/host_jobs.py`); a few suites still start their own compilers directly,
outside the slots. The default is the usable CPUs (affinity and cgroup
quota; see `docs/CI.md`); two gates at once share them through the host slots. `-j1` runs the
serial order exactly and streams output. With
more jobs, each phase's output is printed whole, labelled, in completion
order. The first failure starts no new phase; running phases finish and report
first. `--keep-going` runs every phase whose prerequisites passed and reports
all failures. The release preamble and the other focused suites stay serial.
`tools/gate_plan.py` lists phases in serial order and declares each real
ordering: `after` when a phase reads another's output, `uses` for a fixed path
two phases share, and `alone` for a phase that must overlap no other
(wall-clock deadlines or whole-tree inventories). A new phase needs its
declarations; prefer isolated outputs.

Use `--fresh-objects` to compare ordinary cache reuse with recompilation. The
compiler runs under an explicit environment; ambient `CPATH`, `SDKROOT` and
compiler overrides or default driver config cannot select different cached inputs.
The cache admits only the approved compile flags and include/macro arguments;
profile, PCH, plugin and response-file roles need a reviewed input-boundary extension.
Linking and every
selected test still run. Corrupt/mismatched object receipts force recompilation;
output aliases fail closed. This local build cache does not authenticate an
operator who deliberately fabricates both an object and its matching receipt.

Software test-proof reuse remains disabled. The optional reading-ready
`--software-proof-shadow` records matching observed inputs while running the
full gate again; see [its explicit closure limits](software-proof-shadow.md).

Use [Decision 002](002-bounded-offline-reader.md) before changing reader budgets or introducing resources. A compile command produces the current branch's image, so always select the branch/source and receipt deliberately.
