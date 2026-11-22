# Page-turn work on the host

Status: **awaiting_device**. This is a host investigation of `73c5ed0` and the
pinned official FreeInk SDK `cd6f5b831c8fad899723b6d406586f00c575f021`.
No performance baseline/noise policy is approved. Published measurements use
authored fixtures only. No physical device, network, GitHub operation or
source-authority change was involved. The firmware wiring below is unchanged at
the measurement commit `ee38878`. The subsequent capped idle producer and its
production-path follow-up measurements are recorded in
[065](065-chapter-cache-producer.md); baseline statements below describe the
firmware before that producer was wired.

## Turn map

The firmware decodes/debounces the button and dispatches the Page-panel action.
The shared path is `handleReaderUi` → `SampleReader::turn` → `showPage` (same
chapter), or `locateExternal`/`restore` (chapter boundary). Previous at a chapter
start requests the preceding chapter's **last** page using the maximum anchor.
Book-edge clamps do no new page work. Failed chapters follow the existing bounded
skip/recovery rules; their costs are outside this valid-content matrix.

| Step to the composed frame | Size/position/direction/cache dependence |
| --- | --- |
| UI dispatch; page arithmetic | Constant. Direction determines target page or spine; clamps return without repaint. |
| `ReaderRevisionSession::receiptAction`, source probes, deferred receipt checks | Checked before publication. Payload verification and receipt I/O depend on consumed source blocks; chapter selection verifies the selected compressed range. No full-source digest/import/admission on these measured turns. |
| `ChapterPageCache::show` lookup | Same selected spine only. Current `ready(spine)` refuses a different spine, including a prepared destination cache. A miss uses the ordinary renderer; no cache build occurs during a native turn. |
| Derived-file validation, index and page read | A fresh SD cache-file lease verifies its footer and CRC by reading the complete cache file (`SdChapterCacheStorage::validate`). SDK then reads every 8-byte page-index entry; `validIndex` walks every entry again. Linear in chapter page count, plus bounded selected-page data. Anchor lookup adds index search for backward crossings/restores. |
| Chapter select/metadata/navigation/style resolution | New spine reopens canonical metadata and binds navigation/weights. Style resolution/watch can add passes; it is content-dependent. Same-chapter selection/totals can be reused. |
| ZIP reopen/seek/inflate | Ordinary path always starts the member at offset zero. Stored members read expanded input; deflated members restart inflation. Prefix work grows with target position. Selected compressed blocks retain their existing authentication. |
| SAX parse, entity filtering, style watch, line breaking, pagination | First successful chapter parse must reach EOF to certify totals and failure semantics. Later ordinary turns stop after the target page and settled styles, with chunk/paragraph lookahead. Work grows with prefix length; a backward last-page crossing needs the entire previous chapter. |
| Copy page plan, tear down parser and release borrowed frame | Bounded by page-plan limits; source/receipt guards still apply. No parser state survives. |
| Commit page/anchor/progress and `ReadingTracker::navigate` | Commit only after accepted rendering. Direction affects reading credit; normalized anchor remains the location authority. Metadata projection may add a checked derived-record read after crossing. |
| `renderReaderUi` → repaint captured page, chrome, footer/progress | Page-bounded raster work, no new chapter parsing. Orientation changes geometry, pagination, page count and the page-index cost. |

Firmware-only dispatch/scheduler timing, FAT/SD electrical timing, SPI/conversion,
waveform selection, display BUSY waits, ghosting and optical settling are absent
on the host. Runtime checkpoint/housekeeping and refresh scheduling also surround
the shared action. The host cannot assign a percentage of a device turn to these
CPU/storage/layout counts. In particular the earlier BUSY observations in
[044](044-device-latency-attribution.md) do not supply a current reader-turn
CPU/display split. Host microseconds below are not device milliseconds or a
prediction of user-visible latency.

## Cache production on firmware at the measurement baseline

At `ee38878`, firmware **never builds page caches during ordinary operation**:

- Open uses admission/receipt promotion, not the explicit cache-building import
  route (`firmware/reader_revision_session.h:135`, `:149`;
  `firmware/revision_import_session.h:51`). Turns pass `buildOnMiss=false`
  (`firmware/reader_revision_session.h:174–181`). There is no build after N turns.
- Idle services covers only (`firmware/main.cpp:685`,
  `firmware/library_cover_producer.h:122`); scheduler callbacks service
  cancellation/yield cadence (`firmware/reader_scheduler.h:70`). Neither builds
  chapter page caches or prefetches the next chapter.
- The separate explicit builder (`firmware/revision_import_session.h:33`) parses
  and lays out the complete chapter, with possible style-policy passes, then
  serializes pages/index and commits the file (`firmware/chapter_page_cache.h:206`).
  Work depends on expanded bytes, markup/layout work and serialized output size;
  it is not a constant-cost preparation.

Without externally prepared matching files, **0% of actual page turns hit a page
cache**: fallback prefix re-parse/layout is the baseline device path. The measured
**warm** cells instead describe an explicitly prepared cache. They are not a
steady-reading firmware baseline. Even with a prepared destination file, the
first cross-spine request falls back: `ready(spine)` requires the currently
selected spine (`firmware/chapter_page_cache.h:302`). Later same-spine turns can
hit an existing compatible file; no destination file is created by crossing.

For those conditional hits, consecutive same-file turns retain a stable lease
(`firmware/sd_reader_cache_storage.h:91`); whole-file CRC is required on a fresh
lease (`:99–103`, `:167`), not on every turn. The index is still reloaded and
validated each hit (`firmware/chapter_page_cache.h:172–187`). Metadata/title or
estimate I/O on the shared port closes the lease
(`firmware/reader_revision_session.h:206`, `:213`, `:301–306`), including after a
crossing; changing the cache filename also resets it. Session close and sleep
release it (`firmware/reader_revision_session.h:234`, `firmware/main.cpp:490`).
Opening a reader menu only pauses tracking (`src/reader_ui.cpp:126`,
`src/sample_reader.cpp:295`); exiting reading closes the session after checkpoint
handling (`firmware/main.cpp:425–439`).

## Method and counters

`tests/reader_turn_cost_test.py` extends the existing
`reader_storage_work_test.cpp` native `Device`, rather than implementing another
reader. It runs the real `SampleReader`, UI, `ReaderRevisionSession`, receipt and
SD-cache ports, borrowed-frame publication, phased reader and SDK. Only SHA's
platform port uses the existing host-provider shim. Same code and source/receipt
checks run on both paths.

Missing counters use generated **observation-only copies** under `.build`: decoded
chapter bytes in `ChapterSource`, actual miniz output in `ZipEntryReader`, SAX feed bytes/callbacks, emitted pages/runs,
cache outcomes/write attempts and validated index entries. Every insertion has a
unique source anchor and removing the insertion must recover the exact original
input. Original vendor/firmware/shared files are unmodified; firmware includes no
probe header, branch, field or counter. An uninstrumented native binary compares
all sampled full-frame hashes, normalized anchors and page totals to the instrumented run;
removing observations must fail the counter assertions. This is an instrumentation
negative control, not an old/new performance-fix comparison.

Counters per turn: source payload bytes; absolute source-read positions
(`source_seeks`, one `pread` address per nonzero request, **not** physical SD seeks);
zero-byte source probes; receipt/derived-cache read calls/bytes; source SHA bytes;
Admission/Metadata/Layout phase entries via existing `ReaderWorkEvent`; decoded
chapter bytes and actual inflater output bytes (`inflated`, including metadata); XML feed bytes/events (including
metadata/style passes); emitted layout pages/runs; cache hits/misses/build attempts;
validated index entries. `expanded` counts chapter bytes delivered to the parser. `inflated` counts
actual miniz output, including its up-to-32-KiB dictionary lookahead and any
metadata streams. `xml_bytes` counts filtered SAX input. These counters therefore
measure different work and need not agree. Emitted runs/pages exclude un-emitted
paragraph/line lookahead; they are not a count of every glyph metric call. The composed UI frame is hashed after each real turn. Wall time encloses
shared dispatch, source/storage guards and composition, excluding positioning,
cache preparation and reopen.

Class is declared before sampling: **short ≤ 65,536 expanded XHTML bytes; long >
65,536**. Fixtures have exactly 4,096, 65,536, 262,144 or 1,048,576 expanded bytes
in the middle chapter; two 8,192-byte authored plain chapters flank it. Markup is
plain paragraphs, dense inline bold/italic/style spans, or lists/tables handled by
the pinned SDK's block-flow implementation. Word permutations use seed 1649 and
preserve block byte length and the structural prefix across chapter sizes. Every
shape is tested stored and deflated. This covers structural/storage mechanisms,
not every EPUB feature, language, entropy profile or malformed-input class.

Portrait and landscape CCW use the same font, progress bar off, refresh interval
5. `first`/`middle`/`last` name the **source** page: first anchor zero, the page
containing the middle normalized character, and the last character's page.
Each gets Next and Previous. First/Previous and Last/Next cross **out** to the
8 KiB flank; their destination is short even when the source fixture is long.
`cross-in` Next and Previous cross **into** the measured middle chapter's first
and last page. This keeps source/destination size interpretation explicit.

Cold starts with derived page/metadata files purged with every reader/descriptor
closed. Native turns never create page files, so they remain absent throughout
the cold samples. Reopen at each source anchor certifies full chapter totals
outside PRESS. Receipts remain and reopens may publish title metadata: this is
**page-cache cold**, not a reproduction of #48's receipt-and-page purge or cold
admission. Only the purge/closed-reader/reopen ordering is analogous.
Warm requests complete caches through the existing production builder and format
before reopen. No firmware auto-build behavior is invented. Failed preparations
are retained and their fallback observations remain labelled as attempted warm;
`cache_hits=0` identifies them. Each sampled warm turn is the first PRESS after reopen. Metadata and page files
share one port; open-time metadata loading can close the page-file lease, so
this warm condition includes reacquiring and CRC-validating that file. It is not
a sustained, live-file-lease turn baseline. Warm OS page cache is uncontrolled in both modes;
these are derived-reader-cache conditions, not a claim of cold host disk or SD.

## Data and growth

The complete machine-readable matrix is
[064-page-turn-cost-matrix.csv](064-page-turn-cost-matrix.csv). Each cell has n,
median, nearest-rank p90 (`ceil(.9*n)`), maximum and population standard deviation
for **every** counter and informational host microseconds. Failed/partial warm
preparation evidence is in
[064-page-turn-cost-preparation.json](064-page-turn-cost-preparation.json).
The published run launches no competing build from this worker; host load and
OS caches remain uncontrolled. With n=3, p90 equals the maximum; no statistical
confidence is claimed. Identical counters have zero standard deviation, independently of timing variation.

There are **24 fixtures, 768 cells and 2,304 measured turns**. Each cell has n=3.

| Expanded middle chapter (B) | Markup | Deflated (B) | Stored (B) | Pages portrait/landscape | Cache preparation |
| ---: | --- | ---: | ---: | --- | --- |
| 4,096 | plain | 691 | 4,096 | 7/7 | complete |
| 4,096 | styled | 721 | 4,096 | 6/6 | complete |
| 4,096 | list-table | 771 | 4,096 | 6/7 | complete |
| 65,536 | plain | 7,476 | 65,536 | 118/118 | complete |
| 65,536 | styled | 6,013 | 65,536 | 86/101 | complete |
| 65,536 | list-table | 6,709 | 65,536 | 103/118 | complete |
| 262,144 | plain | 28,633 | 262,144 | 473/473 | complete |
| 262,144 | styled | 21,828 | 262,144 | 345/403 | complete |
| 262,144 | list-table | 24,603 | 262,144 | 413/472 | complete |
| 1,048,576 | plain | 113,183 | 1,048,576 | 1893/1893 | OutOfMemory (7); retained fallback |
| 1,048,576 | styled | 84,951 | 1,048,576 | 1381/1611 | OutOfMemory (7); retained fallback |
| 1,048,576 | list-table | 95,963 | 1,048,576 | 1652/1888 | OutOfMemory (7); retained fallback |

Representative **portrait/plain/stored** cells below show work and host time.
The full CSV includes both methods, all shapes/orientations, out-crossings and
every counter/statistic. `E` is chapter bytes delivered; `R` is emitted runs.
Each E/R value below has median=p90=max and σ=0; n=3. Host timing tuples are
**median/p90/max/σ milliseconds**, informational only.

| Expanded (B) | Cache request | Source position → action | E (B) | R | Host ms [median/p90/max/σ] |
| ---: | --- | --- | ---: | ---: | --- |
| 4,096 | cold | first → next | 2,048 | 224 | 1.280/1.372/1.372/0.049 |
| 4,096 | cold | middle → next | 4,096 | 447 | 1.999/2.026/2.026/0.013 |
| 4,096 | cold | last → previous | 4,096 | 447 | 2.028/2.048/2.048/0.021 |
| 4,096 | warm | middle → next | 0 | 0 | 0.817/0.819/0.819/0.002 |
| 4,096 | cold | cross-in → next | 4,096 | 522 | 2.249/2.333/2.333/0.045 |
| 4,096 | cold | cross-in → previous | 4,096 | 522 | 2.280/2.286/2.286/0.019 |
| 65,536 | cold | first → next | 2,048 | 224 | 1.271/1.356/1.356/0.040 |
| 65,536 | cold | middle → next | 34,816 | 4,692 | 13.355/13.477/13.477/0.064 |
| 65,536 | cold | last → previous | 65,536 | 8,835 | 24.779/24.784/24.784/0.039 |
| 65,536 | warm | middle → next | 0 | 0 | 4.433/4.434/4.434/0.002 |
| 65,536 | cold | cross-in → next | 65,536 | 8,911 | 24.995/25.034/25.034/0.019 |
| 65,536 | cold | cross-in → previous | 65,536 | 8,911 | 25.285/25.355/25.355/0.040 |
| 262,144 | cold | first → next | 2,048 | 224 | 1.357/1.385/1.385/0.040 |
| 262,144 | cold | middle → next | 133,120 | 18,057 | 49.989/50.540/50.540/0.302 |
| 262,144 | cold | last → previous | 262,144 | 35,660 | 97.560/98.447/98.447/0.422 |
| 262,144 | warm | middle → next | 0 | 0 | 15.963/15.979/15.979/0.025 |
| 262,144 | cold | cross-in → next | 262,144 | 35,696 | 99.909/99.925/99.925/0.178 |
| 262,144 | cold | cross-in → previous | 262,144 | 35,696 | 100.961/104.285/104.285/1.574 |
| 1,048,576 | cold | first → next | 2,048 | 224 | 1.334/1.395/1.395/0.037 |
| 1,048,576 | cold | middle → next | 526,336 | 71,685 | 195.559/196.684/196.684/0.625 |
| 1,048,576 | cold | last → previous | 1,048,576 | 143,020 | 388.817/390.378/390.378/1.085 |
| 1,048,576 | warm | middle → next | 526,336 | 71,685 | 195.285/195.385/195.385/0.075 |
| 1,048,576 | cold | cross-in → next | 1,048,576 | 143,059 | 387.749/390.083/390.083/1.109 |
| 1,048,576 | cold | cross-in → previous | 1,048,576 | 143,059 | 393.970/395.692/395.692/1.153 |

There is a second storage cost on these post-reopen warm hits: the SD port's
fresh-lease CRC scans the **whole derived cache file**, before index/page reads.
For portrait/plain/stored middle/Next, 7, 118 and 473 pages produce respectively
9,964 / 148,300 / 590,025 storage bytes and 22 / 267 / 1,051 read calls (medians,
n=3). Their validated index entries are 7 / 118 / 473 and source bytes are zero.
Thus zero XML/layout does not mean chapter-independent storage work. Repeated
turns retaining the same file lease can amortize the CRC; this matrix does not
establish that sustained-lease timing. The index is still reread on each hit.

The largest measured chapter has 1,893 pages. Its current C3 writer
would need at least **16,956 index bytes** (8,764 above 8 KiB),
before any change to cache ownership. Its read index alone needs
15,144 bytes with no anchors. These are capacity calculations from
the existing format/structure, not an approved RAM increase.

Cross-compression verification matched sampled full-frame hashes, normalized
anchors and page totals in **1,152 stored/deflated pairs** across the
whole matrix. This covers sampled pages, not every page of every chapter.

- Deflated first/Next at 4,096 B: delivered 2,048 B; actual inflater output 4,096 B (median, n=3).
- Deflated first/Next at 65,536 B: delivered 2,048 B; actual inflater output 15,369 B (median, n=3).
- Deflated first/Next at 262,144 B: delivered 2,048 B; actual inflater output 15,093 B (median, n=3).
- Deflated first/Next at 1,048,576 B: delivered 2,048 B; actual inflater output 15,111 B (median, n=3).

Growth law: ordinary same-chapter first/Next is bounded by the first pages
and decoder/chunk lookahead; middle and late targets are linear in consumed
chapter prefix. Full incoming crossings are linear in destination expanded
bytes, in both directions. XML events/runs also depend on markup density.
Stored source bytes track consumed bytes; deflated source reads track compressed
input plus decoder lookahead/authentication. They are not a universal linear
function of expanded bytes. A successful same-spine cache hit has zero chapter
XML/layout/inflation work, but first-lease cache CRC is O(cache-file bytes), and index entries/read work
are O(page count).
At 1 MiB, failed cache builds cause attempted warm cells to take the same
ordinary growth path. These are mechanism classifications from work counters,
not fitted device-latency laws.

Expected device signature: once chapter totals are certified, a same-chapter
turn repeats parse/layout through the **target** position. Steady forward reading
should therefore show a position sawtooth: increasing prefix work within each
chapter, resetting near its start after the crossing's full-chapter pass.
Backward turns likewise depend on target position; a backward crossing into the
last page requires the whole destination chapter. This predicts parse/layout
work, not total device latency.

For typical chapters in the tens-of-KiB range, the authored 4/64-KiB cells bracket
useful cases; 256 KiB extends the size sweep. In the portrait/plain/stored cells,
first/Next delivers 2 KiB and emits 224 runs at all three sizes. Middle/Next
instead delivers 4/34/130 KiB and emits 447/4,692/18,057 runs; last/Previous
delivers 4/64/256 KiB and emits 447/8,835/35,660 runs. Corresponding **host-only**
median times are 1.280/1.271/1.357 ms (first), 1.999/13.355/49.989 ms (middle), and
2.028/24.779/97.560 ms (last). Backward incoming crossings consume the full
4/64/256 KiB, with host-only medians 2.280/25.285/100.961 ms. These authored
examples quantify the mechanism; they establish neither chapter-size prevalence
nor device milliseconds.

## Decision and limits on a fix

There **is** a host-demonstrable shared-code cause. Ordinary turns repeat
prefix parse/layout from chapter byte zero, so the middle/late-page work grows
with chapter length; cold crossings do complete destination-chapter work in both
directions. A prepared cache removes this work only for a same-selected-spine
hit. First-lease whole-cache CRC and per-hit index reading/validation also grow
with chapter output size/page count. At the largest
sizes, cache construction exceeds its index arena and warm attempts fall back.

No production algorithm change is included in this original measurement. The
writer reserves 1,536 bytes for
192 anchors, then chunks of 128 page entries (1,032 bytes on host, 1,028 on C3).
Six chunks fit in 8 KiB: 768 pages. A read index needs 8 bytes per page plus
anchors. Increasing these allocations requires owner RAM approval.

Design options only; flash and change-size estimates below are unmeasured.
Every option targets **zero additional RAM and unchanged or tighter selected
stack frames**, requiring four-variant verification before claiming that result.
All preserve cache format/identity and source authority:

| Option | Mechanism, expected benefit and risk | Estimate |
| --- | --- | --- |
| (a) Bounded-memory index | Stream the final index from provisional page blobs via guarded `readBackAt`; read index records from SD instead of allocating all entries. Preserve transaction/footer checks and existing file/page limits. Enables prepared caches above 768 pages (including authored 1-MiB cells); trades bounded memory for I/O. Bounds, interrupted writes and readback/commit failures need coverage. | +3–8 KiB flash; 350–600 production lines. |
| (b) First cross-spine prepared hit | Stage canonical selection in the existing inactive slot, verify compressed source, bind styles/navigation and commit only after accepted cached rendering. Saves whole-destination parse/layout on prepared crossings, retaining source-verification work; 1-MiB fixtures also need (a). Preserve old page/anchor and all generation/receipt guards on failure; relaxing `ready(spine)` alone is unsafe. | +1–3 KiB flash; 120–220 lines. |
| (c) Lease/CRC/index amortization | Retain full CRC and semantic-index validation once per stable lease, with generation-bound certification in existing storage; read only selected index records thereafter. Same-file CRC retention already exists. Avoid unnecessary metadata eviction without suppressing fault retries. A 473-page prepared cell could read roughly two index entries plus headers instead of 473; avoiding reacquisition also avoids its whole-file CRC. Invalidate certification at every lease/source/settings/failure boundary. | +1–4 KiB flash; 150–300 lines. |
| (d) Cheaper fallback prefix | Check compiled code, then consider an early equivalent low-codepoint return in `semanticWidth` and immutable width-table storage (`src/reader_font.cpp:89`). ASCII glyph lookup is already direct. May lower per-glyph classification cost on all applicable fallback cells; bytes/XML/runs and O(prefix) growth stay unchanged. Preserve every metric, Unicode boundary and rounding result; discard if the compiler already optimizes it. | −0.25 to +0.5 KiB flash; 20–50 lines. |

A **cache producer** was a separate architectural decision at this measurement:
when to build, cancellation, idle/battery/SD-write and power-lifecycle effects,
and workspace ownership all need a policy. Options **(a)–(c) only pay off once
such a producer exists** (or files are explicitly prepared externally); none
speeds the baseline uncached device path. The owner subsequently selected the
zero-RAM capped idle design in [065](065-chapter-cache-producer.md). An idle/Home
helper does not by itself
establish a safe zero-additional-RAM page-cache producer. Keeping an inflater,
XML parser and layout continuation alive across displayed frames likewise has no
proven zero-additional-RAM design in the existing borrowed workspace.

The original benefit/risk ranking was **(d), then (c), (b), (a)** conditional
on a separately approved producer. First slice: inspect/profile (d), retaining a
change only with demonstrated deterministic work reduction and exact metric/page
equality. No speedup magnitude is established. For (a)–(c), additionally test
prepared-cache equality, capacity/crossing/lease work-bound negative controls,
and corruption, read/write/commit, metadata, cancellation and rollback faults.

Any future fix must retain every-page frame/anchor equality across both
orientations/directions/crossings/reopen and applicable sleep/wake cases, a
work-bound test that fails on this algorithm, heterogeneous corruption/storage/
cancellation failures, and unchanged or tighter RAM/selected-stack budgets. No
such fix, equality claim, measured improvement, or physical speedup is claimed
by the original measurement commit. The existing frame control validates only the
observations, and the existing full gate retains failure coverage.

Only authored-fixture data belongs in this report; it establishes a generic
mechanism, not its prevalence across a library.

## NEWT-9 device protocol — proposal for owner review

Use #48 `reading-ready.py` (reference inspected at
`6415038a7f751504d06e37c77aef31c133b5e7fa`), `reading-acceptance.md` and its explicit
`--latency-mode warm|cold` on the final integrated source. This branch does not
modify #48 or claim its review state is clean. Start each run at Home, use its
single connection/two HELLO checks, exact installed image/readback receipt,
private device/media/plan bindings and owner-declared chapter class. Use separate
private plans for each generic size class; never infer it from title or digest.

Record exact source/tooling/procedure identity, image SHA/bytes, SD identity and
filesystem/setup, temperature, orientation, font/bar/refresh settings, owner
observed power state and reset/boot evidence. USB-powered, battery-powered
sleep/wake, and cold power-on are distinct conditions. USB serial sampling cannot
establish an unplugged battery result. Hands-on checks remain pending; this is
not a candidate ready for owner acceptance. Power/reset transitions occur outside each fixed serial journey;
an in-journey reset invalidates that run.

**Primary measurements:** first/middle/last same-chapter turns in both directions,
plus forward crossings into first pages and backward crossings into last pages,
on chapters in different declared generic expanded-size classes (for example
4/64/256-KiB classes). Record actual bytes and target anchors. Select an adjacent
turn pair inside the chapter near each position; an outward book/chapter edge is
a crossing, not a same-chapter sample. Use #48's request-bound stage rows.

Keep cold open/strict selection, warm reopen, navigation/menu, any explicit
loading/index preparation, and sleep/wake/reopen correctness as separate
journeys. Do not add their intervals together or substitute book-open
measurements for turns. Cold sampling must verify closed-reader DEV
purge, zero failures and the subsequent newer reopened generation before PRESS.
#48 **warm means no purge, not a cache hit**, unless a hit is observed. At the
measurement baseline both modes take fallback without external preparation.
For the candidate in [065](065-chapter-cache-producer.md), distinguish immediate
fallback turns from later observed hits after eligible quiet-idle production;
waiting two seconds alone does not prove that production completed. Retain
misses, absent census credit, cancellation and failed preparations as separate
conditions. Record the producer's 128-page eligibility separately from byte-size
class. Legacy capacity experiments above 768 pages remain supplementary.

For the fixed #48 journey, position the primary so its first forward turn stays
in the same spine and the second crosses; its existing fixed stages retain both
crossing directions and exact anchor checks. `--latency-cycles 20..100` samples
its adjacent same-spine anchor pair, not arbitrary first/middle/last or repeated
crossings. Repeat the existing complete journey with owner-prepared positions to
cover the three positions. Accumulate crossing samples over complete journeys
bound to the same planned anchors/conditions; do not relabel same-spine samples
as crossings or improvise unreviewed serial commands. Keep each session within
600 seconds/2048 audited records and forecast the cold-mode wire budget. A
sampler extension, if needed, is separate tooling scope.

Proposed floor: **20 valid individual samples per exact cell**: source/destination
expanded-size class and byte count, position or crossing, direction, derived-cache
mode/hit outcome, page-file lease state (first post-reopen or sustained),
orientation, power condition and uniquely observed refresh
class. Both orientations and warm/cold are mandatory. Collect 20–100 cycles when
the wire forecast permits; repeated journeys can fill rare full-refresh/crossing
groups. Retain unknown refresh modes and partial/malformed stage rows as
incomplete evidence, never a passing group. Retain failed requests and stop
according to the contract; no automatic retry or dropped outliers.

For each valid group report n, individual samples, nearest-rank p50/p90/p95/max
and population stddev for both wrapper and protocol endpoints, plus the matching
request-bound nine-stage rows, calls, total/residual/flags and STATUS refresh
interval separately. Chapter classes alone cannot distinguish size from target
position: compare size sweeps at a fixed normalized position and positions within
the same chapter. Confirmation on the **fallback** path requires a parse/layout **position sawtooth**:
within-chapter growth, a reset near each start, and whole-chapter backward
crossings, with a size-dependent envelope. No prepared-cache hit is required to
test that baseline hypothesis. Candidate hits must separately demonstrate the
removal of parse/layout stages and account for cache validation/index stages;
the host's work reduction alone does not establish a device latency improvement.
Refutation of prefix work as the dominant observed latency explanation means
display or other stages dominate and do not vary with position; absence of a
parse/layout sawtooth also refutes the predicted stage signature. Distinguish a
real but small parse/layout sawtooth beneath a dominant display floor from a
claim that it dominates total latency. Cache-capacity failure is a separate
finding, not a warm-cache speed regression.

The candidate must improve every affected device cell beyond the independently
reviewed noise allowance, preserve unaffected cells against the ratchet, satisfy
correctness/anchors and all resource gates, and retain optical/buttons and battery
acceptance as separate owner checks. Host timing ratios cannot set that allowance.

Proposed baseline/noise policy: keep `docs/reading-latency-policy.json` unchanged
and unapproved. Gather at least three complete repeated baseline sessions under
each exact condition, review the raw bound bundles independently, then propose
per-group absolute millisecond allowances from repeatability rather than silently
raising them from a candidate. The owner/reviewer must choose the actual numeric
allowances, baseline digest and rationale before candidate evaluation; this report
approves none. Compare p50/p95/max independently for each contract endpoint/group,
with the declared sample floor and reviewed allowance, never pool power, cache,
orientation, anchors or refresh classes. Report 500/750/1000 ms goals separately.
Stale/unknown/inadequate evidence blocks admission. No baseline or noise value is
automatically grown, and p90/stddev supplement rather than replace the contract's
p50/p95/max ratchet.

## Verification

All Python runs used the managed interpreter first on PATH as required. No
installs, network, device actions, commits, pushes, PR operations or history edits.

| Run | Result |
| --- | --- |
| Focused matrix development (`--quick --uninstrumented-control`) | Two early rc=1 harness failures: invalid synthetic saved anchor, then an unhandled optional cache-capacity failure. Positioning was corrected; capacity failures were retained rather than counted as warm hits. Subsequent focused run passed 128 cells plus uninstrumented frame/anchor control. |
| `tests/reader_efficiency_test.py` | rc=0; existing exact pixel/anchor oracle, directions, orientations, late positions, crossings, reopen, style/fallback, source loss/cancellation and rollback controls pass. |
| `tools/code_health.py` | rc=0; structural/boundary ratchet, pinned dependencies and generators pass. |
| Initial 18-fixture characterization | rc=0, 576 n=3 cells; superseded by the four-size run. |
| Intermediate four-size matrix | rc=0, 768 cells, 413.15 s; delivered deflated bytes were not yet separated from actual inflater output. Superseded. |
| Final four-size matrix with actual miniz output | **rc=0, 337.65 s**, 768 cells / 2,304 samples, all statistics in the CSV. |
| Post-run evidence consistency/compression comparison | rc=0; 768 n=3 cells, 288 preparation records, 1,152 matched sampled frame-hash/anchor/page pairs. Full report samples preserved across later smoke runs. |
| Requested quick invocation without profile | rc=2, 0.04 s: this checkout requires explicit Linux profile; no tests ran. |
| `tools/check.py --suite quick --profile linux` | **rc=0, 73.92 s**, including matrix and counter-removal control (prior delivered-byte instrumentation). |
| `tools/check.py --profile linux` on final instrumentation | **rc=0, 226.40 s**, end to end; includes the new 128-cell smoke test and instrumented/uninstrumented control (42.649 s), ASan/UBSan, existing failure suites and reviewed renderer frames. |
| Round-3 docs correction: `tools/check.py --suite quick --profile linux` | **rc=0, 51.68 s**; includes 128-cell matrix and observation controls. |
| Round-3 separate `tests/reader_turn_cost_test.py --quick --uninstrumented-control` | **rc=0, 26.43 s**; 128 cells, sampled frame/anchor equality and counter-removal negative control pass. |

The final full gate exercises the final actual-inflater counter. No prior test
assertion or golden anchor was weakened. The existing harness only gains an
optional settings argument (same default) and a separately selected matrix main.
Observation anchors are exact unique insertion sites and fail closed if source
changes. No production source, source identity rule, transaction or cache-format
change exists to validate as an optimization. All-frame equality for a future
optimization remains outstanding; sampled hashes here must not be promoted to it.

`tools/firmware-lanes.py` ran all four clean health builds. Aggregate rc=1 because
both DEV variants hit the explicitly anticipated dirty build-ID overhead. Deltas
are against the committed `73c5ed0` artifacts; **every stack report entry is equal**,
not merely the largest selected frame. No resource record/ceiling was edited.

| Variant | Result / duration | Flash (B; delta) | RAM (B; delta) | Selected stack delta |
| --- | --- | ---: | ---: | --- |
| default | pass, 38.684 s | 1,473,184; +0 | 133,248; +0 | all +0 |
| dev-usb | rc=1, 63.097 s; dirty ID exceeds ceiling by 16 B | 1,611,056; +16 | 134,080; +0 | all +0 |
| regional | pass, 87.777 s | 1,479,840; +0 | 133,744; +0 | all +0 |
| regional-dev-usb | rc=1, 112.199 s; dirty ID exceeds ceiling by 16 B | 1,619,680; +16 | 134,576; +0 | all +0 |

The original dirty-tree results above are not a four-variant pass. On committed
`ee38878`, the Lead subsequently reported all four builds passing (93 s) and the
full host gate passing (221 s). This documentation correction changes no firmware
code. Tracked `artifacts/*` remain exactly as the tools wrote them. The Linux gate
explicitly leaves arbitrary ImageIO PNG/JPEG coverage to `tools/check-mac-covers.py` on this exact source. Fresh adversarial and
maintainability reviews belong to the Lead's next stage and are not claimed here.
No new optimization was made, and no owner physical/performance acceptance closes.

Reproduce the full authored report (runs no hardware):

```sh
cd ~/worktrees/newt-turn-perf
/usr/bin/env PATH=~/worktrees/newt-x3-port/.build/python-bootstrap/python/python/bin:/usr/bin:/bin python -B tests/reader_turn_cost_test.py --report docs/064-page-turn-cost-matrix.csv
```

