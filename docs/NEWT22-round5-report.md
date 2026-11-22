# NEWT-22 round 5: storage notice ownership

The transaction coordinator returns independent progress and destination results:
`Unchanged`, `Verified`, or `Unconfirmed`. The notice consumes both before the
caller derives overall success. Verified progress updates the acknowledged
checkpoint even when the destination is unconfirmed. Only progress uncertainty
sets the Page bit and `PG SAVE ERR`; destination uncertainty uses `STORAGE`.

`ReadingSaveNotice` owns all shared storage diagnostics and the uncertainty
bits. Its projection alone writes the read-only `DeviceInfo` storage fields.
Detailed text, the center footer and the Home hint derive from this state with
fixed priority: page uncertainty, destination uncertainty, then diagnostics.
A visible cold Home route cannot assert verified Home intent: only a validated warm route or an explicit Home commit establishes the Home-specific copy. The three-byte, alignment-one owner fits existing recovery padding. Only page
and destination uncertainty cross warm wake in the existing recovery word;
boot storage diagnostics are recomputed and retained independently of RTC.

The existing selected compiler boundaries and signatures remain intact. The
canonical typed checkpoint composes their single-record operations with
`commitReading=false`, then independently verifies the destination. The
structural gate forbids production callers bypassing that coordinator. Legacy
selected `const char**` parameters remain inert to preserve measurement keys;
typed load status is carried in existing target padding. No adapters, exclusions,
selected measurements or protected ceilings change.

| Source | Retirement condition |
| --- | --- |
| Page uncertainty | Verified current progress record; destination success cannot clear it |
| Destination uncertainty | Verified requested intent commit or bounded exact readback; progress success cannot clear it |
| NVS version, recovery, unavailable | Successful NVS initialization, normally next boot |
| Home preference: unavailable warm/default load, invalid record, unconfirmed commit | Valid durable load or verified explicit preference commit; runtime choice remains unchanged until verification |
| Selected book unavailable | Valid selected-record load or authoritative empty selection when no committed reading intent requires a peer |
| Selected book unverified | Exact selected-record commit/readback verification |
| Missing SD card | Card verified present during source preparation |
| Changed/unavailable source | Successful validated source and reader open |
| Reader session/memory admission | Successful session binding/admission |
| Reader content/checkpoint: invalid locator/text, I/O, non-EPUB, encrypted, truncated, unsupported, parse, unavailable/changed source | Successful reader open |
| Resume authority unavailable | Loaded/empty authority read or verified requested destination commit; a skipped load cannot retire it |
| Progress unavailable | Loaded/empty progress read or verified progress commit |
| Reader preferences unavailable | Loaded/empty preference read or verified settings save |

App-local Library navigation, package and reader-panel messages retain their
existing scopes. They cannot assign the shared storage display fields.

The new matrix covers three progress outcomes by three destination outcomes by
foreign diagnostic present/absent, both with and without prior uncertainty, in
both geometries. It checks detailed text, Home flags/hint, footer classification,
exact full-size center glyphs and unchanged fraction/percent/battery. Verified
retries in both orders retire only their own state. Actual firmware admission,
checkpoint, exit and startup wiring exercises the two review scenarios and
subsequent recovery. Archived baseline witnesses reproduce six intended runtime
failures on clean `6cb251b`: three scenarios in both geometries. Existing warm
wake and cold-start regressions remain in the Linux gate.

All final software validation uses corrected source `be58565`; later commits
contain audit metadata, documentation and evidence. Quick, focused and complete
Linux gates pass. Fresh review 5 is CLEAN and independently rejects the unsupported review-4 claim. Physical USB sleep/wake,
unplugged battery sleep/wake, cold power-on and exact-source Mac ImageIO coverage
remain separate pending checks. This report claims no hardware behavior or owner
acceptance readiness.

Record 45 measures clean source `be58565` against an isolated clean `6cb251b`:

| Lane | Flash B | Growth B | SRAM B | RTC B | Selected entries |
| --- | ---: | ---: | ---: | ---: | ---: |
| Default | 2,166,432 | 2,512 | 133,232 | 300 | 350 |
| DEV | 2,306,288 | 2,160 | 134,080 | 304 | 379 |
| Regional | 2,173,136 | 2,496 | 133,728 | 300 | 352 |
| Regional DEV | 2,314,992 | 2,176 | 134,576 | 304 | 386 |

Every selected entry fits its existing ceiling. `setup()` is 384 B in every
lane; `openReader()` is 144/560/144/560 B; Frosted/Stage Home are 496/320 B.
RAM/RTC, protected history, policy pins, exclusions and selected measurements
are unchanged. The symbol explanation and raw comparisons accompany
[CODE_HEALTH.md](CODE_HEALTH.md#newt-22-storage-ownership-record-45-2026-10-20).


The only shared display writers are `src/reading_save_notice.cpp:77–78`, the
owner's projection. Reproducible grep:

```sh
rg -n '(\.|->)(storage_|pageSaveUnconfirmed_)\s*=' firmware include src tools --glob '*.cpp' --glob '*.h'
```

It finds only `info.storage_=message()` and
`info.pageSaveUnconfirmed_=pageUnconfirmed()`. The fields are private with
read-only getters and one owner friend. The structural gate additionally rejects
legacy direct writers, aliases, a second projection and raw checkpoint calls
outside the canonical typed boundary. All 27 code-health tests pass.

Quick, focused and complete Linux gates PASS on the Library correction in
353.183 / 40.066 / 867.826 s.
The full Linux gate includes ASan/UBSan, all 20 approved renderer frames, the new
ownership sequences and matrix, all existing warm/cold regressions, nine actual
startup/checkpoint mutants, five Stage restore/selector controls, sample
ownership and failed-chapter controls, descriptor starvation and cache producer/
historical retry controls. Supplemental sample-acknowledgement and retained-
notice mutants also fail their intended predicates. Intermediate failed fixture
migrations and the stopped initial Linux run are archived separately; they are
not passing evidence. The final complete run reports no failures.

Full every-page parity PASSes 18 fixtures and 13,408 comparisons in the
standalone 554.003 s run and again in the final Linux gate (562.346 s).
The fresh full turn matrix PASSes 24 heterogeneous fixtures, 3,456 observations,
1,152 exact cold/produced pairs and 1,152 cells. Every observation matches round
4 in every field except informational host time. The later startup and Library wiring
fixes are outside the matrix dependency closure; a fresh final-source relink gives
the identical tested executable, with its SHA recorded in
`artifacts/newt22-r5-matrix-final-source-proof.json`. These are deterministic
host work/pixel/anchor results, not physical latency.

All twelve native receipts bind clean `be58565`. Every selected frame fits;
the sole actual change from `6cb251b` is cold checkpoint restore, 112 to 96 B
against its unchanged 112 B ceiling. All sixteen live core artifact hashes and
all compiler-input hashes verify against record 45. Complete per-entry values
and hashes are in `artifacts/newt22-r5-resource-validation.json`.

The six authored normal/Page/other-storage frames are regenerated in
`/var/tmp/newt22-evidence/` and committed in `artifacts/newt22-r5-footer/`, with
PBM/PNG hashes. `PG SAVE ERR` is 103 px at full 16 px in both 140/200 px center
fields without ellipsis. `STORAGE` covers other diagnostics, with the plain
percent and fraction unchanged. Both inherited footer test sets still pass.

The pinned release gate passes full adoption
`fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d..HEAD` and `7cc92ab..HEAD`.
Committed candidate logs bind reviewed `572681a` and include each exact command,
range and resolved HEAD. Final handoff checks run both ranges once more after
the metadata commit; their logs and resolved final HEAD are retained in
`/var/tmp/newt22-evidence/newt22-r5-handoff.json` and adjacent release logs.
Physical USB sleep/wake, unplugged battery sleep/wake, cold power-on and
exact-source Mac ImageIO remain pending, with separate power/reset evidence
required. No push, flash, device access, dependency acquisition or history
rewrite was performed.


Fresh Claude Opus 5.5 high reviews inspect the complete round-5 range. Review 1
found one LOW historical documentation-number error, now restored; the whole
pre-round-5 CODE_HEALTH prefix matches the base exactly. Review 2 found one LOW
Library refusal copy regression: an older higher-priority shared notice could
masquerade as the selected candidate's refusal cause. The local refusal now uses
the canonical typed resume-authority cause, while shared owner priority remains
unchanged. The new production-body regression fails on the previous source and
passes both higher-priority sibling states (encrypted reader / Page) in both
geometries. A structural check rejects copying the shared projection into local
action messages. Raw reports and responses are archived as
`artifacts/newt22-r5-review-{1,2,3,4,5}.md` and corresponding response files. The first two reviews
approved the prior engineering growth. Review 3 explicitly approves the final
48/32/48/48 B increment and be58565 receipts; its sole LOW stale-log finding is
corrected by directly archiving fresh verifier output with assertions against
record 45. No runtime, ownership, type, RTC or resource defect was found. Fresh
review 5 is CLEAN. No runtime/source change follows clean `be58565`.
The implementation runtime is GPT-6.1 Sol high; Claude Opus 5.5 is a different
family (review 1 inferred the author model incorrectly from authorship metadata).


The complete final Linux rerun is archived in
`artifacts/newt22-r5-linux-refusal-final.log.gz`. The authoritative resource
reverification log is `artifacts/newt22-r5-resource-final-handoff.log.gz`, captured
directly from a fresh check of all sixteen be58565 core hashes. The older
`newt22-r5-resource-validation.log.gz` records the superseded 41bc669 check;
retaining it preserves the evidence history, without making it final evidence.


Review 4 raises no supported finding. Its sole claim that the verifier was not
archived is disproved: [the tracked verifier](../artifacts/newt22-r5-verify-resources.py.gz)
was committed in 1dc3ac5 and matches the executed script byte for byte. The
index now links it explicitly; `newt22-r5-review-4-archive-proof.json` records
the introduction commit and decompressed SHA-256. All three accepted findings
remain fixed. Review 5 independently confirms that this claim is unsupported and returns CLEAN.
Five fresh cross-family rounds leave zero open actionable findings: three LOW
findings were fixed, and one unsupported claim was rejected.
