# NEWT-33: idle progress saves

Integration on main `8e882c3`: flash record 56 and protected update 35 follow
NEWT-34 record 55/update 34. Page route revision 4 and cache finalization stay
owned by NEWT-34.

A periodic save requires the existing quiet interval, an empty physical queue,
and no pending DEV bytes or reply. It does not depend on the reader panel or a
valid display receipt: a static menu cannot indefinitely block durable progress.
The single UI task stamps quiet time after the blocking paint completes; a long
render cannot consume that interval, and no paint overlaps a save admission.
If source verification is pending when a checkpoint becomes due, the same
input-preemptible 4096-byte steps run on any reader panel, even with an invalid
display receipt or low/unknown battery. They restore the borrowed native prefix
from the unchanged logical frame without painting or publishing a generation.
Progress still waits for full verification. Before a checkpoint is due,
optional verification keeps its Page/display/power guards; producer admission
keeps its existing Page/display/view guards throughout.

Periodic chapter identity and estimate publication follows the progress core
as one coalesced quiet duty. After a chapter turn, the new identity therefore
arrives through that follow-up. Home can briefly show an unavailable/generic
chapter label; it never substitutes the wrong chapter. Explicit saves and exits
keep synchronous metadata publication.
The optional duty cannot displace an already-held Pages lease: if a turn has
rewarmed it, the debt waits until a normal admitted checkpoint releases it.
This avoids a second cold turn caused solely by deferred metadata. A rejected
optional semantic projection is remembered as unknown within the immutable
session/chapter, rather than rereading it and displacing Pages on every turn.
Only a validated record with rejected geometry is remembered; missing records,
allocation failures and I/O failures remain retryable.
Changing chapter/source or successfully publishing replacement metadata clears
that memo; canonical source, path, CRC and cancellation checks remain.
Only a stop observed before estimate publication retains debt. Input after a
completed write or an already-equal result cannot schedule that work again.

The admission stop callback applies before the first writable begin. A deferred
producer save interrupts production without denying the chapter; an admitted
checkpoint completes both durable syncs and bounded lost-ack readback.
Explicit, accepted-open, Back and Sleep saves stay synchronous.

DEV inclusive save spans retain purpose and deferred markers; cheap input
deferrals do no storage work and emit no span. Exit rows report checkpoint age.
The Lead device script and prior matched USB evidence are described in
`../artifacts/newt33-report.md` and `../artifacts/newt33-r7-report.md`.
Integration resources are recorded below; [the final report](../artifacts/newt33-r8-report.md)
contains validation times and the hashed evidence index.

## Integration resources

Record 56 and protected update 35 preserve main’s complete history. See the
NEWT-33 integration table in [CODE_HEALTH.md](CODE_HEALTH.md) for all four
exact flash/SRAM/RTC measurements and exit-chain changes.

The producer-pause authored stack path (while its parser is live) is
2,480→2,384 bytes in release (-96), and 2,528→2,560 in DEV (+32).
Its `Poll::pause` frame grows 32→112 bytes and `serviceProducerPause`
48→64; their growth is visible in the full compiler report and chain audit.
These are static authored contributions on top of a live parser, not a
whole-task stack bound or a runtime margin claim.

The independent Claude Opus 5.5 xhigh review approved the exact proposal.
The device-script regression is included in quick and full host gates; it
uses a fake serial peer and does not open a device.
