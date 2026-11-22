# Installed reading admission

The invariant is concrete: a supported reading-ready result requires a fresh,
terminal journey through the actual installed reader, plus reviewed renderer
pixels. Software checks and file-copy/readback integrity cannot satisfy it.
`tools/reading-ready.py` is the supported machine-derived handoff command.
It cannot constrain arbitrary assistant prose, nor authenticate an operator who
fabricates JSON. Installation/readback metadata remains trusted private evidence
from the single authorized hardware owner; no signing/service infrastructure is
introduced.

## Commands and states

```sh
python3 -B tools/reading-ready.py software-verified
python3 -B tools/reading-ready.py installable --variant regional-dev-usb
```

Both run the existing host gate once. Installable additionally verifies the
existing clean, current native resource receipt and actual linked artifacts.
The optional `--software-proof-shadow` records available before/after software
inputs while still running that full gate every time. Operational reuse remains
disabled because complete execution-input closure and prior-success provenance
are not established. See [the shadow scope and limits](software-proof-shadow.md).
It permits the separately authorized candidate flash **before** hardware testing.
It does not confer hardware acceptance. No extra native builds or budget changes
are added. Target capacity is distinct from reviewed regression ceilings; measured
free heap/largest block are required at every journey stage, and actual opening
and navigation must succeed rather than predicting admission from idle heap.

After the hardware owner has installed and read back that exact candidate, they
can explicitly run (this task performed no device/SD action):

```sh
python3 -B tools/reading-ready.py journey --variant regional-dev-usb \
  --install .build/private/install.json --plan .build/private/plan.json \
  --target owner-x3 --port /dev/EXPLICIT-OWNER-PORT \
  --output .build/private/journey.json --latency-cycles 20
python3 -B tools/reading-ready.py hardware-validated --variant regional-dev-usb \
  --install .build/private/install.json --plan .build/private/plan.json \
  --target owner-x3 --journey .build/private/journey.json \
  --ui-review artifacts/library-ui-review.json \
  --latency-conditions .build/private/conditions.json \
  --latency-baseline .build/private/approved-baseline.json
```

The fixed journey holds **one** existing `SerialClient` connection. Its existing
two HELLO handshakes verify the selected build and a stable boot; each press
requires ACCEPTED, CONSUMED and a terminal result. No discover/reset/retry or
Sleep input is added. Begin at Home. The runner visits Library, selects a primary
book from its measured current index, opens it, turns forward twice/back once,
exits to Home, reopens via the physical Back/Read route and verifies the anchor.
It repeats open/turn/exit for a distinct admitted sibling; opens an intentionally
rejected authored EPUB; requires an immediately responding Library STATUS/DIAG;
then reopens/turns both admitted books at their saved anchors and finishes Home. It stops on the first
unexpected screen, unavailable diagnostic, book/anchor mismatch or ambiguous
completion, closes the connection and retains failed/partial evidence.
The current fixed journey contains 27 stages, including the recovered chapter
boundary exit/reopen/back/forward checks. The primary book starts at an owner
selected position where the first forward turn stays in the same spine and the
second crosses a chapter; that positioning remains under the hardware owner's
existing scope. The command never prepares or resets book state.

Every stage records current free heap/largest block, actual reader status/open,
source identity and position. The exact fixed commands and returned response
bodies must match the live ordered protocol audit; merely supplying stage labels
or renumbering an audit with all presses removed fails. The strict validator rejects missing/skipped/unknown
stages, simulation, stale/future/preinstall evidence, wrong source/image/procedure/
media/device, resets, reopened connections, corrupt/duplicate/oversized JSON,
missing/late terminal responses and changed Library counts. The freshness window
is fixed at 24 hours; the journey has a 600-second bound and the existing client
has a 30-second per-command bound. It certifies that bounded session, not all books
or future behavior. Private identities, passages, plan hashes and raw receipts
must not be published; stdout only gives state and acceptance statuses.

## Producer integration contract

This tooling consumes the current runtime's STATUS and the
existing DIAG via `client.request`, reusing protocol/checksum/nonce enforcement.
`tools/reading_acceptance.py::DIAG_KEYS` requires the existing DIAG fields plus version=2,
`current_book`, `heap`, `largest`, `opening`, `library_count`, `library_selected`,
and `device`. HELLO must advertise `press,status,frame,diag2`.
`current_book` is the current selected reader source, not the retained wake target;
`device` is the first 16 hex characters of the SHA-256 of the uppercase,
colon-delimited factory MAC/USB serial, not a per-connection boot nonce.
SD current_book uses the first eight full-source SHA-256 bytes as little-endian.
The source owner still verifies the full source digest; the short wire identifier
is an observation match, not a replacement for source admission.
Library count/index must reflect the actual browsable catalog while on Library;
zero is permitted outside it. The runtime owner confirms a versioned producer and a 512-byte record bound
(worst 507 bytes including request identity, CRC and newline); its 64-byte DEV
buffer increase needs the existing resource review. The combined reader/Library
checkpoint includes those reviewed producer/client changes and their exact native
cost review. The old producer is deliberately insufficient: it fails closed before
any press. The existing ProtocolClient.trace must preserve each exact decoded raw `body`
on every request/response record; kind-only audits fail closed. Parent coordinates
schema changes and preserves runtime/UI workers' existing `check.py` additions.

Private installation JSON contains exactly schema=1, target (opaque alias), device
(stable private 16-character fingerprint), build (verified native HELLO build ID), binary_sha256,
readback_sha256 (the exact installed application image bytes, not whole-flash
hash), inputs_sha256 (existing resource source identity), and installed_at (Unix
seconds). The plan contains media_sha256 (private owner-maintained catalog identity),
library_count and primary/sibling/rejected objects with index and numeric book.
Use the exact private admission metadata under the hardware owner's authorization.
Repository-only examples use `tests/fixtures/the-small-lantern.epub`, the existing
`tools/make-compressed-reader-fixture.py` variant, and a deliberately malformed
EPUB containing authored non-ZIP bytes. Never commit private books/hashes. The
media-plan binding is an operator-supplied media identity, not a device-wide SD
hash claim; the live current-book identities and catalog count are independently
checked. New copying or card preparation requires the existing owner scope.

## UI and physical acceptance

`tools/system_ui_contract.py` renders the same C++ host component as firmware.
Its matrix covers Home plus Library's four focus positions, empty, unavailable,
loading, no-cover, corrupt-cover, long-title, 43/61, last shelf, sparse shelf and actual prepared-cover/error states. It also pins
reader/menu/settings/device snapshots changed by shared component integration.
The prepared fixture exporter and firmware sidecar adapter are reused from the
UI child; absent producer tools fail final admission.
It compares the canonical header pixels and shared Up/Down footer region with
Home, requires a visible focused-entry change, and checks every full-frame hash
against an explicitly reviewed manifest. The UI child owns shared components,
font/focus geometry and actual button mapping tests. Existing verbatim firmware
input tests continue to enforce the physical Back/Read and directional routes.

`export_for_review(binary, output)` exports actual PBM/PNG files and a candidate
manifest with **blank** review. Inspect every state, then deliberately record
review and save `artifacts/library-ui-review.json`; no command auto-blesses pixels.
A new design's goodness is a human decision. Changed renderer output fails the
reviewed hash comparison even when shared chrome still matches. The old Library
renderer is an actual negative control, not an approved baseline. Host CI runs
`python3 -B tools/check.py` through the reviewed managed-Python workflow. Its
mandatory navigation baseline also runs in every focused host lane. The tests are
invoked by the canonical host gate. After UI integration, that gate and final
hardware admission both compare the entire current matrix with the reviewed
manifest; a missing manifest or any unreviewed pixel change fails.

Hardware-validated leaves battery wake, optical/buttons and owner acceptance
pending. Only after physical checks, use `owner-accepted` with the same arguments
and `--owner-review .build/private/owner.json`. That exact JSON contains
journey_sha256 (SHA-256 of the private terminal journey file), battery_wake,
optical_buttons and owner; each of the latter three must be `accepted`. This is
explicit manual evidence, never inferred from USB or screenshots. No 'ready to
use' result is supported before these independent statuses pass.

The JSON `ready_to_use` flag is true only for owner-accepted. Hardware-validated
keeps that flag false even after automated reading succeeds.

## Independent serial latency admission

`docs/reading-latency-contract.json` preserves the exact contract committed at
`6911ddb16b50d657dd779036fc3eefd96756be86`. Its historical Home/Library aggregates
are context with small samples, not approved current baseline evidence. The
evaluator uses individual PRESS records from this journey. It separately measures
the monotonic wrapper interval and matching protocol request-to-terminal interval,
retaining ordered ACCEPTED/CONSUMED/terminal timings. STATUS's last completed
refresh duration stays separate. Existing driver/API tags classify the matching
PRESS window; missing or mixed tags are unknown. These are software/USB/serial
measurements; optical latency, waveform, ghosting and battery behavior remain
unmeasured. The p50 ≤ 500 ms, p95 ≤ 750 ms and maximum ≤ 1000 ms goals are
reported separately from the baseline
regression checks, so being above a goal alone cannot block a stable candidate.

Without `--latency-cycles`, the unchanged correctness journey records provisional
timings. An explicit 20–100 cycles adds bounded previous/next pairs immediately
after `primary_next` using the already observed adjacent same-spine anchors. Each
turn must return the exact expected book/spine/character with a fresh render and
valid STATUS/DIAG before continuing. The sampler returns to the original forward
anchor, then executes all remaining fixed stages. It uses the existing single
connection, 600-second session bound and 2048-record audit cap, with no retry or
forced refresh mode. Before connection, the wire forecast also reserves the
plan's worst-case Library positioning; large catalogs can exhaust that bound
even without sampling, so available cycles depend on the complete plan.
Unknown, no-visual-change or insufficiently sampled refresh
classes cannot satisfy release latency admission; rarer modes may remain blocked
even at the 100-cycle cap.

`hardware-validated` and `owner-accepted` both require the independent latency
ratchet after correctness validation. The current source revision and image size
come from the verified native sidecar/resource artifacts, not the journey itself.
Both current and approved baseline bundles must independently pass correctness,
freshness and wire-binding validation, and match the device, media/plan, actual
sampled anchors, procedure, endpoint mode and explicit owner-observed conditions.
Policy must cover exactly the observed navigation/action/refresh/endpoint groups.
The proposed engineering floor is at least 20 individual samples in each group,
using nearest-rank p50/p95/max; it makes no confidence claim. Each of those three
statistics must stay within the reviewed baseline plus an explicitly reviewed
noise allowance. No automatic allowance or baseline growth is applied.

The checked-in `docs/reading-latency-policy.json` deliberately has no approved
baseline or sampling/noise review. Installable and journey states remain usable
for an already authorized physical trial. To prepare a reviewable baseline,
the owner can request `--latency-bundle .build/private/candidate-baseline.json`
and `--latency-report .build/private/analysis.json` during hardware admission,
with explicit `--latency-conditions`. That initial evaluation remains blocked
while producing exclusive 0600 private files. After reviewing the raw physical
samples and conditions, a separate policy edit records the raw bundle SHA-256,
reviewer/reason, complete groups, sample floor and noise limits. The CLI never
approves its own measurements or overwrites a baseline. The execution procedure
includes the producer, evaluator, contract and relevant controls; policy approval
has its own digest to avoid a circular baseline/procedure hash.

The conditions file is a bounded object of explicit text declarations, such as
power, temperature and display settings, supplied by the owner. These are trusted
observations rather than independently sensed values. Raw device/media/request
identities, conditions and timing samples stay in the explicitly requested files
under ignored `.build`; stdout contains sanitized admission status. Installation
preservation and normal-return proof remain the hardware owner's existing
responsibility: the minimal installation JSON alone does not prove those checks.

## NEWT-22 durable save and exit contract

The mount still allows three open files. Before each routed SD state transaction,
`SdReadingState::begin` asks the owning revision session to release its optional
Pages read lease. The admitted source and receipt may stay open, leaving one
slot for selected-book, progress, wake-destination or DEV Home-preference state.
A paused producer already suspended its provisional writer; the state transaction
does not reset that writer. A later cached turn reacquires Pages through the
existing complete source/receipt/cache checks. No per-book exception or extra
reader-open/page-turn hook is involved.

Runtime acknowledged progress advances only after a verified checkpoint. A failed
attempt retains newer live progress separately as pending recovery. Back and
Sleep proceed even if storage cannot confirm the attempt; they never label it
acknowledged. Complete writes with a lost response get bounded readback through
the same namespace/digest validation. Unresolved results mean a complete old or
new value may load after restart. Independent Home-preference commits keep their
existing verify-before-publish rule and use the same reserved descriptor.

The reader footer uses its center chapter field for `PG SAVE ERR` at the full
16 px size only while the recovery owner's explicit Page uncertainty bit stands.
Other storage diagnostics show `STORAGE`; clear state restores the chapter label.
The right column keeps overall percent and the page fraction is unchanged. No
storage-message text is treated as evidence of page-save failure. Sleep paints
the detailed warning before power-down. Your X3 keeps the detailed Storage line;
Home reuses its existing hint for `Last save unconfirmed`. No overlay or resident
buffer is added. The detailed copy is:

- `Save unconfirmed. Restart may use old or new page.`
- `Page save unconfirmed. Restart returns Home.` (Home commit verified)
- `Home save unconfirmed. Restart may reopen book.`

Warm RTC restoration writes live progress into the recovery owner, independently
of durable acknowledged progress loaded at startup. RTC v7 carries Page and
Destination uncertainty in existing recovery bits; the record stays 68 bytes.
Reopening/publishing a page cannot clear either uncertainty. A verified retry
retires only the acknowledged part, and Sleep repaints when destination wording
changes. Cold power-on still uses the durable authority and never RTC.

Recovery selection leaves acknowledged progress unchanged. The save boundary
captures a valid live or last-good tracker after an unconfirmed save, including
a failed chapter with no publishable workspace. A verified save retires older
pending state. Settings keeps its reflowed retry when the final commit is
unconfirmed rather than marking it acknowledged.

A failed Home destination must not imply that the previous Reading intent was
replaced. Pending progress may remain usable during the live session, while
battery power-off can restore the preceding durable destination instead.

`tests/reader_durable_escape_test.py` exercises actual startup/save/exit bodies
with the real three-descriptor source/receipt/cache/state owners and an authored
EPUB. Run its default cold-POWERON route, `--usb` Library route and
`--negative-control`. The post-save turn must publish a cache hit with no ordinary
layout fallback. Exporting the same regression against `213c28f` fails its exact
checkpoint/Sleep/Back escape predicate in both reset routes. The full host gate
includes all three current-source modes, lifecycle and preference restoration,
failed open/write/readback and lost-acknowledgement coverage, and cache parity.

Host results, firmware compilation and physical power behavior are separate
facts. Owner verification remains pending for USB-powered sleep/wake, unplugged
battery sleep/wake, and cold power-on. Record power condition and reset evidence
for each; USB success cannot satisfy battery acceptance. No candidate is ready
for owner acceptance solely from these software checks.
