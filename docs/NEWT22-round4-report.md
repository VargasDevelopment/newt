# NEWT-22 round 4 integration

Merge `37c14e6a3bff501727231806a2fe1dcaa1c88fff` integrates pinned main
`7cc92ab2fb3b7236329dbf96237a80340b232ba0` into `44453bc`, preserving both
parents with the requested Conventional subject and coauthor. No history was
rewritten. No push, network acquisition, device access or flashing was performed.

The footer resolves both branches' behavior: the explicit Page uncertainty bit
replaces the center chapter field with `PG SAVE ERR`; any other visible storage
diagnostic uses `STORAGE`. Normal overall percent and the page fraction remain
independent. Main's plain percent, label folding, 14 px floor and ellipsis are
retained. Tests from both parents remain, with an additional unspellable-label
fallback test: `Section N` cannot override the warning. Pixel equality against
independent 16 px glyphs proves the complete 103 px warning is unellipsized in
both 140 px portrait and 200 px landscape fields, even without a storage pointer.
The same renderer writes six authored frames under `/var/tmp/newt22-evidence/`
and archived copies under `artifacts/newt22-r4-footer/`.

The audit preserves main records 1–41 byte-equivalently, renumbers the historical
NEWT-22 measurements as 42/43, and adds exactly one integrated record, 44.
Record 42's predecessor is main 41; its measured after/source/evidence/reviewer
remain the original unintegrated measurement. This bookkeeping adjustment is
explicit in its reason. All protected history, prior tighter RAM/stack values,
selected entries, exclusions, adapters and policy pins are unchanged.
Both resource explanations retain their original review and source provenance.
The twelve live reports/identity/linked-guard receipts are regenerated, rather
than treating either parent's receipts as integrated measurements.

The exact four-lane sizes and symbol explanation are in the integration entry of
[CODE_HEALTH.md](CODE_HEALTH.md#newt-2223-integration-record-44-2026-10-20) and
`artifacts/newt22-r4-symbol-delta.json`. Each raw report hash in record 44 covers
the decompressed archived bytes. A clean pinned-main comparison reproduces all
four record-41 images. RAM and RTC remain unchanged; every selected frame fits
its existing ceiling. The Lead's independent integration review remains pending.

Validation below measures the integrated production source; subsequent commits contain only audit metadata, documentation and evidence.

All four final clean native lanes PASS on
`7cb5567` using `firmware-build.py --clean --health-report` and the appropriate
DEV/regional flags. Exact sizes match record 44. Clean identity receipts bind
that source; all sixteen live core hashes and all compiler-input hashes verify.
There are no selected-frame actual changes from `44453bc`. Complete per-entry
actuals/ceilings and receipt hashes are in
`artifacts/newt22-r4-resource-validation.json`.

Other-storage cases remain destination-only uncertainty, resume authority
unavailable/corrupt, selected-book state unverified/unavailable, missing/changed
SD source, reader-session admission failure, NVS version/recovery/unavailability,
and DEV Home-preference restore/commit diagnostics, including
`sdSelection.storageProblem()`. Each uses `STORAGE` unless an independently set
Page bit takes precedence. Detailed notice text is retained on existing surfaces.

The focused reader-fault gate PASSes (60.897 s including common prerequisites).
Two additional retained-state mutants fail their intended runtime predicates:
copying RTC sample live state into acknowledgement, and discarding retained
uncertainty from a pending Library Sleep. The complete Linux gate also reruns
all built-in cold-start, descriptor, Settings/sample and transaction controls.
An early quick invocation was deliberately interrupted before its shared matrix
work; its replay is the passing result reported below. The first supplemental
retention-control harness observed the intended rejection but expected an older
message; correcting only that expected message produces the passing control log.
No production or test-oracle relaxation was made.


The quick Linux gate PASSes 157 phases in 279.414 s. The complete Linux gate
PASSes 260 phases in 841.145 s on `1164dce`, including ASan/UBSan, all 20 reviewed
renderer frames and all built-in negative controls. Full every-page parity
PASSes 18 fixtures and 13,408 comparisons in 521.367 s. The fresh full turn-cost
matrix PASSes 24 heterogeneous fixtures, 3,456 observations, 1,152 exact
cold/produced pairs and 1,152 cells. Host timings are informational, not physical
latency. Complete raw matrix/parity data and logs are archived as
`artifacts/newt22-r4-*`; their compact index is
`artifacts/newt22-r4-validation.json`.

The pinned release gate PASSes both
`fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d..1164dce` and
`7cc92ab..1164dce`. The final documentation/evidence-only commit is checked again
at handoff. The twelve live receipts remain bound to clean `7cb5567`; their
compiler inputs match record 44's clean source `37c14e6`. Production, tests and
build procedures have not changed after that merge. No second integrated flash
record, resource-ceiling change, review exception or history rewrite was used.

The isolated pinned-main measurement worktree and this round's scratch directory
are removed at handoff. Requested `/var/tmp/newt22-evidence/` is retained, with
six PBM/PNG pairs and a hash manifest; committed copies provide durable evidence.
The Lead's independent review is next. Physical USB-powered sleep/wake,
unplugged battery sleep/wake and cold power-on each remain pending with separate
reset/power evidence; exact-source Mac ImageIO coverage is also pending. This
handoff claims no hardware behavior or owner-acceptance readiness.
