# Battery-only wake evidence proposal

Battery-only resume has no proven root cause. The owner repeatedly observed
USB-connected success and battery-only return to Home, with no agent attachment
during the failures. At 17:45 UTC the owner repeated the exact sequence on installed
f6: Read to text, sleep to the correct CCW frosted reading scene, then wake. USB
returned the same text; battery returned Home. Both sleep scenes were correct.
Treat this as the acceptance fixture: the observed divergence follows correct
sleep-destination rendering. Prioritize wake/reset/restore or subsequent input;
do not ask the owner to keep repeating it before instrumentation.

Source and ELF audits establish RTC FAST retention and no
USB-specific resume-policy branch; they do not establish the device's actual
reset cause, retained-record validity, reopen result or subsequent input.

This worktree implements the bounded DEV-only RAM receipt described below.
It authorizes no worker USB opening, enumeration, reset, input, sleep, flash,
SD access or persistent logging. At 19:36 UTC the owner made hardware available;
parent approval of an exact diagnostic image, installation and the coordinated
comparison remain pending. Installed f6 has not been replaced by this work.

## Minimal receipt

Capture the battery wake locally before any host connection. The receipt is created
in the awake boot and remains available at Home until another reset; USB opening
is neither needed nor part of collection. A stable failed wake therefore preserves
its evidence while the owner opens Your X3.

Use fixed primitive RAM storage for a boot receipt and show a compact, read-only
Wake details view on Your X3 **alongside** the storage warning. Preserve normal
wake routing and first-screen rendering. Do not redirect a failed wake to a
diagnostic screen or attach USB to obtain the evidence.

[Early snapshot](../firmware/sleep_lifecycle.h) captures reset cause and retained
bytes before consuming RTC magic. [SleepSession](../firmware/sleep_session.h)
owns one [80-byte receipt](../include/newt/wake_evidence.h), replacing the prior
DEV diagnostic backing state. The existing USB schema still reports its live
reader fields; it is not required for this local receipt. [Your X3](../src/ui_screens.cpp)
shows Wake details and the storage warning together, using the existing portrait
system shell after either saved reader orientation. Null/unavailable evidence
uses the existing device view. About, wake routing and first-screen selection
remain unchanged.

The initial result is copied inside the actual reader-open boundary, before an
invalid-checkpoint settings reset can erase its typed failure. Largest-block and
free-heap guards retain their existing short-circuit query counts. Completed
display receipts latch the first actual screen/generation; declined paints do
not. Consumed physical input and the first Reading-to-Home transition fill
separate slots, including the latter's physical or synthetic USB source. No
additional ADC reads, heap allocations, RTC fields, NVS/SD logging or USB
collection are introduced.

The focused sanitized [wake harness](../tests/wake_evidence_test.py) compiles
verbatim firmware open, paint, button mapping and consumed-action blocks with
real host readers. It checks invalid records, both heap guards and query counts,
actual parser allocation failure, exact and validated-anchor restore policies,
immutable initial results after retry/settings/close, declined versus completed
paints, separate physical/synthetic event slots, both saved orientations,
maximum locator/time formatting and warning coexistence. These are host facts,
not a diagnosed battery failure or hardware acceptance. The receipt is 80 bytes;
host SleepSession is 88 bytes versus the prior 72. Native SRAM/flash/stack costs
have completed measured review and exact limit adoption.

The exact reviewed and adopted structural exception is `firmware/main.cpp`
546 to 556 lines: three completed-paint hooks and seven lines surrounding
dispatch capture the actual screen, reader-open state, event source and time.
The exact main546→556 observation exception and measured DEV-only resource costs
are independently reviewed and adopted: DEV flash +3,584 / regional DEV +3,600,
SRAM +16 each and loop/setup +16 each. Ordinary and other selected limits remain
unchanged. Full host and all four current resource/guard receipts PASS for compiled
source `829f8c834568c351a99350483c9f90e24bc15f5b`. See the [exact candidate manifest](../artifacts/m1-wake-evidence-builds.json)
and [handoff](m1-ux-handoff.md); installation and physical cause collection remain
pending parent coordination. Native static metrics do not establish runtime peaks.

| Capture boundary | Bounded evidence |
| --- | --- |
| Early boot snapshot / restore | Reset reason, wake cause, eligibility, record validation status, requested and restored destinations, rejected-entry tag, checkpoint-save acknowledgement. Treat invalid-record target/locator fields as untrusted. |
| Initial reader-open attempt | Not requested / largest-block guard / free-heap guard / open failed / succeeded; actual restore policy, reader status, typed restore failure, expected/actual locator and saved orientation. Freeze before retry, Settings or close. |
| First completed paint | Actual screen and display generation; leave explicitly unavailable if no paint completed. |
| First consumed physical event | Hardware button, mapped action, time since boot, screens before/after dispatch. Fill once. |
| First Reading-to-Home transition | Input source, button/action, time and reader-open state before dispatch. Record separately because the first event may be harmless. |

Boot fields stay immutable until the next boot. Event slots fill once and are not
erased by navigation to Your X3. Use bounded text formatting and existing readable
UI typography. Measure the actual compiled SRAM/flash/stack costs and verify both
orientations, storage-warning coexistence and record immutability before proposing
an exact diagnostic image. Preserve the NVS startup guard, one-shot eligibility,
68-byte RTC sleep contract and separately owned SD state.

## One coordinated comparison

Only after the parent approves an exact diagnostic build and any necessary
installation, the owner performs one matched pair at the same recognizable
noninitial passage, page and orientation:

1. With USB connected but no host serial session opened, photograph the passage,
   sleep and wake, then photograph the first awake screen before other input.
   Open Your X3 and photograph Wake details with any storage warning.
2. Return to the same passage. Disconnect USB **before preparing sleep**, then
   repeat on battery. Keep USB disconnected until all evidence is captured.
3. Record every owner button press after wake. Repeat the other orientation only
   when needed to separate an orientation-specific result.

Photographs and book content remain private; no raw backup, private book or device
identity is committed or uploaded. Stop the comparison after the evidence pair;
do not silently retry, restore, reflash or open a diagnostic USB session.

## What the receipt distinguishes

| Observation | Supported conclusion |
| --- | --- |
| Ineligible reset | Startup took the cold/reset route. |
| Eligible reset, record validation rejected | Retained-record validation prevented restoration. |
| Valid Book request, initial open failed | The book reopen boundary failed; the frozen reason separates guards from reader failure. |
| Successful exact open and first Reading paint, then recorded Back to Home | A later consumed event caused the transition. |
| Valid Home request | Home was prepared as the destination; this alone does not explain why. |

A recorded pre-owner Back cannot distinguish ADC noise from a real press. If that
case appears, propose a bounded capture of the samples **actually consumed by
polling**. Calling the SDK's separate `readButtonAdc()` would perform additional
reads and would not faithfully record the decision samples.

RAM is sufficient when the failed wake stays running until photographed. Another
reset overwrites it. If that prevents collection, propose a separate checksummed,
bounded RTC diagnostic history without repurposing the sleep record. RTC history
still does not guarantee survival through power loss. Persistent flash logging
would require a separate reviewed preservation design; it is not this proposal.
