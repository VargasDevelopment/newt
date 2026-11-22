# Partial reading evidence and retained failures

The installed `e29803be2108-regional-experiment` candidate completed ten of the
required 23 reading stages on the owner's X3. The primary book opened, moved
forward twice and backward once, exited, and reopened at the exact saved anchor.
Sibling activation then produced no acknowledgement within the unchanged
30-second command deadline. The connection closed without retry. The candidate
is not accepted for the trial freeze.

The exact image and sanitized results are in
`artifacts/m1-reading-partial-evidence.json`. Independent review checked all 242
captured CRC-valid replies against the audit and verified the completed stage
prefix. The 16 KiB loop task had 6,648 bytes of minimum observed free stack across
the completed primary paths; this does not certify the unresolved sibling open.
No panic or in-session reset appears in the capture. Its one startup banner
precedes both HELLO replies, consistent with the transport's documented opening
behavior. The failed receipt remains failed.

Before boot, the complete 16 MiB readback matched the expected image replacement
in exactly 306 app1 sectors, with every outside byte preserved, including the
diagnosed historical coredump. Low flash, including NVS, was unchanged after the
first startup. Two bounded boots passed. These preservation proofs precede the
reading journey; they do not claim that legitimate progress saves left NVS
unchanged during reading.

The firmware buffers `ACCEPTED`, then immediately consumes the queued action
and opens synchronously before its next bridge poll sends that acknowledgement.
Selection hashes the whole book before reader admission. Silence therefore
does not establish inactivity or identify the blocked phase. In addition, the
host records a request before writing and previously discarded the chained
exception on failure. The old capture cannot distinguish failed delivery,
response timeout, or blocked device work.

The host change retains private failure context: stage, command, phase,
exception type, bounded message, and one bounded chained cause. For uncertain
PRESS completion it also records whether the complete host write returned and
how many replies validated. A successful host write does not prove device
consumption; an incomplete write may still have effects. Closure failures are
retained separately so they cannot hide the original failure. Opening failures
also retain their error detail.

Complete receipts retain their existing schema. Failed receipts cannot pass
acceptance. The fixed stages, audit sequence, request and overall deadlines,
one-connection rule, no-retry policy, and hardware authorization are unchanged.
The new diagnostics are source-only and have not been installed. Private books,
device identifiers, raw serial, flash dumps, and NVS remain outside Git.

Validation: all 22 reading-acceptance tests pass, including production client
write-failure/response-timeout cases and failure/closure preservation. The full
`python3 -B tools/check.py` host gate passed with ASan/UBSan, exact previews,
both orientations, cancellation/recovery, and NVS fault tests. Independent
adversarial and maintainability reviews are clean. No firmware source changed
in this diagnostics patch, and no new firmware image was built or installed.

The next firmware proposal should send acceptance before synchronous dispatch
while retaining consumption and actual render completion as distinct facts.
It needs bounded backpressure and stale-ticket/physical-takeover tests, opening
phase evidence, and cancellation review. Early acknowledgement alone cannot
make a book open that exceeds the terminal deadline pass. Another hardware
operation requires the coordinator to resolve the current uncertain state and
review the exact next candidate and procedure.
