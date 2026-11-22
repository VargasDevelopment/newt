# Block-checked page cache (NEWT-34)

Integration, 2026-10-23: this is document 072; device Settings owns 071.
The normal two-parent merge `e37c1ba` incorporates main `2f78920`.
NEWT-34 is now flash record 55 and protected update 34, following main's
record 54 and protected updates 32–33. Earlier standalone provisional numbers
in archived reports are historical. Main's complete history and floors remain
unchanged; the standalone record is preserved separately.

The Lead's USB run on `8bf8744`, with matched steps and warm-up, confirms the
pathname-work saving. These are device results on that earlier head, separate
from the merged head's host and compile evidence:

| Position | Cached-open total | Cache slot v[6] | Baseline 3154a93 v[6] |
| --- | ---: | ---: | ---: |
| 0 | 1.90–1.99 s | 371–409 ms | 661–805 ms |
| 1 | 1.59–1.63 s | 340–348 ms | 365–438 ms |
| 3 | 1.59–1.61 s | 322–326 ms | 587 ms |

CQ probes are about 232–265 ms, zero, zero. Retained turns are within about
30 ms of baseline; the last position-1 turn has about 105 ms in v[6] versus
about 80 ms before. Other power states and runtime memory margins remain
separate checks.

The page route advances from engine revision 3 to 4. Old page names are ordinary
misses. This route accepts only Newt's NTBE version 1 envelope: unchanged FIBP
payload, one little-endian CRC per 4 KiB block, and a 64-byte manifest. The
manifest binds the complete route, sizes, directory CRC and payload CRC, and
has its own CRC. Each block CRC includes the full route, block number and true
length. This detects damaged media; it does not authenticate hostile writes.
Metadata, identity and progress records keep their fully checked NTCC format.
There is no old-format parser on the new page route. A damaged canonical file
cannot use its backup; only canonical absence permits the existing `.bak` rule.

A native query explicitly lends `PageReadQuery` through the existing authority
entry callback. Its raw capability is restricted to the expected name, exact
FilePort generation, physical bounds and immutable scope. A failed source guard after successful authority entry revokes the query before
the verifier or SDK can read through it. The SDK reads only
through the temporary `VerifiedPageStore`: it retains the checked directory,
reads the adjacent CRC directory and manifest together, loads complete blocks
into checked RAM, and serves virtual FIBP offsets from
that window. Every new query checks its own consumed bytes, including under a
retained descriptor. Presence and extent remain hints. Direct native page
`readAt` refuses access. Source entry/exit validation and authority finish still
precede page capture, navigation or pixel publication. NEWT-31's pending-source
gate and cache-only refusal are unchanged.

The non-DEV query uses 8,352 bytes on the host and 8,344 bytes with the installed C3
compiler (`newt34_query_bytes`, size 0x2098). Compile-time assertions cap it at
8,448 bytes and keep it beyond the 8 KiB SDK index inside the existing 48 KiB
parse loan. Query destruction precedes returning that loan. No descriptor,
query pointer, directory or block buffer is retained by the native store.
The old resident GuardedStore read-ahead bookkeeping is removed. No scheduler,
checkpoint, payload/page limit, 128 KiB opening cap or reserve changes.

Finalization reads the payload back through the existing transient 4 KiB
allocation, builds directory entries from those SD bytes and checks the rolling
write CRC. It appends the manifest, syncs, and reads the complete resulting
candidate again before the existing old-or-new promotion. Private transfers and
CRC work preserve 1 KiB cancellation boundaries. Candidate quota/write credit
includes the maximum 4,160-byte envelope overhead without raising payload caps.
Both producer admission paths, including the retained-descriptor probe, use the
same complete verifier before declaring a candidate complete. A confirmed
checksum failure is an optional miss that permits repair; I/O/cancellation, including a transient route-level refusal that the probe
callback does not observe, remains a refusal. Retained producer admission uses
only the immutable scope, with 1 KiB CRC cancellation polls and no foreground
128-attempt/3-second budget. Intact 63/64/147-block candidates remain Complete;
last-block damage permits repair. The shared codec owns CRC/wire primitives and
incremental block seed/update/finish for both reader and producer. This avoids turning a presence hint into repair authority.

## Complete native work counts

`072-cache-envelope-counts.csv` contains 572 matched actions across the original `e3913e2` baseline, a fresh
`b9cdb32` review-baseline replay, and fixed source `1d890e9`, across
11 authored fixtures: stored/deflated plain text, styling, lists/tables,
anchors, dense styled runs, small files and a 710-page/5,680-byte page index.
Both orientations exercise first/middle/last anchors, both directions,
chapter crossings, cached open, displaced, retained and post-save turns.
The harness uses actual native sessions, UI turns, builder, SD storage and
pinned SDK. Each sampled publication matches ordinary pixels and locator.
Physical page-file counters include manifest/directory reads and presence
probes, rather than replaying a model. Preparation and ordinary-oracle work are
outside each counted action. Complete-source/storage budgets remain checked
separately by the existing I/O suite. These are host work counts, not SD timing.

Examples below use landscape, the first anchor and Next. Values are page-file
bytes / physical reads. Every source/settings match and hit/miss remains in the
CSV. Both revisions use the same short SD root, so the unchanged 256-byte
path bound permits semantic metadata temporary files in both runs. An earlier
replay used a longer root for `b9cdb32`: metadata publication then exceeded that
bound and its repeated misses displaced the page descriptor in 87 retained
cells. Equal-path replays remove that harness effect. Same-chapter retained
hits assert zero page opens; the combined read saves one physical read without
changing bytes or opens.

| Payload | Action | Original e3913e2 | Review b9cdb32 | Fixed 1d890e9 |
| ---: | --- | ---: | ---: | ---: |
| 4,347 B | cached open | 6,683 / 8 | 8,579 / 6 | 8,579 / 5 |
| 4,347 B | first displaced turn | 6,683 / 8 | 8,579 / 6 | 8,579 / 5 |
| 113,004 B | cached open | 116,062 / 34 | 10,844 / 6 | 10,844 / 5 |
| 113,004 B | first displaced turn | 116,044 / 34 | 10,844 / 6 | 10,844 / 5 |
| 113,004 B | retained turn | 3,028 / 5 | 10,780 / 5 | 10,780 / 4 |
| 113,004 B | post-save turn | 116,053 / 34 | 14,940 / 7 | 14,940 / 6 |
| 145,773 B | cached open | 0 / 0, miss | 0 / 0, miss | 0 / 0, miss |
| 145,773 B | first displaced turn | 149,021 / 42 | 10,877 / 6 | 10,877 / 5 |
| 145,773 B | retained turn | 3,236 / 5 | 10,813 / 5 | 10,813 / 4 |
| 145,773 B | post-save turn | 149,030 / 42 | 14,973 / 7 | 14,973 / 6 |

The two largest authored chapters exceed the old whole-scan attempt allowance:
those baseline turns abandon the cache and use ordinary layout. Their new
block-checked turns are hits; the CSV identifies that distinction. The opening
cap still refuses oversized payloads before any payload/manifest read.
Small files can read more bytes, and retained turns generally read more bytes
because they recheck complete blocks. No claim that every cell beats fallback,
or that host byte ratios predict device latency, is made.

Keep 4 KiB blocks: all 72 displaced-turn cells with expanded source at least 32 KiB meet the
phase-1 half-byte criterion: the largest after/before byte ratio is 0.4852,
including backward/middle/last cases and the large index. Pages
crossing block boundaries and partial tails are included. At the existing
4 MiB payload cap, 2 KiB blocks require an 8 KiB directory plus 2 KiB window;
8 KiB blocks require a 2 KiB directory plus 8 KiB window. Both exceed the
8,448-byte loan bound before query state. The 4 KiB choice fits both buffers
and state and avoids new memory ownership.

## Verification and remaining physical evidence

Pure codec coverage includes maximum directory, partial blocks, exact extent,
manifest/route/directory/block damage, over-cap self-consistent manifests,
exact extent/count geometry, directory substitution, frozen checksums,
retained-query damage, generation loss and read refusal. Removal controls must
fail for payload cap, exact extent, block count, manifest, route, directory
and block checks. The combined-read count control fails at `b9cdb32` too. Native consumer controls
must fail for the parse-loan guard, checked exit and publication order;
failed source-good entry performs zero raw query reads while still finishing
the authority scope. That regression fails at `b9cdb32`, alongside
the unchanged cache-only/pending-source controls. Native storage tests cover
38 finalization write/readback/sync/close/rename boundaries, both before/after
returned failures and abrupt process exit, requiring complete old or new values.
They also cover corrupt canonical versus complete backup, equal-time damage,
and both retained-directory and retained-payload admission repair cases.
Large retained admission and every transient route-refusal pulse pass current
code and fail the exact `b9cdb32` firmware headers. A separate probe-owned
cancel stops within the first 4 KiB block, preserves the usable lease, and
fails when forwarding that cancel is removed.
The existing producer tests retain cancellation, short/zero reads, EINTR,
allocation failure, temporary refusal, lost acknowledgements and three handles.

Resource receipts, complete gates, review and full every-page parity are recorded
in the implementation handoff. None of these tests uses hardware. A candidate
still needs the Lead's hands-on timing, heap and stack measurements before
owner acceptance; USB, battery sleep/wake and cold power-on remain separate.

Amendment to [069's cache timing row](069-book-open-attribution.md),
2026-10-23: with NEWT-34, `v[6]` covers the manifest/directory read and
verification of consumed blocks. The whole-file current-chapter check runs
in background `pc-current`, outside that foreground cache-query slot.

## Pathname attribution and query reuse, 2026-10-23

The Lead's later `d9a3e96` USB run attributes most cached-open `v[6]` to
pathname work: three probes take approximately 235 + 118 + 118 ms;
query acquisition and exit each take about 60 ms even with a held descriptor.
Attach is below about 1.5 ms, block work 4–23 ms, and seeks below 1.5 ms.
These measured splits supersede the earlier seek-dominance hypothesis below.

The three probes have distinct callers: `RevisionImportSession::selectAndVerify`
chooses the plain cached selection path; `ReaderRevisionSession::openRevision`
decides whether pending source verification must finish; `pageGate` permits
the first cache-only page attempt. Those decisions and the pending-source
and cache-only gates remain unchanged.

For a canonical pending-source cached open, both `e3913e2` and `d9a3e96`
perform one page-file open, nine descriptor `fstat`s and nine pathname
`stat`s: the first probe costs one open plus three stat/fstat pairs; each
later probe costs two pairs; the query costs entry and exit pairs. Thus ten
FatFs pathname walks occurred in both versions. The old first probe also
checked the whole NTCC file; the envelope changed data/CRC cost but had not
removed these walks. Exact counts assume no backup, error, replacement,
metadata displacement or reentry; other source/receipt/metadata work is separate.

The installed VFS `fstat` reads the open `FIL` size without `f_stat`.
Pathname `stat` calls `f_stat → follow_path → dir_find`; `f_open` also
calls `follow_path`. Compiled `dir_find` resets to the directory start and
scans entries linearly. This supports the inference of approximately 59 ms
per pathname walk in this device run; it does not establish card geometry
or directory occupancy. The page-file basename is actually 134 characters:
64-character book route key + hyphen + 69-character page identity. It uses
11 long-name entries plus one short-name entry: 384 directory bytes per
page file. A lookup after `k` such files can scan about `384*(k+1)` bytes,
plus other/deleted entries and parent-component scans. Cache hits can reduce
physical sector reads; a syscall is not a fixed-cost card operation.
Old-route files coexist with new-route files after migration, so directory
growth is a possible contributor to the slower walks. Neither a matched
baseline CQ split nor actual directory occupancy was supplied; that cause
is not established by the timing or code alone.

The fix reuses an existing held page descriptor for presence hints and skips
the query-entry pathname lookup, deferring it to the checked exit.
The three callers still run; only the first needs filesystem I/O when it
acquires that descriptor. It retains the original full probe checks. Each
block-verifying query checks descriptor type/size and live authority at entry,
then validates its freshly read directory and every consumed block. Its full
pathname check remains at checked exit, before page capture/publication.
Detectable pathname/metadata failures before or during a query still reject
its result. Host inode tests detect same-size/time replacements; FAT exposes
no inode witness for that case, as before. Fresh block/route checks remain
the byte-integrity authority. Failed descriptor entry or a failed exit path
check while the route remains allowed releases only its own generation and
keeps it latched unavailable until close. Outer and nested exits use the same
check; cancellation-only exit refusal preserves the descriptor. No directory,
block, hint object, pointer, buffer or extra flag survives a query; the existing
descriptor/route fields supply the hint. Metadata, receipt, writer, producer
admission and generic immutable-read checks retain their existing boundaries.

The canonical cached-open target becomes one open plus four pathname stats
(five walks, formerly ten). A held query becomes one pathname stat at exit
(formerly entry plus exit); new/displaced queries still open and fully check
their new lease. Expected CQ rows: the first probe remains about 235 ms,
the next two become nearly zero; held `acquire` becomes descriptor-only,
while `exit` remains about 60 ms. Subtracting the removed second/third probes
and held acquisition from matched `open*a/b` rows predicts `v[6]` of about
335–345 ms at position 0, 336–340 ms at position 1 and 315–325 ms at position 3.
Across all supplied cached-open rows the model spans 315–391 ms, preserving the
other measured action costs. This prediction preceded the Lead's USB results
above, which supersede it as measured evidence.
Retained query time should fall by its former approximately 60 ms pathname
acquisition.

A separate layout follow-up could use generic
`Pages/<book-route-key>/<page-id>`: about six FAT entries per book directory
in `Pages`, and seven entries per page file inside that book, instead of
twelve per page across all books. A generic prefix shard can bound the
book-directory scan too. This needs route migration, census/quota/recovery
tests and device measurements. No layout change is implemented here.

## Earlier device latency investigation, 2026-10-23

The Lead's USB run of `d8d987a` against `3154a93` did not establish a
latency improvement. In particular, position 1's cached open spent about
590–620 ms in `v[6]`, versus 410–440 ms, and a later retained turn spent
145–157 ms versus 79–81 ms. Fewer host page-file bytes do not establish
faster device reads. The current envelope can also read more bytes on a
retained turn: it checks whole 4 KiB blocks, whereas the old retained lease
served the SDK's smaller requested ranges.

The actual path is SDK `CacheStorage` → Newt `FilePort` → POSIX
`lseek`/`read` → Arduino SD's FAT VFS → FatFs → SPI SD. It does not use
the SDK's SdFat `FsFile` implementation. The installed Arduino 3.3.7 /
IDF 5.5.0 libraries compile fast seek **out**. Their `f_lseek` reuses the
current cluster for forward seeks or movement within that cluster; a seek
to an earlier cluster restarts at the first cluster and follows the FAT.
This was checked in the installed `libfatfs.a` disassembly because its
`ff.c` and `vfs_fat.c` source text is absent locally. Each open `FIL` has a
4096-byte allocated buffer, but SD's logical sector and cached data window
are 512 bytes. The filesystem has a separate sector window for FAT/directory
metadata. There is no whole-file or cluster-chain cache. Newt requests
40 MHz; this Arduino SD driver caps its operational setting at 25 MHz.
These are build facts, not measured bus timing or card geometry.

An aligned sequential 4096-byte read can use a multi-sector command, split
at cluster boundaries. Separate 4-byte reads pay separate VFS/seek work,
but need not each issue a card read: up to 128 adjacent CRC entries share
the file's cached sector. Aligned direct payload reads can leave that tail
sector cached. Thus neither syscall counts nor transferred bytes alone
predict the cost.

For payload `P`, block count `B=ceil(P/4096)`, cluster bytes `C`, and
`q(x)=floor((max(x,1)-1)/C)`, estimate a seek from cursor `x` to `y` as
`q(y)-q(x)` FAT links when `q(y)>=q(x)`, otherwise `q(y)` links from the
start. FAT-sector misses, fragmentation, partial-sector fills and SD
command latency add media-dependent costs. The device model is:

`v[6] ≈ presence/acquisition + seeks + data reads + CRC CPU + guards + SDK decode/copy + capture + tracing`.

The new query first reads the tail directory/manifest. The SDK then asks
for its header, trailing magic, index/anchors and selected page, which can
produce tail → block 0 → last block → earlier index → page. With one block
window, returning to block 0 can both rewind the FAT walk and rehash it.
Full `pc-current` verification and finalization are worse: their per-block
payload/4-byte-tail-entry alternation costs approximately
`sum_i(q(4096*i) + max(0,q(P+4*i)-q(min(P,4096*(i+1)))))`
chain links, roughly `O(B*P/C)` rather than a sequential `O(P/C)` scan.
That background cost is separate from foreground `v[6]`.

The smallest foreground candidate is to retain block 0 **within the same
query** while servicing the SDK's trailing-magic check, using the directory
buffer's unused space when `directory bytes + 64 + final-block bytes`
fits. Incremental verification in that space is another option; queries
without enough space retain the current single-window path. This would need
explicit layout/overlap, partial-block, cancellation and damage tests; no
validated state may survive the query. Another small independent candidate
is a read-only nibble/table CRC implementation with exact incremental
parity, if CRC dominates the new timing split. Neither promises to recover
every measured position before device attribution. A sequential pre-read
of the whole needed range could increase bytes for distant pages, so it
needs a measured crossover rather than an unconditional rule.

For background work, batch adjacent directory entries inside the existing
scratch and stream the matching payload blocks before the next tail trip.
Apply the same batching to directory generation and full candidate readback;
preserve complete readback, cancellation bounds and promotion. This is
separable from scheduling and retry policy. The current candidate adds
timing only. The Lead decides the next fix after measuring it; none of
these proposals changes policy, format, caps, idle memory or the SDK.

### Query timing rows

DEV USB adds numeric `CQ probe`, `CQ time` and `CQ work` rows, each bounded
below 160 bytes and dropped if TX space is unavailable. Schema 3 and its
fifteen action slots are unchanged. A probe measures the presence check,
including a new descriptor's open/stat/manifest hint when it occurs there.
A query's two rows share `id`, its starting `micros()` value; pair only
matching IDs. All durations are microseconds, using unsigned wrap-safe
subtraction. The query owns its counters in the existing parse loan;
there are no new resident pointers, buffers or clocks in non-DEV builds.
Including its counters, the DEV query uses 8,408 bytes on C3 and 8,424 bytes
on the host, within the same 8,448-byte bound and existing loan.

| Row | Fields |
| --- | --- |
| `CQ probe` | `id`, `us` inclusive presence/authority time, `ok` presence result |
| `CQ time` | `id`, `us` query construction through checked exit, `v=[acquire,attach,blocks,crc,seek,read,exit]`, `ok` checked SDK result before capture |
| `CQ work` | same `id`, `v=[bytes,reads,seeks,block-loads,window-hits,backward-seeks,backward-byte-span,payload,descriptor-already-held]` |

`attach` includes the directory/manifest read and checks; `blocks` includes
all block loads and their checks. `crc` (including manifest inspection),
`seek` and `read` are subsets of
those inclusive phases, so do **not** add the seven fields. The syscall
counters/timers cover consumed-query reads, including failed attempts;
they exclude presence/acquisition reads, whose time is in `probe` or
`acquire`. Byte counts include positive short transfers. Backward spans
compare only successive observed query reads, not the descriptor's initial
cursor or an exact FAT-link count. `descriptor-already-held` can be 1 on
a cached open when its presence probe acquired the descriptor first.
Query `us` excludes capture, presence probes and row emission. The action's
`v[6]` includes probes, capture and emission; later composition keeps its
existing separate slot. Sum all query/probe durations in that action and
inspect the remainder. A large `(attach + blocks) - crc - seek - read` remainder also
points to native guards and metadata work. Tracing and clock overhead is DEV-only; compare
matched instrumented builds and retain the uninstrumented device baseline.

## Lead device steps

1. Record the exact DEV USB build, orientation/settings, source size, power
   condition and reset evidence. Upgrading from engine revision 3 misses the old
   page routes; revision-4 NTBE records remain wire-compatible with this fix. Let source verification finish and allow the
   normal quiet producer warm-up; wait for a successful `PC attempt` row before
   measuring a compatible cached reopen. Do this on at least two different
   eligible chapters/books. A chapter above the existing production cap remains
   an ordinary fallback; do not change caps to prepare a measurement.
2. Home → Read the same locator/settings. Record `Reader first page: ms=...`
   (and `us=... pending=...`), `Reader digest done: ms=... verified=... changed=...`,
   the complete `Reader action` exclusive timing row and its raw `v[6]` cache-query
   slot. Record refresh, source/receipt and residual separately. Compare matched
   digest-complete and digest-pending cases. Existing DEV schema 3 is retained.
   For this investigation also capture every `CQ probe` and matched
   `CQ time`/`CQ work` pair between input and that action row. Repeat the
   same positions 0/1/3 and settings as the Lead's earlier run. Record
   `probe`/`acquire`, `attach`, block loads, CRC, seeks and reads separately.
   A high seek time with modest bytes supports FAT traversal; a high CRC
   time supports the CPU candidate; a high read time needs sector/command
   and transfer investigation. Do not infer cluster size from a byte span.
3. Measure first/middle/last Next and Previous under a retained lease; then
   repeat after a real save/checkpoint has displaced the page descriptor. Include
   forward/backward chapter crossings and the first post-producer turn. Keep
   the exact page-file payload size and hit/miss with each row. Check body pixels,
   page number, saved locator and anchors against the ordinary rendering.
4. On an authored damaged-current/next-block fixture, confirm there is no wrong
   cached page before fallback/verification, and that the existing quiet producer
   can repair it. Exercise queued input during finalization and save acknowledgement.
   Record minimum free reading heap and loop-stack margin; host static frames
   do not establish either margin.
5. Repeat the relevant journeys under USB power, unplugged battery sleep/wake,
   and cold battery power-on, recording each reset/power condition separately.
   No device is touched by this implementation task.

6. On an eligible authored fixture with a retained page-cache payload of at
   least 64 blocks (262,144 B), leave the early page displayed and let the quiet
   producer proceed to the next chapter. Record complete `PC attempt` rows,
   candidate/prefetch result and timing; the intact current candidate must not
   turn into Denied after approximately 128 reads or three seconds. Repeat with
   damage in the last payload block: it must permit repair rather than a false
   Complete. Capture `BG slow: work=pc-current ms=...` (emitted above 200 ms)
   and the delay from a
   queued Back/Sleep press to its handling during that complete verification.
   Existing schema 3 reports timing, not a page-open counter; descriptor reuse
   is asserted by the host syscall witness. Keep source
   verification complete, use matched settings/power/reset conditions, and do
   not raise production caps. Also interrupt `pc-current` three times, then
   observe whether the next chapter loses prefetch admission. The existing
   per-target retry accounting spends one of the next chapter's three tries
   on each interruption. This change widens that interruption window: at
   `e3913e2` the retained `pc-current` probe was a near-instant stat check;
   it now reads and verifies the whole current chapter and can be cancelled.
   A transient I/O error now returns Denied. This new exposure is tracked in
   [follow-up #98](https://github.com/VargasDevelopment/newt/issues/98), with
   NEWT-33 admission/stop coordination. This task changes no retry policy.
   If no eligible fixture is available, leave this
   hardware check pending rather than substituting the host result.
