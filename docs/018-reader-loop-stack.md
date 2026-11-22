# Reader loop task capacity

The explicit alignment candidate `566c5c3469fc-regional-experiment` passed
complete flash preservation and two clean boots. Its real Martian first open
then triggered a captured stack-protection panic in `loopTask`. The fixed
journey stopped and closed Serial without retry or recovery inputs. Alignment
remains a valid ownership fix, but that image is not a stable reading baseline.

The exact installed ELF returns 8192 from Arduino's weak loop-stack provider.
Captured code addresses connect ZIP tail discovery through scheduled/file
reads, SD seek and newlib locking to the faulting FreeRTOS semaphore call.
ZIP discovery retains a 4240-byte frame, including a 4099-byte window. Native
frame inspection totals 8496 bytes on a reachable opening path before read
helpers and filesystem locks; first-page restoration retains at least 9040
bytes including those helpers, still before filesystem/newlib frames. These
are inspected call-path sums, not a complete all-input stack bound. The saved
panic excerpt does not exclude every possible memory-corruption mechanism.

`reader_task_stack.h` uses the pinned Arduino API to provide a strong 16 KiB
loop-task stack. The SDK sources, compiler flags, XML ceiling, runtime reserve,
reader arenas and existing heap admission guards stay intact. Static SRAM and
RTC are separate from the **8192 additional dynamic task bytes**. Applying
that cost to the failed admission measurement predicts free heap 134536,
7945 above the existing required 126591. Actual free heap and largest block
must be measured; allocation placement is not inferred from static receipts.

All four native guard gates require the strong provider, its exact 16384-byte
return and actual `app_main` register flow into the `loopTask` creation call.
Fault controls reject the weak/default provider, wrong size, literal fallback,
argument clobbers, wrong task, missing calls and control-flow bypasses. The
previously failed real image is also a native negative control. Compiler-frame
ratchets remain enforced; individual frames are not runtime call-chain bounds.

DEV USB records the loop-task minimum remaining stack, in the pinned ESP-IDF
byte units, after completed Library/reader paints. One pending byte defers its
output until protocol records and button requests finish; it retains no stack
value, wire field, buffer or raster. The byte is charged to the named DEV RAM
bound and measured linker occupancy. A bounded private raw receive log
captures these observations alongside existing protocol evidence. Successful
open, turns, exact saved-page reopen, rejection/recovery and stack minima are
required from the next authorized device journey. Settings in both
orientations, sleep/wake, optical behavior and owner acceptance remain distinct
checks. No efficiency or versioning work is included.

The independently audited four-variant cost builds use the same compiled C++
and configuration scopes as source `e230c104d0352141233c2707a33240f0b6404aa5`.
Normal/regional application lengths stay 1237104/1240928 bytes. DEV lengths
are 1247920/1251760 bytes, exactly 160 additional bytes each for deferred
stack observations. This is the only flash-ceiling exception. All four static
SRAM totals, 300 RTC bytes and 24 selected compiler frames remain unchanged.
The one-byte pending observer is absorbed by existing BSS packing and charged
to the named DEV assertion: 1017 < 1024. The additional dynamic loop-task
allocation remains a separate 8192-byte runtime cost. Preliminary procedure
identities are not final freshness evidence; the complete host gate and four
fresh clean native builds are required after freezing this source checkpoint.
