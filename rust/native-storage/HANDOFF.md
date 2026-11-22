# Native FilePort policy contract

This crate implements Newt's `FilePort` and `Route` storage policy. It is safe
`no_std` Rust with an explicit `Native` syscall boundary and fallible optional
alloc. Provenance records stay under `docs/provenance.md`.

FilePort owns one native backend value, one descriptor and only its own
O_EXCL-created provisional. Route owns bounded root/key storage and shares a
read allowance across ports, retries and reopen. Ordinary path construction
uses one fixed 256-byte buffer. Backup/transaction names and the outer read
budget reserve backing fallibly, then release it when unused. Optional Vec
metadata remains; no firmware allocator or resource acceptance is implied.

Operation borrows persistent port state for a short call/attempt. A producer
policy belongs to its caller, retains failure/observed-cancellation facts and
outlives that attempt's explicit cleanup. Release while the policy is attached,
inspect its result, then end the view. Operation drop ends the loan only;
FilePort drop performs best-effort cleanup. An acknowledgement needs the
operation's checked release result; drop alone is not one.

The supported domain is one trusted application-owned tree, one task/writer,
UTF-8 paths without NUL, native u32 transfer extents, balanced checked scopes
and no mutable port reentry. Cancellation/invalid state may change during a
native call and is observed at its next boundary. A returned successful open
or rename still owns its effect, even if cancellation then arrives. Callback-
driven owner replacement is outside this domain.

Reads/writes retry only EINTR, at most four attempts per uninterrupted failure
sequence, resetting after positive progress. Shorts advance actual bytes;
zero/oversized returns fail. Descriptor/path type, extent, timestamp and nonzero
identity hints remain separate from cryptographic source identity. Missing
canonical files may use backup; damaged canonical data never authorizes that
fallback. Deferred immutable exits check the path before publication.

Release detaches ownership before cleanup, revokes tokens, advances generation
and exposes cleanliness. Promotion preserves complete canonical output across
reported failure. Producer destination checks, backup handling, sync, retain,
suspend/resume and readback remain the pinned policies. This crate adds no
parent-directory fsync promise and makes no new SD durability claim.

The standalone C++ port-comparison packet was retired. Source-hints and outer
reader authority are not part of this adoption.
RV32 library compilation is separate from concrete native adapter/allocator
retention, linking, stack/heap/flash/RTC budgets and physical SD/device evidence.
No memory floor or ceiling changes are authorized by this crate.
