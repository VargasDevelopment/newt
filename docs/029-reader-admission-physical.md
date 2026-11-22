# M1 reader admission: actual diagnostic capture

The exact authorized diagnostic was installed once. The primary book opened,
turned pages and reopened at its saved anchor. The sibling still reached the
unchanged 20-second opening budget before metadata or layout. The single journey
closed normally after 10 validated stages and 11 observations of the required 23.
This is useful diagnostic evidence, not stable-reading or M1 acceptance.

## Exact checkpoint and preservation

- Reviewed source checkpoint: `1e9d1c5528ad6f79ad504aac42d193ee2da23032`.
- Compiled source: `4a31ec31750b8877a2fb555375fd2f1d6c3eb13b`.
- Build: `4a31ec31750b-regional-experiment`.
- Image: 1,257,744 bytes; SHA256
  `e7678f6c02ebbc137e881f3a219174ef0a0ebc2228e352ebe4424f5b2a3aaf1c`.
- One 308-sector app1 write: `[0x780000,0x8b4000)`, with 3,824 FF padding bytes.
- Full 16 MiB readback:
  `8c7ca9bacc8406a7db283273b955ddc7112df0e1464528e0c728131dc7f505a3`.

Two independent fresh full backups matched. A third live read matched immediately
before the write. Full readback equalled the fresh snapshot with only the exact
image and padding substituted; every outside byte, including NVS, was preserved.
Two bounded 35-second startups passed. The low 64 KiB remained identical after
startup. Neither old NVS nor old reading progress was restored.

The fresh NVS proof reconstructed every byte, with no unknown mutations or GC.
Generations 7/8 held anchor 1257. One exact forensic-model correction accounted for
the format's alternating blob chunk number, 0→128; 69 negative controls passed,
including a repaired-CRC wrong-chunk record with unchanged logical values.
Two independent action reviews and two final proof reviews were clean.

The sandbox initially denied OS port opening, before a descriptor, received bytes
or protocol commands. That zero-byte attempt was preserved privately. Required OS
access was then approved for the unchanged authorized helpers. No failed hardware
command was retried. No SD/card transfer, sleep or extra recovery action occurred.

## Complete causal profiles

Each primary, reopen and sibling profile contains all twelve distinct complete
rows exactly once, plus `emitted=4095 expected=4095`. All 247 wire responses had
valid CRCs and matched the receipt. There were no malformed, duplicate, truncated,
unassociated or fault rows. The analyzer passed 38 authored controls; two
independent reviewers reproduced its output and audited raw wire boundaries.

Times below are seconds. Inspection, catalog and stream intervals are nested
inside admission; hashing is before reader admission. Do not add nested columns.

| Operation | Whole-file hash | ZIP inspection | Catalog | Member streams | Admission | Metadata | Layout | Audited terminal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Primary open | 2.082 | 5.521 | 0.044 | 2.545 | 8.138 | 0.594 | 0.235 | 14.059 |
| Primary reopen | 0 | 5.538 | 0.045 | 2.551 | 8.163 | 0.589 | 0.235 | 10.903 |
| Sibling open, cancelled | 5.954 | 10.700 | 0.075 | 1.802, partial | 12.605 | 0 | 0 | 21.439 |

Sibling observations:

- Hash input: 2,360,898 bytes; 2,306 updates took 77.473 ms. Nested file calls took
  5.818 seconds, 97.7% of the hash interval. Sequential read throughput was about
  407 kB/s. Source requests SD mounting at 4 MHz; no clock experiment was run.
- Admission file calls took 12.143 seconds, 96.3% of admission. Reads took
  5.453 seconds; **431 seeks took 6.673 seconds across whole admission**. These
  are not separately instrumented inspection-only seek measurements.
- Inspection alone took 10.700 seconds, 84.9% of admission. Catalog construction
  took 75 ms and 276 duplicate comparisons took 441 microseconds.
- Member streaming started 24 members and completed 23. The current member,
  ordinal 23, stopped after producing 2,048 bytes of its declared 20,721 bytes.
  The cancellation makes this no evidence of a corrupt member.
- Nine image-named streams took 1.578 seconds. All streams took 1.802 seconds,
  including 1.548 seconds of nested file calls. The remaining 254 ms contains
  inflate, copies, callbacks and bookkeeping; it is not isolated inflater CPU.
- Admission's 556 scheduler sleeps took 117 ms. Sleep removal is not the main
  target. Metadata and layout never started; complete sibling-open cost is unknown.

ZIP admission validates structure, flags/methods, bounds, local/central names,
duplicates, stream errors and exact expanded sizes. The current implementation
**does not compute ZIP CRC32**. Do not describe these timings as CRC validation.
Full SHA identity remains unchanged. Optional cover-sidecar CRC is a separate path.

## Reading and limits

Primary anchors were 1257→1708→2127→1708, then exact reopen at 1708. The latest
observed primary position **1708 is authoritative**. Post-journey durable NVS was
not recaptured; historical snapshots must not replace current progress.

There were 68 observations of the 16,384-byte loop task, with minimum free 5,800
bytes, including actual primary reading/reopen. This observed high-water is not a
general stack bound or certification of sibling readability. The device was left
awake at Library error screen 6, reader closed and serial closed. No further
hardware work followed the failed journey.

## Concrete next source experiment

`src/epub_admission.cpp::inspect` reads central header→local header→central
name→local name for each member/name chunk. That repeatedly crosses the physical
file cursor. The fast SDK catalog construction is a different operation.

Test a bounded **4 KiB central-directory window** in the existing parse arena,
scoped to inspection. Allocate only after validating the EOCD and directory range;
fill only inside `[directory,end)`, capped at 4,096 bytes. Cache central headers and
names only; retain direct local-header/name reads and every existing check. Use
arena mark/release on every exit, before catalog and inflater allocations. Preserve
small-arena callers with an uncached path if the window does not fit.

Cache hits must preserve cancellation, handle and source-size guards and
cooperative scheduling. Newt's tested zero-length `FileBookSource::readAt` checks
those guards without seek/read; its use through the scheduled source needs generic,
short-read and fault-source controls. Avoid assuming unchanged physical read or
callback counts when the access algorithm changes.

Verify window crossings, maximum names, extra/comment skips, short/fault reads,
local/central mismatch, NUL/encryption/method/length/duplicate/corrupt-stream
rejection, cache-hit cancellation/stale size and early-return arena release.
Require unchanged authored page/anchor results and both orientations, the full
host gate, four clean native resource builds, bounded independent reviews, and a
new exact candidate proposal before any further hardware grant.

This is a concrete proposal, not an implemented fix or measured speedup. Keep
full SHA, all admission checks, both 48 KiB arenas, 32 KiB reserves, pinned SDK and
the 20/30-second deadlines. No skipped-image validation or clock change is needed
for this first experiment.

Full 23-stage reading/recovery and owner settings/sleep/wake/optical/battery/
orientation acceptance remain blocked. Library focus/ghosting and optional cover
FAT/card checks remain pending. The 3–5-day trial freeze and apps SDK milestone
have not started.

Sanitized numbers and proof hashes are in
`artifacts/m1-admission-physical.json`. Raw wire captures, backups, NVS records,
book/device identities and helper authorizations remain private and ignored.
