# Whole-open scheduling profile and bounded reader cadence

The installed sequential-read image reduced physical selection/hash time from
14.177 to 6.084 seconds, but sibling opening still exhausted the 20-second
budget. Its physical journey remains FAILED after ten of 23 stages. The current
source correction is not installed, and there is no X3 efficiency baseline or
successful sibling-reading claim. See `024-sequential-reader-physical.md`.

## Causal fixed-work host evidence

Frozen authored EPUBs and pinned SDK source compare the previous per-callback
one-millisecond suspension with the exact new clock cadence. Both policies
perform the same full streamed source SHA, admission, metadata and complete
first-chapter reflow. Read counts, hook visits, SHA, exact frame digest, five
pages and 2,261 characters match. No private books or hardware were used.

| Authored expanded padding | Previous whole open, two samples | Cadence whole open, three samples |
|---|---:|---:|
| None | 130.8–135.7 ms | 3.9–5.1 ms |
| One 4 MiB member | 3,203–3,214 ms | 14.4–16.2 ms |
| Fifteen members, 60 MiB total | 46,223–46,266 ms | 124.9–129.3 ms |

Measured suspension accounts for 99.3% of the large control's whole-open time.
Its 30,913 callback sleeps become 21–22, while the separate source-hash wait is
unchanged. The cadence's directly observed large admission phase has 30,876
callbacks and 148 source reads; initial metadata has 14 callbacks/reads; layout
metadata has another 14, and chapter layout has nine callbacks/two reads.
Admission takes 121.9–124.5 ms, initial metadata 0.230–0.233 ms, and layout
1.054–2.731 ms on this host. These measurements identify expanded-stream callback
sleep as the highest causal cost in the fixed authored control.

Standalone phase diagnostics are never added to the whole-open totals. Phase
sleep/callback/read sums equal their parents, and nonnested phase time does not
exceed the parent interval. Source and fixture snapshots are bound before and
after compilation/run. An earlier live-source model is supplementary only and
is excluded from these accepted results.

`artifacts/m1-reader-yield-host-profile.json` records the distribution, equality
checks and hashes. The ignored authored snapshots, results and driver are in
`.build/yield-cadence-profile/`; reproduce locally with
`python3 -I -B .build/yield-cadence-profile/stable-profile.py`. Host warm-file,
ASan/UBSan and operating-system sleep timings do not predict C3 latency. The
fixed-work probe allows both arms to finish and does not attach the target
20-second cancellation hook; production preserves that deadline.

## Source invariant and service fairness

One UI-owned `ReaderYieldCadence` shares its timestamp across every external
reader hook. Every existing bounded source-read, decoded 2 KiB chunk and layout
step callback still runs. `ReaderScheduler` checks opening cancellation/deadline
at every visit, independently of the sleep gate. A callback requests the same
`delay(1)` only after at least five elapsed milliseconds since the last wait
completed; it records fresh time after waking. Unsigned elapsed arithmetic
handles clock wrap, and a delayed wake cannot produce catch-up sleeps.

The pinned C3 configuration requests one FreeRTOS tick for `delay(1)`. The
higher-priority button task remains independently preemptive; periodic waits
continue to give lower-priority scheduler/idle work a turn. The clock gate also
covers zero-output deflate source hooks. Its cooperative service interval is
five milliseconds plus the current bounded work/storage segment; this is not
a hard bound on a blocking storage driver. No yield site, integrity/admission
check, read cancellation check, parser reserve, catalog lifetime, SDK pin,
startup/NVS guard, 20-second opening budget or 30-second terminal deadline is
removed or extended. Whole-stream admission validates expanded extent; this
change does not introduce or claim a new ZIP member CRC verifier.

## Target evidence and resource boundary

Optional trusted read-only phase scopes expose admission, metadata and layout,
returning to Idle on every success or error. The DEV report has one bounded
line: `Reader work: a=ms,calls,sleeps,wait m=... l=...`. Phase elapsed includes
wait time; do not add wait again. Metadata observations include repeated selects
during page restore. The existing outer trace still separates painting,
selection/full digest, progress loading and reader work; unclassified reader
overhead is the outer reader span minus phase totals.

DEV counters belong to the existing reader-open stack scope. A four-byte borrowed
profile pointer is cleared/restored on all exits; callbacks outside that scope
retain no diagnostics. Callbacks never log or allocate, and counters retain no
book data, raster or filename. The final report uses the existing `Print::printf`,
which can transiently allocate up to 152 bytes for a line exceeding its 64-byte
local buffer; reporting happens after parser teardown. One report requires
160 free TX bytes for its maximum
151-byte line; unavailable capacity skips the observation. A future hardware
audit must verify captured reports rather than assume they were emitted.

The named DEV persistent RAM assertion explicitly includes the borrowed pointer
and retains its strict 1 KiB limit. Native RAM, flash and selected-frame costs
require fresh measurements and independent review before candidate freeze.
The exact main-file allowance changes from 615 to 619 lines after independent
structural review: the four additions connect the service include, UI-owned
instance, scoped profile lifetime and observer registration at existing owner
boundaries. Behavior lives in the cohesive service helpers. Policy, exclusions
and all resource ceilings remain unchanged at this source implementation stage.

Focused ASan/UBSan checks pass threshold, wrap, delayed wake, profile detachment,
phase accounting and actual compressed-reader Sleep/deadline cancellation in
admission, metadata and layout. Both orientations preserve the immutable exact
checkpoint, settings, published frame and clean retry. Existing expansion,
corrupt-deflate, empty-spine, XML ceiling and parser ownership checks pass.
The complete host gate subsequently passed, including the existing admission,
zero-output deflate, parser allocation, publication, progress, wake and recovery
controls. Its log SHA-256 is
`8c4fe74fbbd4baf47e5d7fcba695af15315eb9f0ece70f8640ea8f28f463a20d`.
Native acceptance and exact candidate provenance remain separate required steps
before any new coordinated physical test.

## Measured native cost review

Four clean preliminary builds of implementation
`1ad90ad5cb05b65f6ff99cccd25040808cf15163` compiled successfully and passed
linked startup/NVS, sleep, retained RTC and owned 16 KiB reader-task guards.
Their old resource ratchet rejected the following exact feature costs:

| Variant | Shared SRAM bytes | Image bytes | Image growth | RTC bytes |
|---|---:|---:|---:|---:|
| Default | 133264 | 1237888 | 288 | 300 |
| DEV | 134080 | 1249696 | 800 | 300 |
| Regional | 133776 | 1241696 | 272 | 300 |
| Regional DEV | 134592 | 1253536 | 784 | 300 |

Phase observation scopes grow two selected static frames by 16 bytes each:
`PhasedReader::open` 848 to 864, and `select` 128 to 144. The other 22 selected
frames are unchanged, including layout, source hashing and dispatch. The scope
captures its trusted observer once and closes it on every exit; replacing that
ownership with repeated manual completion calls would make failure accounting
less reliable. Exact measured ceilings cover this functional cost with no
additional headroom or removed measurements.

Raw compiler reports also show the scheduler callback frame growing from zero
to 16 bytes, a new 16-byte work-scope destructor and zero-byte observer setter.
DEV additionally has a 16-byte phase callback, a 48-byte profile destructor and
`openReader` growing from 144 to 224 bytes. Its 68-byte C3 profile scope stays on
that existing stack rather than exceeding the strict persistent diagnostics
limit. These are individual frames, not a complete call-chain or physical peak
bound. Previous hardware high-water evidence belongs to the installed image;
the new image still requires an observation after actual reader paint.

Linked SRAM occupancy and RTC totals remain unchanged because object growth
fits existing packing/alignment. This does not mean the service needs no storage:
the global reader adds a four-byte hook, and the scheduler holds a four-byte
clock plus a four-byte borrowed profile pointer in DEV. Independent native
disassembly against the previous frozen ELF confirms external owner allocation
sizes are unchanged: 61055 bytes with borrowed parsing, 99401 with separate
parsing, largest allocation 49171. The PhasedReader hook fits native object
padding. Host allocation sizes are separate measurements.

The application slot remains 7798784 bytes; remaining capacity is 6560896,
6549088, 6557088 and 6545248 bytes respectively. Two independent fresh reviews
verified all 334 archived outputs per variant and judged the exact flash/frame
costs proportionate. Adoption changes only those measured costs and provenance;
SRAM, policy, adapters, exclusions and selected measurements remain intact.
Preliminary receipts use the project-local Python 3.12.6 wrapper. Fresh final
builds must use the canonical host Python 3.14.6 wrapper and retain the approved
project-local 3.12.6 PlatformIO interpreter before a candidate is frozen.
