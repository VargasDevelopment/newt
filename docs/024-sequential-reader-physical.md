# Sequential reader physical result

The sequential-read candidate was installed under its subsequent exact-image
grant. Installation, preservation and the two planned startup observations
passed. The unchanged 23-stage physical reading journey **failed after ten
validated stages** when sibling opening exhausted the existing 20-second
cooperative budget. This image is installed, but is not a known-good M1 release
and is not ready for the owner trial freeze.

`023-sequential-reader-candidate.md` records the earlier proposal, including its
then-absent authorization. This document records the later execution; it does
not retroactively change that proposal or authorize another hardware action.
The machine-readable, deidentified result is
`artifacts/m1-reader-sequential-journey.json`.

## Exact tested image and preservation

- Implementation: `82b5a9ca2b074af1bcd24408a887006a6f2bed18`
- Compiled source: `9489a63701d78e963da5d80abec3293c3bf596de`
- Reviewed installation HEAD: `6ac16ba5f6a407dc6c7d97c3e53ce7b2badc18cc`
- Build: `9489a63701d7-regional-experiment`
- Image: **1,252,752 bytes**
- SHA-256: `c1a5c214b57be03e2f86de680de58046cb41558c44db4d111673d4662b688cdf`
- Bound native builds: `artifacts/m1-reader-sequential-builds.json`

Fresh matching full-flash snapshots and durable restoration copies were retained
outside Git. The fresh NVS reading changes were reconciled against prior
evidence with exact record and byte checks, including negative controls. No
older progress was restored. A third complete pre-write read matched that fresh
baseline. One application write covered 306 sectors, `[0x780000, 0x8b2000)`,
including 624 bytes of FF padding. Full 16 MiB readback matched the expected
candidate, and every byte outside the application span matched the fresh
pre-write baseline, including core, partition data, NVS and filesystems.

Both planned 35-second startup observations reported the exact build, startup
guard, expected panel and trusted app, with stable ready Home status. The first
startup's low 64 KiB matched the fresh current baseline. An initial controller
path assertion stopped before serial construction; a separately reviewed
boot-only continuation corrected the approved package path and completed the
original two boots. Private provenance distinguishes that zero-hardware stop
from the actual boot observations. It was not another firmware write.

## Physical reading result

The ten passing stages were Home, Library, primary open, next, next again,
previous, exit, reopen, exit again and sibling Library. The eleventh observation,
`sibling_open`, expected Reader and received ready Library with the reader
closed. The command log's eleven recorded entries include that failed
observation: only ten stages passed.

Primary anchors were **1:578 → 1:963 → 1:1257 → 1:963**, followed by exact reopen
at **1:963**. These checks demonstrate page navigation and saved-anchor reopening
for the primary book in this journey; they do not demonstrate physical
sleep/wake resume or sibling readability.

| Sibling opening measurement | Previous candidate | Sequential candidate |
|---|---:|---:|
| Selection and full-source hashing | 14.177 s | 6.084 s |
| Reader phase starts | 15.484 s | 7.387 s |
| Remaining 20-second budget at reader start | 4.516 s | 12.613 s |
| Return with cancellation reason 2 | 20.088 s | 20.089 s |

Selection and hashing improved by **57.09%** in these observations. The reader
phase still did not complete: its observed span until cancellation was 12.702 s.
That span includes admission, metadata and page preparation; the current trace
does not isolate those subphases. The pre-cleanup SDK result was `IoError` (1).
The separately observed deadline cancellation explains the failed open, and
does not establish a media fault. Cleanup reported a closed reader and Library.
Neither the 20-second opening budget nor the 30-second command deadline changed.

Final activation was accepted in 10.344 ms, consumed in 11.847 ms, and returned
its terminal render in 21.431 s. All **247 CRC-valid replies** matched the audit
in order, and all **96 operations** had terminal responses. One startup banner
preceded HELLO; boot/session remained stable afterward, with no captured panic,
watchdog or reset. An independent audit confirmed the partial result.

There were **68 stack samples**, with **6,712 bytes minimum free** from the
16,384-byte owned task, including a sample after primary reopen rendered. This
is partial journey evidence, not a full journey or all-input stack bound.

## Closed state and next boundary

The connection closed after the failure, leaving an awake, ready Library screen
6, generation 73, with reader and opening closed and no dropped inputs. No
retry, subsequent reset, sleep input, card action or restoration was performed.
The latest reading progress is now authoritative; pre-journey backups cannot be
used as the current progress checkpoint. A future authorized installation needs
fresh preservation evidence again.

The remaining blocker is completing sibling opening inside the existing budget.
Source inspection identifies per-callback sleep cadence as a follow-up to
measure: the current external yield hook requests `delay(1)` at every bounded
source read, decoded chunk and layout step. This is a source-level hypothesis,
not a physical cause attribution or a verified replacement candidate. Any
correction must preserve full digest and whole-stream admission, bounded reads,
zero-output deflate scheduling, cancellation checks, parser reserves, the
startup/NVS guard and both deadlines. New source, clean resource builds and
reviews must precede an exact new hardware proposal.

An authored-only ASan/UBSan host probe counted the existing hook visits during
phased open, with no sleeping callback and no private books. Its measured counts
and authored source/fixture hashes are in
`artifacts/m1-reader-callback-analysis.json`:

| Authored container | Expanded padding added | Phased-open callbacks |
|---|---:|---:|
| 3,931 bytes | None | 63 |
| 8,131 bytes | One 4 MiB member | 2,118 |
| 66,941 bytes | Fifteen members, 60 MiB total | 30,890 |

All cases preserved the same five-page first chapter and source requests no
larger than 4 KiB. This demonstrates callback amplification from expanded
members; host elapsed time does not predict device latency. The current pinned
C3 configuration requests one FreeRTOS tick suspension per `delay(1)` call;
that request is not a guaranteed one millisecond of wall time.

The bounded next proposal is one shared UI-owned, wrap-safe timestamp gate:
retain every hook visit, request a sleep only after about 5 ms elapsed, and
record fresh time after the sleep. Cancellation stays independent of the
sleep gate. The proposal requires threshold, wrap and post-sleep fake-clock
tests, existing zero-output deflate/cancellation/corruption/frame/arena tests,
and four fresh native resource measurements. It does not supply a hard latency
bound for a blocking storage driver. Production source remains unchanged.

Sibling resume, remaining rejection/recovery/settings stages, optical/button
behavior, Library focus latency/ghosting, physical battery sleep/wake and owner
acceptance remain pending. Actual FAT timestamps and optional cover preparation
or transfer were outside this hardware grant. The 3–5-day trial freeze and apps
SDK milestone wait for M1 acceptance. Raw serial, device identities, private
book identities and full-flash copies remain ignored and private.
