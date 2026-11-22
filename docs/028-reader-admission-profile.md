# Reader admission causal investigation

M1 remains blocked by the sibling open. This checkpoint changes observation only;
the 20-second cooperative opening budget, 30-second terminal deadline, complete
23-stage journey, full source SHA identity and all admission checks stay intact.
No device, USB, card or private-book action is part of this source task.

## What the actual capture establishes

The installed `3b1041e2f901-regional-experiment` sibling open used 1.305 seconds
for opening paint, 6.052 for selection/hash and 12.734 in the reader. Its
20.094-second total reached the unchanged budget; admission consumed 12.642
seconds and its measured waits only 142 milliseconds. Metadata and layout did
not begin. The terminal reply arrived at 21.440 seconds. This identifies the
phase but cannot distinguish SD calls from stream processing. Primary pages
and exact reopen to anchor 1257 passed; ten of 23 stages validated. The latest
device progress is authoritative; historical backups must not replace it.

The budget starts before paint and selection, leaving approximately 12.6 seconds
for sibling admission in this capture. A sibling reader interval longer than
that can fail even if it would fit a reader-only 20-second budget. There is no
measurement yet of how long its complete validation and first page would take.
Neither the deadline nor the algorithm can be judged appropriate for the target
book from the available timings. Increasing the budget or skipping checks would
be a new behavioral decision, not a verified performance correction.

## Source and authored EPUB evidence

`src/epub_admission.cpp` opens and fully reads every ZIP member before metadata
and layout, including images unused by the reader. It checks container/local
header structure, duplicate names, supported methods, stream results and exact
expanded lengths. The pinned SDK stream reader and current admission do **not**
compute ZIP CRC32. Earlier mentions of possible admission CRC cost were a
hypothesis; there is no such measured or implemented CRC stage. Full file
SHA256 remains the independent book identity. NLC2 thumbnail CRC is unrelated.

The existing SD mount requests 4 MHz. That setting is unchanged; measured source
bytes and file-call time can establish observed throughput before any proposed
clock or access-policy change.

`tools/reader-admission-profile.py` generates seven real, authored EPUBs and runs
the production reader, file adapter and pinned SDK with ASan/UBSan. Six valid
fixtures preserve identical chapter text, anchors, totals and four sampled page
frames while varying one unused PNG's representation. The seventh uses a
reserved DEFLATE block type in that unused image: existing admission rejects it
before metadata/layout. It is not a CRC corruption test.

Stored 64 KiB versus 1 MiB images vary bytes read and expanded chunks. Repeated
versus pseudorandom 1 MiB images hold expanded bytes fixed while changing compressed
input. A third pair holds compressed input at 1,200,000 bytes while changing
expansion, but also changes empty DEFLATE block counts; that confound prevents
attributing their timings solely to expansion. Host warm filesystem timings
cannot predict C3 or card latency. All unused compressed spans are read and
expanded, including the unused image. The original preinstrumentation report is
retained privately in the ignored build directory; paired detailed/phase-only
runs match its exact fixtures, page frames, source reads and yield counts.

Since staged admission (docs/054), admission expands only container.xml, the
OPF and the nav. The damaged-image fixture now opens with identical pages, and
the tool asserts that instead of the all-member coverage above.

## Diagnostic contract

One fixed numeric profile belongs to the existing reader-open stack scope.
Initial SD selection includes source hashing; nested reader scopes borrow the
outer profile and only its owner emits. Both lexical scope objects still have
automatic storage; native DWARF measures profile 432 and scope 444 bytes. True
reopens have their own scope. The old 6,616-byte free-stack observation does not
certify this diagnostic. Actual costs and partial-frame limits are recorded in
`docs/CODE_HEALTH.md`.
Callbacks allocate nothing, print nothing and retain no book content, filenames,
catalog pointers or rasters. The same file handle, byte counts, cursor, seek
policy, source bounds and cancellation checks are preserved.

The diagnostic separates:

- Existing admission/metadata/layout elapsed, callbacks, sleeps and wait-ms.
- ZIP structural inspection, catalog open and actual duplicate comparisons.
- Member ordinal, method, coarse extension category and declared/produced bytes;
  aggregate counts, current/failed member and maximum inclusive stream time.
- Each `ZipEntryReader::read`, including EOF/failure, and its nested file-call
  microseconds/read bytes. Stream time includes decompression, copies, callbacks,
  waits and bookkeeping; subtracting nested I/O does **not** produce exact inflate
  CPU time. Member-open work is outside the stream interval.
- File size, position, seek and read call counts/times/bytes in five buckets:
  other 0, admission 1, metadata 2, layout 3 and source SHA hashing 4.
- Inclusive SHA duration, update calls/time/bytes and its existing wait calls/time.
  SHA setup/finalization is included in total time, not separately timed.

Durations are inclusive subsets: never add nested I/O or waits to their parent.
Counters use unsigned microsecond subtraction and are tested across clock wrap.
The unchanged bounded opening normally limits durations well below counter wrap;
a blocked SDK call still cannot be interrupted by the cooperative deadline.

Output uses fixed 160-byte formatting and exact serial write acknowledgement,
with no new waits or callback-time output. Twelve data-row bits correspond to
work 0, stages 1, member counts 2, streams 3, SHA 4, I/O buckets 5–9, current 10 and
maximum 11. A complete capture needs all twelve distinct newline-terminated rows
and `Reader rows: emitted=4095 expected=4095`. The marker alone does not prove
host delivery. A missing/truncated/duplicate row, missing marker or smaller mask
is incomplete evidence. No retry is implied by incomplete diagnostics.

## Next causal decision

An exact diagnostic image needs a separate coordinator grant. Its bounded capture
must cover primary/sibling real opens, observed row completeness and postpaint
stack margin using the existing owner workflow. Preserve current progress and
normal acceptance/recovery rules; do not restore an old image's NVS snapshot.
If SD-call time dominates, investigate measured card/file access first. If
stream processing dominates, use current/maximum member and image aggregates to
justify a bounded decompression investigation. Do not skip unused-member
validation or introduce an alternate book-identity path from authored controls.
Exact inflate timing can be a later scoped probe if the residual remains unclear.

Reading recovery, settings, battery sleep/wake, both physical orientations,
Library focus/ghosting and owner acceptance remain pending. Approved Library
checkpoint 0993cd8 remains integrated. Optional thumbnail freshness uses source
size and POSIX modification seconds rounded down to a two-second boundary;
actual FAT timestamps, cover preparation and card transfer need owner hardware
coordination. Trial freeze and apps/SDK follow M1 acceptance.

## Exact diagnostic proposal

Compiled source: `4a31ec31750b8877a2fb555375fd2f1d6c3eb13b`. The complete host
gate passes, including real cancellation/recovery and exact pages in both
orientations. All four final clean native health builders pass. Their canonical
outputs are preserved under `.build/admission-final-native`; each variant has
334 hash-bound outputs, unchanged static SRAM/RTC, and all 28/30 exact selected
frame guards. Authored numeric evidence is `artifacts/m1-admission-causal-host.json`;
the final build/proposal record is `artifacts/m1-admission-diagnostic-builds.json`.

The proposed regional DEV image is `.pio/regional-dev-usb/build/newt_x3/firmware.bin`,
build `4a31ec31750b-regional-experiment`, **1,257,744 bytes**, SHA256
`e7678f6c02ebbc137e881f3a219174ef0a0ebc2228e352ebe4424f5b2a3aaf1c`.
Its exact carrier span would be **308 sectors, [0x780000,0x8b4000)** with
**3,824 trailing FF bytes**. This extends the previous installed span by one
sector. A new grant must bind this image/span and fresh current preservation
evidence. No grant, installation or device measurement is part of this checkpoint.

The profile is best effort. The parent should treat incomplete captured rows as
incomplete causal evidence and stop without an automatic retry. Full file-call
time is a nested stream subset; a CPU residual still includes inflate, copying,
callbacks and bookkeeping. This candidate supplies measurements, not a claim
that sibling reading now succeeds. Current device progress remains authoritative.
