# M1 directory window: complete physical reading journey

The exact directory-window candidate passed the unchanged **23-stage physical
reading/resume/recovery journey**. The sibling, which previously failed to open,
now opens, advances, returns and reopens at its saved page. Its ZIP inspection
fell from **10.700336 s to 2.260523 s**, a **78.8743%** observed reduction.
This is one complete fixed run; owner and broader M1 acceptance remain pending.

## Candidate and installation

Build `57ba55228181-regional-experiment` is **1,258,736 bytes**, SHA256
`e39bcbd491853aedee03bc2d6c77f2e15904cb5d01b97d9badc61585d437111d`.
Compiled source remains `57ba552281813d6979b71c9714b6a715537b800d`;
the live action was reviewed at `2b683f75cca1436dfc43b5e7ab943c4c04057e1d`.
Compiled, tool and test scopes remain unchanged from the freeze. Full host and
all four clean native gates passed; final resource receipts remain current.

The first attempt stopped before write at its security-info SYNC, as recorded
in `031-directory-window-install-stop.md`. The parent then authorized one
supported transport recovery and a newly reviewed attempt for this same image.
Read-only USB inventory matched the original target. One supported USB reset
established a verified ROM SYNC and security flags zero, without stub upload,
application boot or flash write. The earlier failure's underlying cause remains
unestablished. Its failed attempt, latch and backups are preserved unchanged.

Two new independent 16 MiB captures after recovery matched the preserved
1708 snapshot exactly. Two independent 308-sector recovery slices are durable.
The unchanged strict forensic guard reconstructed every NVS byte with zero
unknown mutations; all 69 negative controls passed. Two independent action
reviews passed before the new authorization was frozen.

The renewed attempt required a third identical full read before its single
write to **308 sectors**, `[0x780000,0x8b4000)`, with **2,832 FF padding bytes**.
Tool hash verification and full 16 MiB readback passed. Every byte outside the
approved span, including current NVS and coredump, was unchanged before boot.
The expected full readback SHA256 is
`176f5fd28ea7b41780d887fa914aa3573502a487c8d500d6d81a8d4061d868de`.
Both planned 35-second boots passed the NVS guard, expected panel and stable
Home checks. Low 64 KiB after the first startup matched the fresh snapshot.

## Actual reading and recovery

The fixed journey completed in **376.316688 s** with **23 validated stages**,
**245 operations** and **646 ordered CRC-valid responses**. No hardware retry,
fault marker or in-session reset occurred. The connection closed normally.

Primary anchors were spine 1, characters **1708 → 2127 → 2607 → 2127**; normal
reopen and post-rejection recovery both returned to **2127**, then advanced to
**2607**. Sibling anchors were spine 1, characters **0 → 137**; its recovery
reopen returned to **137**, then advanced to **710**. The planned rejected entry
reported `NotFound` (9), with reader closed and opening false; it did not prevent
subsequent valid reading. This verifies the planned failed-open recovery, not a
general corruption-detection guarantee.

The final state is awake normal Home, screen 0, reader closed and opening false,
with serial closed. **Primary 2607 and sibling 710 are the latest observed
positions and remain authoritative.** Post-current-journey durable NVS has not
been recaptured. The pre-write 1708 snapshot is historical and must not replace
the newer reading state.

There are **186** actual stack observations on the **16,384-byte** reader task,
with minimum free **5,752 bytes**. A sample is bound after the final reader paint
and before its exit. This measures the complete fixed journey; it does not
establish a general all-input stack bound.

## Complete diagnostic profiles

All six profiles contain the complete 12-row mask, `4095`, associated with the
audited activation's raw wire interval. Independent analyzer reproduction and
all **38** authored analysis controls passed. There are zero association,
numeric-consistency, CRC and fault findings.

| Stage | Inspection s | Hash s | Admission s | Metadata s | Layout s | Terminal s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Primary open | 1.117878 | 2.084339 | 3.740 | 0.593 | 0.236 | 9.652916 |
| Primary reopen | 1.130869 | 0 | 3.754 | 0.591 | 0.242 | 6.505169 |
| Sibling open | 2.260523 | 5.987161 | 9.463 | 0.657 | 0.265 | 19.255118 |
| Rejected open | 2.735647 | 5.243906 | 9.350 | 0.574 | 0.050 | 18.125897 |
| Primary recovery | 1.119715 | 2.081953 | 3.739 | 0.591 | 0.243 | 9.667327 |
| Sibling recovery | 2.260365 | 5.987311 | 9.463 | 0.657 | 0.300 | 19.362358 |

The existing **20-second cooperative opening budget** and **30-second wire
deadline** were unchanged. Current sibling terminals leave limited timing
margin; one run is not a worst-case latency guarantee or a hard SDK-call bound.

| Stage | Whole-admission reads | Whole-admission seeks | Read bytes | Members completed/started |
| --- | ---: | ---: | ---: | ---: |
| Primary open/reopen/recovery | 620 | 128 | 835,053 | 59/59 |
| Sibling open/recovery | 1,515 | 211 | 2,381,145 | 100/100 |
| Rejected open | 1,591 | 340 | 2,090,698 | 165/165 |

The previous sibling had 431 seeks and 755 reads across **partial admission**,
with only 23/24 started members completed. Those counts are not isolated ZIP
inspection counts or a same-complete-work comparison. The current successful
sibling validates all 100 members, including nine image-named streams; their
complete stream time is about 7.051 s. Previous partial stream time cannot
establish a full-stream speedup. Hash, stream and nested file intervals are
inclusive; no exact inflater CPU subtraction is supported. Current admission
does not compute ZIP CRC32. Full source SHA identity and validation paths remain
unchanged.

`artifacts/m1-directory-window-physical.json` contains numeric profiles, opening
phase traces, counts and evidence hashes. Two independent offline actual-evidence
audits verified preservation, recovery, source/native/runtime bindings, all wire
responses, reading/resume, stack observations and analyzer results. Raw serial,
flash captures, recovery slices and private book identities remain outside Git.

## Remaining M1 gate

Matched owner optical/focus/ghosting, physical buttons, both orientations,
settings, sleep/wake, battery and broader book acceptance remain pending on this
exact build. Actual cover preparation/transfer and FAT timestamps require their
parent-coordinated card scope. The proposed **3–5 day trial freeze has not
started**; apps SDK build/install/try remains the second milestone. No additional
hardware, sleep, card or restore action followed the completed journey.
