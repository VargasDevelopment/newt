# Battery recovery acceptance

This is a proposed owner procedure, not a hardware result. The owner reports
selection and progress loss after a chapter error followed by battery sleep/wake.
The device's earlier reset class and records remain unverified. The owner has
separately returned the unit for bounded diagnostics; this proposed procedure
authorizes no installation or additional device action.

The [27-stage chapter journey](038-chapter-transition-diagnosis.md) remains
required and unchanged. Its USB protocol evidence can establish chapter crossing
and close/reopen within that session. It leaves `battery_wake=manual-pending`
and `book_completion=unverified`. Host tests, firmware builds, USB resets and
synthetic receipts cannot establish battery recovery.

## Evidence required before scheduling the owner run

The parent must identify the exact reviewed installed image, its existing
installation/readback evidence and the agreed physical actions. Use one already
admitted SD source with a recognizable noninitial checkpoint beyond a chapter
boundary. Keep the same source, card, orientation and settings for both cases.
Do not reproduce an error deliberately unless its bounded trigger is separately
agreed. Preserve any existing failed run as evidence before preparing another.

Record the source's complete 64-hex-character SHA-256 in private evidence. A
title, Library index, filename or 64-bit `current_book` is insufficient. An
independent record review must decode the selected valid `newt_selected` record
using [SelectedBookCodec](../firmware/selected_book_store.h), verify its filename
and full SHA against the admitted source, and decode that source's saved progress
and the `newt_resume` destination. Apply the existing record validation and slot
ordering rules; never choose whichever slot gives the desired result.

Use an already authorized, read-only evidence route for this record review. No
new extractor, device attachment or card read is approved here. The current
local Wake details view exposes frozen expected/actual locators but does not
expose the complete selection SHA. If independent selection/progress/destination
evidence cannot be obtained without changing the tested boot or records, mark
that part **unverified** and stop short of acceptance. Do not replace it with a
screenshot of the correct title or a matching short book identifier.

The current frozen local receipt is compiled under `NEWT_DEV_USB`. If the
selected production image omits that view, the parent must first resolve the
missing local reset/open evidence in a separately reviewed observation change.
This procedure does not authorize a diagnostic image installation, and a USB
session opened after wake must not replace the original battery boot evidence.

## Owner sequence after explicit hardware release

Run one bounded pair. The owner performs every physical action. USB must be
disconnected before setup and stay disconnected through sleep, wake, the first
awake photograph and the local Wake details capture. Record all button presses
and their order, including presses needed to reach Your X3. No host session runs
during either case. Do not disconnect the battery, open the enclosure, drain the
cell, induce brownout or use a USB/software reset to manufacture a cold boot.

1. **Intended Reading.** Open the agreed source and confirm the independently
   recorded exact `(spine, character)` checkpoint with unchanged settings. Use
   the ordinary exit/reopen route to establish that checkpoint first. From
   Reading, use the agreed physical power-button sleep action; record the
   reading sleep scene and any `UNSAVED` or storage warning. Wake with the normal
   physical button. Before any navigation, photograph the first awake screen
   and recognizable passage. Then photograph every local Wake details line
   and the storage warning, preserving the initial open result and locator.
2. **Intended Home.** After the first case is captured and reviewed, use the
   ordinary Back/exit action to Home and confirm the checkpoint remains saved.
   Sleep from Home with the same physical action and then wake. Photograph the
   first awake Home screen before input, followed by every Wake details line
   and storage warning. Only after those captures, use Home Read/Back to reopen
   and independently check the same full source identity and exact anchor.

The Reading case tests a saved Reading destination; the Home case tests an
intentional Home destination without losing the selected source or its progress.
Home on wake is therefore judged against the recorded intent, not against a
universal expectation that every wake must open a book. Do not navigate to Home
between the Reading sleep and its first-awake capture.

If the chapter error is already present, preserve its typed error and last
successfully published anchor as a separate failed-run attachment. Do not count
an error screen, unavailable anchor or failed save as an acknowledged Reading
checkpoint. Stop on an unexpected screen, warning, reset, wrong source, missing
locator or ambiguous button result. Capture what remains available; no retry,
erase, migration, format, backup restoration or reflashing follows automatically.
The separately approved normal UI saves are the only intended record changes.

## Classify the actual boot before judging recovery

Read the raw reset and wake values from the locally captured boot receipt and
decode them against the exact installed SDK. Do not infer reset class from the
sleep image, USB disappearance, screen appearance or owner button action.

| Actual boot evidence | What this run can establish |
| --- | --- |
| `ESP_RST_POWERON`, USB disconnected throughout, owner physical action | Battery cold-start candidate. Reading requires the validated durable destination, selected source and matching checkpoint; Home requires the saved Home destination. |
| `ESP_RST_DEEPSLEEP` with the recorded physical wake cause | Battery deep-sleep wake candidate. Review retained-record validation and exact reopen separately; this does not pass cold-start persistence. |
| `ESP_RST_SW`, including rejected sleep-entry restart | Software restart evidence. Record the rejected-entry status; this does not pass the battery cold-start or deep-sleep case. |
| Brownout, watchdog, panic, other or unavailable reset class | Failure or inconclusive recovery evidence. Preserve the result; no cold-start pass and no induced repeat. |

For Reading, a cold-start pass requires the saved Reading route, successful
initial reader open, `reader_status=ok`, no restore failure, the same orientation,
and exact equality of independently saved, boot-expected and initial actual
`(spine, character)` anchors. The first completed paint and the owner must show
Reading. A successful later manual reopen cannot repair a failed initial resume.
Record any subsequent Reading-to-Home event separately so it does not obscure
the initial result. A validated-anchor recovery policy may be recorded, but a
containing page with a different character start does not pass this exact-anchor
case; it needs a separately defined recovery case.

For Home, require an independently validated Home destination, first completed
Home paint and no automatic reading attempt. A missing/untrusted intent that
merely defaults to Home cannot pass. The subsequent explicit reopen must retain
the full selected-source SHA and exact saved checkpoint. Evaluate selection,
position and intended destination independently in both cases.

## Private manual evidence contract

This compact schema describes an owner-reviewed receipt; no existing command
validates it or promotes readiness automatically. Values below are unfilled.
Keep passages, photos, full source SHA, filenames, device identity and raw records
private. Commit only a sanitized outcome and references to approved evidence.

```json
{
  "schema": 1,
  "kind": "owner-battery-recovery",
  "installed_image_sha256": null,
  "installation_evidence_ref": null,
  "owner": null,
  "cases": [{
    "intended_destination": "Reading or Home",
    "started_at_utc": null,
    "usb_disconnected_entire_interval": null,
    "physical_actions": [],
    "selection_before": {"full_sha256": null, "record_evidence_ref": null},
    "saved_checkpoint": {"spine": null, "character": null, "record_evidence_ref": null},
    "saved_destination_evidence_ref": null,
    "first_awake_photo_ref": null,
    "local_wake_details_ref": null,
    "reset_raw": null,
    "wake_cause_raw": null,
    "decoded_reset_class": null,
    "initial_open_result": null,
    "restore_policy": null,
    "expected_anchor": {"spine": null, "character": null},
    "initial_actual_anchor": {"spine": null, "character": null},
    "first_completed_screen": null,
    "storage_warning": null,
    "selection_after": {"full_sha256": null, "record_evidence_ref": null},
    "home_explicit_reopen_anchor": null,
    "selection_result": "unverified",
    "position_result": "unverified",
    "destination_result": "unverified",
    "battery_cold_start": "unverified",
    "battery_deep_sleep": "unverified"
  }],
  "reviewer": null,
  "reviewed_at_utc": null,
  "book_completion": "unverified"
}
```

Create a separate case object for each physical run. Fields not applicable to a
Home initial open remain explicitly unavailable; its explicit reopen supplies
the anchor check. Missing required evidence never means pass. Acceptance needs
both intended destinations with the required actual reset class and independent
selection/position checks; a single deep-sleep run leaves cold-start acceptance
pending. The existing owner-review `battery_wake=accepted` must cite these
reviewed physical results and must not be filled from a USB journey or host test.
