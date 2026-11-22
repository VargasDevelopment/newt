# Sequential reads and the failed bounded-open journey

The coordinator-approved image `191c0795d401-regional-experiment` was installed
once, with full-flash readback matching the current baseline except the approved
306 application sectors. Both planned boot observations passed the startup
guard. The first startup's low 64 KiB matched the fresh pre-write snapshot.
Current reading progress was preserved; no historical progress was restored.

The subsequent fixed 23-stage journey failed after ten validated stages. Its
eleventh observation is the failed sibling-open stage, not a completed stage.
The final activation delivered ACCEPTED in 10.3 ms and CONSUMED in 11.9 ms;
RENDER_RETURN followed at 21.435 s. The opening trace reached cancellation
reason 2 at 20.088 s and returned a ready Library screen with the reader closed.
The connection closed without retry or subsequent hardware input.

Selection and full-content hashing consumed 14.177 s; only about 4.516 s
remained inside the existing 20-second opening budget. The previous attempt's
unacknowledged 30-second activation remains unresolved. This attempt proves
command delivery and cooperative cancellation, not successful sibling reading.

The passing primary prefix opened the current saved anchor 1:238, advanced to
1:578 and 1:963, returned to 1:578, and reopened exactly at 1:578. All 247
captured response CRCs matched the audit in order. No in-session panic or reset
was captured. There were 68 loop-stack observations, with 6,708 bytes minimum
free from the 16,384-byte task, including an observation after primary reopen
painted. These are partial journey measurements, not all-input stack bounds.
Private receipts, wire captures, book identities and flash copies stay ignored.

## Source correction

`FileBookSource::readAt` previously called seek before every read, including
contiguous 1 KiB reads during full-content hashing. The pinned local Arduino
implementation routes position to ftell, seek to fseek, and read to fread.
The adapter now checks the live file cursor and seeks only if it differs from
the requested offset. It retains no cursor, raster, book bytes or extra buffer.
An owner's reposition between calls is observed rather than guessed. A position
error sentinel cannot match the bounded valid offset and falls through to the
existing checked seek. The caller owns the cursor exclusively during readAt;
the separate input task may signal cancellation but may not use this handle.

Cancellation is checked before and after position lookup, after optional seek,
and after reading. Full-source SHA, source-size checks, the 4 KiB read cap,
positive short-read handling, catalog release before reader allocation, parser
admission, storage routes and both opening/host deadlines remain intact.
There is already one full hash per activation; no identity pass is removed.

Focused tests exercise sequential exact bytes, backward/random access, owner
reposition, short-read continuation, failed seeks, position error sentinels,
rebinds, cancellation during cursor lookup/seek, and the existing boundaries.
Real stdio-backed authored EPUB lifecycle tests verify complete digest reads
without redundant seeks, full-SHA mismatch rejection, catalog lifetime and
rollback in both orientations. Existing real compressed-reader admission and
sleep cancellation tests retain their semantics with live cursor test doubles.

Avoiding a redundant seek is a source-level fact. A physical timing benefit,
successful sibling opening, full rejection/recovery/settings journey, optical
buttons, battery sleep/wake and owner trial remain unverified. This isolated
candidate has no installation authorization. Its exact built image and reviewed
resource receipts must precede any new coordinated hardware proposal.

## Reviewed flash cost

Four clean measurements of source `82b5a9ca2b074af1bcd24408a887006a6f2bed18`
exceeded the preceding flash ceilings only. Two independent reviews verified
all 334 output hashes per variant, actual ELF/map/image/stack measurements and
the exact flash-only baseline proposal. RAM sections and padding, RTC 300 bytes,
all 24 selected frames and the adapter's 32-byte frame stayed unchanged.
Linked text adds 42 bytes (four in readAt and 38 for File::position); unwind
metadata adds 44. Image packing yields 80 additional bytes in default and DEV,
96 in regional and regional DEV. Exact flash ceilings are therefore 1,237,600,
1,248,896, 1,241,424 and 1,252,752 bytes, respectively, with no extra headroom.

Only these four flash ceilings and their source/input provenance are adopted.
Policy, line allowances, dependency adapters, exclusions, SDK pin, RAM/stack
ceilings, parser reserves and the strict DEV budget remain unchanged. The first
regional attempt rejected changing inputs during concurrent host work and was
discarded. Early default/DEV procedure-cache bindings became stale during the
host suite; their archived measurements are cost evidence only. Policy checks
and four fresh post-adoption clean builds must precede final native acceptance.
