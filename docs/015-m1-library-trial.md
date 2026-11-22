# M1 source candidate and remaining trial gates

This source candidate adds cold EPUB admission and scheduling to the existing
SD library reader. It has not been flashed or tested against hardware. The
library remains read-only; Newt progress uses separately guarded NVS records
with complete SHA-256 identity. Legacy state and credentials are never imported.
The earlier measurements in documents 012–014 describe their respective slices;
the current limits and aggregate results are recorded here.

## Selected-book persistence

A successful real page open records the selected filename and complete digest
in a separate Newt-only two-slot, 144-byte durable wire format. Ordinary boots
load that validated selection read-only and ignore RTC selection bytes. Every
subsequent Read hashes the actual source before using its independent progress.
Unknown schemas, extents, reads and competing generations block replacement
and silent sample fallback; a valid current-version slot survives a torn peer.
Selection writes use the inactive slot and verified read-back after successful
page publication. A lost acknowledgement can become newest after restart if
the complete commit actually landed. An unverified selection remains visibly
warned while its current book stays readable. Eligible RTC recovery keeps a
newer selected source/checkpoint and checks durable identity read-only to carry
that warning through wake. Neither the 68-byte sleep nor 140-byte SD RTC record
changes, and no progress or selection is imported from migration evidence.

## Runtime boundaries

| Boundary | Current behavior |
| --- | --- |
| Library | `/Newt/Books`, at most 128 filenames, at most 95 UTF-8 bytes per name; controls, malformed UTF-8, hidden names and paths rejected. Catalog admission and RTC selection share one predicate. Any scan failure publishes no partial catalog. |
| Container | At most 64 MiB compressed; ordinary single-disk ZIP, stored or deflate, at most 1,024 members and 1 MiB central directory. ZIP64, encrypted members, duplicate names, NUL names, names over 1,024 bytes and inconsistent headers are unsupported. |
| Expansion | At most 4 MiB per member and 64 MiB cumulative. Every member is streamed before metadata parsing; actual decoded length must equal the admitted length. Metadata declarations alone cannot bypass the limit. CRC validation is not added by this admission layer. |
| Scheduling | At each successful source read of at most 4 KiB and each decoded chunk of at most 2 KiB, independent of page emission. Hashing yields after at least 64 KiB read, including short reads. These bound application scheduling intervals by work chunks, not milliseconds of SD or inflater latency. |
| Layout | Two independently allocated 48 KiB arenas; metadata is destroyed before its arena is reused for layout. At most 4,096 pages per chapter. SampleReader owns navigation and permits at most eight consecutive empty spines. Firmware reuses the idle display buffer as staging and publishes only successful layout, or a complete explicit unavailable scene. No partial page replaces the published frame or progress. |
| Text | Strict UTF-8 and the existing 341-codepoint font. Unicode codepoints, byte offsets and graphemes are distinct. Unknown glyphs are explicit unsupported results; images, interactive links, embedded styles, ruby and general international font coverage are outside this text profile. |

The source must remain immutable while selected. Complete-content hashing guards
persisted identity and wake selection; in-place edits during a read are not a
supported workflow. Cold admission reads every stream and is repeated on cold
opens, trading latency for an explicit work bound. No writable EPUB cache exists.
SDK Expat allocations are outside the two arenas but have a separate hard
32 KiB aggregate requested-byte ceiling, including aligned ownership headers.
Long attributes/comments and deep nesting stop on allocation refusal and release
all ownership. libc metadata/fragmentation and CPU latency remain outside this
ceiling; streaming member and scheduling bounds do not prove a time limit.

## Evidence and budget

The host gate exercises ASan/UBSan, short reads, lying ZIP lengths, expansion
limits, corrupt deflate, zero-output deflate prefixes, whitespace-only chapters,
UTF-8, per-book persistence, sleep selection and arena cleanup. Local adversarial
rounds and targeted rereviews found and closed scheduling and malformed
deflate error handling defects. The maintainability review led to shared filename
admission and one navigation owner. This evidence does not establish physical
SD behavior or all chapters of every private book.

The read-only 61-book host probe opened the first readable chapter in 59 books,
returned unsupported for two, had no other failures and restored 57 next-page
anchors. One lacks a required glyph, and one archive contains a member above
4 MiB; none exceeds 64 MiB total
expanded size. The selected migration target independently opened and restored
an anchor. The preserved private proposal's page-start anchor and spine also
restore through the current SampleReader runtime. No private title, path,
digest, passage or saved position is published.

Observed host peaks: metadata 46,526 bytes, layout 48,562, parse 45,824 and
requested Expat allocation bytes 15,288 (including ownership headers). The complete external reader owner allocations
cost 99,424 host bytes, with a largest block of 49,152. Charge the filename
catalog separately: 12,304 host bytes. Their sum with observed XML allocations is
127,016 bytes; allocation headers, SdLibrary ownership, SD buffers, task stacks
and runtime reserve are additional. Separate high-water observations are not a
continuous simultaneous heap trace. The historic Home free-heap measurement is
not a current mounted-reader guarantee. Firmware admission requires both the
largest contiguous block and owner allocation bytes plus the complete 32 KiB
XML ceiling and a separate 32 KiB runtime reserve. The resident catalog is
charged separately.

The four C3 native variants are measured with the actual app image length and
shared instruction/data SRAM occupancy; the current measurements are in
`artifacts/m1-sd-reader-verification.json` and source-bound code-health receipts.
The SDK ZIP directory locator alone reports 4,240 static stack bytes in compiler
output; call-chain, SDK allocator overhead, mounted SD heap, stack high-water and
cold-open latency remain unproven. Compact Home table fields recover flash while
preserving frozen glyphs, rasters and every tested pixel/kerning value.

## Standalone host evidence

`probe-library.py` and `crossink_migration.py` share the verified standalone
build boundary. Each verifies the pinned SDK checkout's commit, official origin
and clean tracked/untracked inputs, then compiles a fresh temporary helper and
SDK objects. Existing named helpers or cached objects cannot supply evidence.
Receipts bind source/build procedure, compiler and executable bytes, checked
again before results are published. Probe aggregate evidence receives the
revision established by that verification; a hard-coded label is insufficient.
Migration receipts remain in the private proposal alongside its private passage.
No private paths, titles, book identity or passage enter aggregate build receipts.

`artifacts/sd-library-probe.json` is specifically SDK metadata and streaming
spine-XML evidence: all 61 metadata opens and 2,859 XML parses succeeded. That
probe bypasses Newt's reader admission and never lays out or renders a page.
It therefore does not establish the runtime result of 59 readable books out of
61, per-book navigation/progress, glyph coverage or device behavior. Its
`evidence_kind` and exact aggregate-only schema preserve this distinction.
The refreshed metadata/XML run observed a 15,832-byte XML allocation peak and
a largest allocation of 4,104 bytes, including Newt ownership headers. These
replace the earlier standalone payload-only numbers for this artifact; they do
not replace the separate runtime-reader measurements above.

The producer and public host gate reject omitted/incompatible build receipts,
SDK/profile drift, private or reader/device fields, and inconsistent counters.
A committed receipt records the historical run's source and executable digests;
it cannot authorize reuse of an executable or claim that a later source change
was probed. The public gate checks its contract without private inputs. A new
probe still builds and verifies a fresh helper before publishing its own receipt.

## Migration boundary

`crossink_migration.py` explicitly selects pinned CrossInk 1.5.1/schema 66 or
1.6.0/schema 77; it does not infer a release from progress bytes. Both support
4/6/10-byte little-endian progress records without a version field. A source
Unicode offset, including whitespace before collapse, is never copied into a
Newt normalized-text anchor. The host proposal requires unique same-spine text
evidence and uses an actual earlier Newt page-start anchor. It remains
`approximate_review_required`, with `automatic_apply: false`.

The preserved owner backup uses an explicitly selected, historically documented
1.5.1 profile and validated schema 66; the schema alone does not prove the
installed release. Its passage proposal
remains private. Cache provenance comes from recent-book metadata/cover paths;
host `std::hash` must never reconstruct the legacy full-SD-path cache key.
Exact within-page conversion, settings mapping and automatic import are not
supported. Corrupt ZIP streams, unknown releases, partial/mismatched schemas,
conflicting progress and ambiguous passages stop with a private generic result.

The host preview validates a bounded EOCD candidate and supplies the same
comment-free, read-only ZIP view to Python and the SDK witness. This prevents
signatures inside comments from selecting a different, unbounded directory.
At most 32 plausible end-record candidates are examined, with central records
streamed before Python's bounded directory allocation.
Original book digests and member offsets/content are preserved; no source file
is rewritten. The physical reader's pinned ZIP locator has not gained this host
view, so signature-bearing comments may still be unsupported there.

## Proposed separately authorized X3 test

1. Have the owner approve one combined, reviewed image and its recovery route.
   The latest UX branch is integrated. RTC v6 retains the 68-byte record and
   v4's independent source discriminator, with a separate 140-byte validated
   filename/full-digest SD record. Old RTC versions fail closed; durable per-book
   progress remains available. NWTP v1 is accepted and v2 writes the 24-byte
   record with refresh cadence at byte 14. Regional gray remains off by default.
2. Reidentify the spare card by the preserved receipt's volume UUID and compare
   all 61 book digests. Historical mount paths are not sufficient evidence of
   current card identity. Keep the original backup and `.crosspoint` untouched.
3. Measure free heap, largest block and task stack high-water before mount,
   after scan, during cold admission/layout and after reader release. Stop if
   allocation admission fails or the watchdog resets. Record cold open, reflow,
   page-turn and input-abort latency, with repeated library/Home transitions.
4. Read the selected target through chapter boundaries; compare the private
   legacy passage with the proposed Newt page and have the owner explicitly
   accept the approximate location. Any subsequent progress installation needs
   its own reviewed path; this host proposal cannot install a locator.
5. Verify separate per-book progress across Home, deep sleep/wake and restart;
   exercise card removal/read failure and changed-book identity. Compare legacy
   state and book digests before/after and demonstrate recovery to CrossInk.

No hardware, serial or card operation is part of this source task. Later X4
support can reuse the parsing/migration boundary while supplying its own display
geometry and hardware evidence; no X4 implementation is included.
