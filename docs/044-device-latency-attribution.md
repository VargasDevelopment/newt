# Existing device latency attribution

This reanalysis adds no device actions or instrumentation. It matches the existing
20 navigation PRESS acknowledgement windows and three complete reader-opening
profiles from the installed `29fffd243c63` journey. Sanitized numeric evidence is
`artifacts/m1-reader-latency-attribution.json`; private raw records and protocol
identities remain private.

`artifacts/m1-device-latency-contract.json` supplies the CI handoff: endpoint
definitions, exact compiled image/source, helper hashes, required private
device/install bindings, outcomes, refresh classification and all six sampled
navigation groups. The existing scalar baseline is the host monotonic wrapper
around `client.request(PRESS)`, ending after its three validated replies and
excluding subsequent STATUS/DIAG. The audit also supplies the narrower timestamp
immediately before request write through decoding the matching terminal response;
these distinct endpoints must not be mixed by the ratchet. Replies must match
boot/session/request ID and be ordered ACCEPTED, CONSUMED, then RENDER_RETURN or
NO_VISUAL_CHANGE. Failed correctness actions never become passing timing samples.

Group completed samples by action and observed refresh class; nearest-rank
`ceil(p*n)` p50/p95/max retain sample count without interpolation. Missing/mixed
driver tags remain unknown. Current sampled ceilings and the owner's
500/750/1000 ms targets are separate: this is a software/serial completion
baseline, not an optical, ghosting or battery acceptance claim.

| Action | n | STATUS refresh ms | Reported controller waits ms | Refresh residual ms |
| --- | ---: | ---: | ---: | ---: |
| Home island focus | 9 | 504 first, then 543 | 388 first, then 426–427 | 116–117 |
| Home full cleanup focus | 1 | 1261 | 942 | 319 |
| Library entry | 1 | 1649 | 1084 | 565 |
| Library focus | 8 | 811–823 | 389–390 | 422–433 |
| Return Home | 1 | 1222 | 903 | 319 |

Existing EpdBus BUSY-wait elapsed times explain roughly 77–79% of Home island
refresh and 47–48% of Library focus refresh. The latter leaves 422–433 ms in
combined cover I/O, composition, conversion, SPI/old-plane synchronization and
other driver work. That residual cannot identify cover or CPU-render dominance.
Same-group and new-group Library focus have similar observed completion times;
both still render and attempt to read all four covers. These waits do not measure
optical settling or ghosting, and do not refine missing mono waveform tags.

The initial saved Read took 18,819 ms: source hash 5,985 ms, admission 9,461 ms,
metadata 651 ms and layout 154 ms. The two reopens took 12,699 and 12,700 ms:
admission 9,472/9,479 ms, metadata 656/650 ms, layout 154 ms, source hash zero.
Reopening the existing selected source handle avoids another digest; actual
source reopen still verifies full SHA. Admission accounts for about 74.6% of
repeat opening, while layout accounts for about 1.2%. Nested admission/stream,
image and I/O profiles overlap and must not be summed as independent costs.

Reader opening performance therefore needs the reviewed authenticated admission
or imported-revision reuse slice. Its owning commit/bundle and review evidence
have not been handed off into the inspected local refs. Raw SDK BookCatalog or
PageCache facilities are insufficient substitutes. The integration plan is:

1. Obtain the parent's exact reviewed checkpoint and validate its source identity,
   invalidation and resource evidence against the current recovery boundaries.
2. Integrate in an isolated owner worktree after current correctness checks.
   Retain full-digest selection, cancellation/release-before-open/sleep and
   publication/NWBR ordering. Reject stale font/layout cache generations across
   the spacing change in source 29ff.
3. Run the combined sanitizer and four clean native resource gates, then review
   the exact resulting diff and image. Preserve all current admission reserves.
4. Propose the exact image for parent installation coordination. Measure the same
   saved Read/reopen correctness actions and existing phase endpoints afterward.

No reintegration is needed for the reviewed Home/one-pass-cover optimization:
its three production files are byte-identical in the installed source, although
the original `ee60b707` commit itself is not an ancestor. The reviewed in-session
page/chapter reuse is already included through `f22f72044`; it does not remove
whole-book admission on reopen.

Optional automatic-cover generation is a separate reviewed source port. Its
existing four native measurements exclude the producer, so combined firmware,
idle ownership/cancellation, VFS/heap and physical-card costs still need owner
integration proof. It supplies artwork rather than the reader opening fast path.
It should follow correctness and measured reading performance work.
