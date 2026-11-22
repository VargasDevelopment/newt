# M1 directory-window install stopped before write

This historical first-attempt stop was followed by one parent-authorized ROM
recovery and a newly reviewed successful install/test. See
`032-directory-window-physical.md` for the current result and authoritative
reading positions. The failed attempt and its evidence remain unchanged.

The parent-approved attempt for `57ba55228181-regional-experiment` stopped
before any flash write. The fresh `get-security-info` connection failed during
its initial no-reset SYNC receive with `FatalError: No serial data received.`
The reviewed fail-fast guard closed the connection and prohibited retry,
fallback and cleanup traffic. No further hardware commands were issued.

The exact candidate remains 1,258,736 bytes, SHA256
`e39bcbd491853aedee03bc2d6c77f2e15904cb5d01b97d9badc61585d437111d`,
compiled from `57ba552281813d6979b71c9714b6a715537b800d` and reviewed at
`b67e2c3a51d774f160c9353502846477600f85d0`. Its approved proposed span was
308 sectors, `[0x780000,0x8b4000)`, with 2,832 FF padding bytes.

## Evidence completed before the stopped attempt

The complete host gate and all four clean native gates passed. Two independent
final proof/action reviews passed, and the offline installer check exited 0.
Fresh duplicate 16 MiB captures match; two independent 308-sector recovery
slices are durable. Every NVS byte is reconstructed with zero unknown changes
and no GC. All non-NVS bytes match the previous diagnostic readback. All 69 NVS
negative controls pass.

The fresh durable primary anchor is spine 1, character **1708**. Its two progress
records are generations 9/10, active counters 126756/137708 ms, forward pages 5
and qualified sessions 0. These fresh records remain authoritative; older
snapshots must not replace them.

## Exact stop boundary

The install attempt successfully confirmed the original target identity and
16 MiB flash. The next security-info SYNC received no serial data. The helper
exited 1 before the third full read, fresh write preflight or write-attempt
marker. No flash write, candidate readback, boot test or reading journey ran.
The settled guard did not retry or send cleanup traffic.
Two independent offline failure audits confirmed this stop boundary and the
intact backups and recovery slices.

The last confirmed device state was bootloader/flasher stub after the flash-ID
command. Current hardware state has not been re-observed. The previous
`4a31ec31750b-regional-experiment` image has not been replaced. The serial
connection closed; no reset or boot followed the failure. The logs establish
the receive failure, not its underlying cause.

## Remaining gate

The physical effect against the sibling's **10.700336-second** ZIP-inspection
baseline is **unmeasured**. Candidate reading is **not run**; the previous
diagnostic journey still failed. No stability or M1 acceptance claim follows
from the source or preservation proof, and no trial freeze has started.

Parent-coordinated recovery/re-entry and a newly reviewed exact attempt are
required before continuing. That scope must retain current progress, fresh
preservation checks, one minimal write, full readback and the unchanged
23-stage journey with 20/30-second budgets. The failed attempt cannot be rerun.

Numeric evidence is in `artifacts/m1-directory-window-install-stop.json`.
Raw serial logs, complete flash captures and recovery slices remain private and
outside Git. No card transfer, private-book upload or sleep input occurred.
