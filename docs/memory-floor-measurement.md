# Measuring the need-based memory floor

Plan for the Lead (NEWT-29). It sets no floor. The current floors stay until
the owner approves a measured one (docs/CODE_HEALTH.md, "Memory policy").
Host tests and receipts cannot supply any of these numbers; each is a device
fact for the exact image and power condition recorded with it.

## What a floor means

Heap floor = the largest allocation the runtime may still request while
reading + a stated safety margin. The device passes when, in every worst-case
journey, the lowest free heap stays at or above the floor and the lowest
largest free block stays at or above the largest request. A stack floor is the
same idea for free loop-task stack.

## Telemetry that exists (DEV USB images)

- `Runtime:` line after completed reader, Library (both `kind=reader`) and
  Stage Home paints: `stack-size`, `stack-min-free` (loop-task high-water since
  boot) and `heap-min-free` (`ESP.getMinFreeHeap()`, the low point since boot).
  It names the paint, not the phase that reached the low point.
- `DIAG` reply: current `heap`, current `largest` (`ESP.getMaxAllocHeap()`) and
  `opening`. `STATUS`: current `heap`.
- `COVER` record per generation: `min_free_local` (IDF local minimum monitor
  over that generation only), `min_largest_observed` (sampled at call
  boundaries, not a watermark) and the reporting task's `stack_min_free`.
- Boot line: free heap and largest block once at startup.

Known planned request sizes come from source, not the device:
`SampleReader::largestExternalAllocationBytes` and
`largestExternalOverviewAllocationBytes`, `kXmlBudgetBytes` (32 KiB) and
`kCoverPlannedPeakBytes`. The 64 KiB `kCoverHeapReserveBytes`
(`firmware/cover_memory.h`) is a reserve cover work leaves free, not a request
size, and the gate's floor records do not bind it.

## What is missing (DEV-only follow-ups, not in this change)

1. **Heap low point per phase.** Outside cover generation only the whole-boot
   low point exists, so one bad moment hides every later phase. Start an IDF
   local minimum monitor at the start of reader open, chapter crossing, durable
   save, Library open and Home overview, and report `min_free_local` with the
   phase name when the phase ends.
2. **Largest free block low point per phase.** IDF has no largest-block
   watermark. Sample `heap_caps_get_largest_free_block` at every guarded
   allocation in the phase and report the minimum with the phase name.
3. **Actual largest request.** Report the size of each guarded allocation
   request (reader arena, XML budget, overview, cover) once per phase, so need
   is measured on the device, not inferred from planned sizes.
4. **Sleep-path stack peak.** Still unmeasured. Report the loop-task
   high-water just before deep sleep, delivered on the next boot.

## Worst-case journeys to drive

Choose books by structure (expanded chapter size, styling, cover presence),
never by title or file. Record power condition and reset cause for each run;
USB power and battery are separate facts.

1. Cold open of the largest available chapter, cache empty.
2. Chapter crossing in that book, forward and back.
3. A page turn that starts a durable save, then the next turn while it runs.
4. Library open on a full shelf while a cover is being produced.
5. Home overview preparation for a book with estimated progress.
6. All together: cover production active, Library opened, then return and
   cross a chapter.

Repeat each journey at least three times on one boot and once after a cold
power-on, and keep the raw logs, failed runs included.

## Choosing the margin

Margin covers allocator overhead, fragmentation and paths the journeys missed.
Propose it as a number with its reason, for example the larger of a fixed
amount and a fraction of the largest request, and check it against the gap the
device shows between lowest free heap and lowest largest block. The owner
approves the final floor; it is then recorded in the baseline's `floors`, with
a record whose `owner_approval` names the approval. Never pick a margin that merely
makes today's image pass.
