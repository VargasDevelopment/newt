# Preserve the acknowledged reading checkpoint

This source checkpoint completes the periodic wake-intent repair on the
installed `f22f72044de2` / evidence `c505be3` runtime, retaining the earlier
failed-chapter sleep fix. It changes no wire format, backend, book bytes, source
admission or reset eligibility. No records are migrated, erased or restored.

## Commit authority and interrupted updates

Existing `newt_resume` NWBR is the final destination authority. A Reading intent
references an acknowledged progress CRC and settings. Newer selected/progress
peers can be provisional; their generation alone cannot replace its checkpoint.

| Authority | Update and cold-boot rule |
| --- | --- |
| Reading A | Protect A's matching selected identity and progress peer. Stage a new checkpoint in the other progress peer, then commit/read back NWBR last. |
| Reading A, switching to B | Protect the selected A peer while B's full-SHA identity and progress stage. Until NWBR commits B, power-on resolves A. Repeated failed B attempts cannot overwrite A. |
| Home | Preserve Home during staging. Only a successfully published explicit Reading opening can commit Reading. Home never triggers periodic Reading saves. |
| Missing intent | A verified explicit first page immediately saves initial progress and Reading intent, without waiting for Sleep or the thirty-second interval. |
| Unknown/ambiguous intent | Refuse automatic Reading and all transactional staging. A possibly newer torn Home record cannot authorize fallback to older Reading. |

Boot resolves both selected peers for NWBR's book and both full-SHA-authorized
progress peers for its CRC. These reads do not repair storage. Source reopening
still hashes the complete current EPUB and restores the acknowledged page.
Mount, source or parser retry preserves the same owned checkpoint and remains
read-only. POWERON is the sole durable automatic route; RTC and other reset
eligibility remain unchanged.

Transactional progress saves validate every peer's known schema/extent, even
with no protected CRC or a Home/different-book intent. Known torn v1 writes can
retain their valid peer; unknown versions/extents/reserved fields cannot be
overwritten as decoration or fresh storage. Full digest checks still authorize
per-book namespaces. Different full digests sharing a short book ID fail closed.
CRC zero is an ordinary present reference. Two distinct valid equal-CRC peers
are ambiguous, not a missing legacy checkpoint.

## Explicit opening, settings and exits

An actual successful explicit page publication acknowledges the complete
selected/progress/intent combination immediately. A failed final commit reports
unsaved state while retaining the old acknowledged checkpoint. Repeated saves
continue targeting its unprotected peer until a verified new NWBR can commit.

A legacy installed intent can already reference progress overwritten by two
uncoupled periodic saves. Boot cannot invent that missing page. If the peers are
otherwise known, valid and unambiguous, an explicit opening revalidates the full
source and actual page before establishing a new coherent checkpoint. Unknown,
unreadable or ambiguous peers cannot use this adoption path. Boot/pending retry
never adopts an orphaned intent.

Pre-settings and reader-exit progress saves stage without committing Reading.
New geometry becomes authoritative only after its actual containing page has
successfully published. Intentional Back/Home commits the Home tombstone before
closing the reader; failure rolls back the exit. An unacknowledged Reading sleep
now returns failure before any intent write. Abandoning sleep cannot turn the
previous Reading checkpoint into an unintended Home destination.

Equal full digests under different filenames identify the same book content;
the newest canonical pathname can be selected before the final intent commits.
Exact previous-pathname rollback for same-content aliases is not guaranteed.
Source digest, page and settings authorization remain mandatory; missing or
changed paths retain the checkpoint and require retry.

## Verification and scope

`tests/periodic_checkpoint_recovery_test.py` runs the actual save, periodic,
successful-publication, explicit selection acknowledgement and cold-setup
branches with authored EPUBs and ASan/UBSan. It covers genuinely empty-store
first opening before Sleep, both orientations, exact frames/statistics, repeated
rejected final commits, partial writes/lost acknowledgements, settings, Home,
selected-book staging, mount/hash/parser retry, strict schemas, known legacy
orphan adoption, CRC zero and deterministic canonical CRC collisions.

Four isolated mutations compile and fail their specific runtime assertions:
omitting the periodic Reading commit, discarding the protected CRC, treating
CRC zero as missing, and bypassing strict progress validation. Independent
adversarial controls also reject nonstrict saves and ambiguous-CRC adoption.
Existing failed-chapter recovery retains its 28 exact deep/cold cases and
compiled old-condition control. Existing cold and selection coverage remains
active; the reusable in-memory selection backend moved to its own small header.

Two fresh independent source reviews are CLEAN. They approve only the exact
`firmware/main.cpp` structural allowance from 631 to 647 lines: transaction
integration belongs at existing save/open/settings/exit/setup owners, with
decisions contained in `reader_checkpoint.h`. The extracted selection fixture
is below the generic 700-line limit and retires its prior 710-line exception.
These source reviews authorize no native cost exception. Generic policy,
adapters and exclusions remain unchanged; native costs have their own measured
independent review in `CODE_HEALTH.md`.

The complete `python3 tools/check.py` gate exits 0 at `086044f`, including
ASan/UBSan, both orientations, compiled fault controls and exact PBM/PNG
previews. Publication fixtures call the actual save helper or a counted endpoint
that rejects automatic boot saves; cancellation and no-write assertions remain
active.

The full host run creates additional project-local procedure bytecode caches.
Post-host identity validation detected that drift in the earlier `9540109`
receipts. Their archives and candidate are preserved as historical evidence.
Four fresh clean native builds after the caches settle at
`086044fc05a873b86c3d4ec6b0daeb951521ff9e` all exit 0. Both refreshed final
independent audits are CLEAN: all 2,672 canonical/archive output hashes match,
current compiler/procedure/runtime identities match, and actual ELF/map/image/
compiler-frame costs and linked NVS/sleep/RTC/strong 16 KiB task proofs are
rederived. All original 34/36 frames remain, with eight new exact checkpoint
guards giving 42/44. Static RAM and RTC are unchanged.

The frozen regional DEV image is 1,263,328 bytes, build ID
`086044fc05a8-regional-experiment`, SHA-256
`d53a552fe11f9c111fd534e64d6029063eb131311745b57a1f74d6fb1f9de955`.
Full receipts are in `artifacts/m1-battery-transaction-builds.json`. This is a
candidate on the installed-runtime baseline. Parent must integrate these fixes
with its newer opening-feedback checkout and obtain fresh combined build
evidence before a proposed flash; this checkpoint cannot silently replace it.

## Actual device evidence remains incomplete

The last bounded returned-device observation identified installed
`f22f72044de2-regional-experiment`, idle Home and USB reset. It did not read
durable selected/progress/intent records or establish the earlier battery reset.
The running firmware has no NVS read API. A new private wrapper is scoped only
to `[0x9000,0xe000)` NVS plus target/security metadata, but requires a separately
authorized ROM-entry transition and reviewed return to the app. The capture and
separate return helpers have two CLEAN action reviews and 16 passing offline
controls. Parent subsequently authorized that exact read/reset envelope. Target
identity matched on ROM entry, but the next flash-metadata command stopped in
its first SYNC with invalid packet head `0x72`. No NVS bytes were read, and the
fail-fast wrapper performed no retry/fallback or automatic return reset. This
is transport preflight failure, not evidence of NVS corruption. Current durable
records remain unknown. Old full-flash capture scripts and historical backups
must not be substituted. The separately approved normal return reset subsequently
executed once and closed its port. Its failed-preflight admission has two CLEAN
reviews and five passing offline controls, reusing the frozen normal-reset body.
App boot and battery recovery are not verified. No flash installation, erase,
restore, card action or diagnostic retry occurred.

No actual local copy of the owner's You Love Me EPUB is available, and no
reader card is mounted. Its first-spine error cannot be diagnosed from the
generic failure photograph or authored fault matrix. Record capture, actual
chapter cause, owner battery sleep/distinct cold-start and physical focus
latency/ghosting remain release gates. Parent owns a separate isolated source
worker for Home/Library latency. Automatic covers and future chapter-cache
wiring stay paused. This source work does not authorize a flash or trial freeze.
