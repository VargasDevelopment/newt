# Overlay Home and one device refresh policy

## Why

**Direct gray Home.** Home's frosted gray was drawn as Direct (absolute) gray.
The first black-and-white paint after it needs a destination pass and a GC:
1.32 s every time a book opened from Home.

**The experiment** drew Home as an Overlay instead. Reopen fell to about 1.1 s,
and the owner saw no ghosting. It still regressed the rest of the device:

- **Home focus moves** repainted all of Home. The quiet cursor island only
  worked with Direct gray, and repainting Overlay gray starts with a
  whole-panel white scrub.
- **Leaving Home** for Books, Apps or Your X3 forced a GC. Returning paid the
  scrub again; only the landscape reader got the cheap exit.
- **Gray appearance:** the gray masks were uploaded MSB first, which the
  driver silently rejects. Home's grays were drawn from the wrong planes.
- **Split refresh policy:** Home's focus budget and the UI fast-paint budget
  were separate counters, and reader paints spent the device budget.

## What changed

**Overlay Home** (regional builds; `NEWT_HOME_OVERLAY`, default
`NEWT_REGIONAL_HOME`):

- **Base and masks:** the base is the MSB selector plane. The LSB mask is dark
  (`L&~M`), uploaded first; the MSB mask is any gray (`L^M`).
- **Repainting a shown Overlay Home** (metadata changed) first rebases
  controller RAM to the inverse of the new base, so the settle pass drives
  every pixel and no white scrub runs.
- **A due cleanup** requests a resync instead, so the base is cleared with a
  GC from whatever the panel shows, including a black-and-white screen such as
  Books. That is the periodic full refresh. Without the resync, arriving from
  black and white would settle the base differentially and still reset the
  budget.
- **Leaving Overlay gray** for any screen, in any orientation, uses the same
  inverse rebase plus one fast pass, which drives every pixel. It counts as a
  cleanup. Direct gray (the sleep screen) still leaves with a full refresh.

**Quiet focus under Overlay.** This extends the tracked regional FreeInk patch
(`patches/freeink-regional-gray.patch`; the manifest's hashes advance).

- When an Overlay base lands, the UC8279 driver captures the binary content of
  the fixed 240×16 cursor island.
- After an Overlay pass whose masks are empty inside the island, the existing
  island API accepts an update. It first restores the captured on-panel island
  into OLD, because the masks occupy that RAM, then runs the existing windowed
  DU. Gray outside the island is untouched.
- Every other display operation invalidates the proof.

**One device policy.** `UiRefreshPolicy` is the policy for every screen except
the reader: Home, Books, Apps and Your X3. Fast updates (DU and the cursor
island) are the default, with a full cleanup after nine. Home no longer keeps
its own focus budget. The reader's policy, `ReadingRefreshPolicy`, now holds
its page cadence plus its own nine-paint cap. Neither spends the other's
budget. `kFastPaintsBeforeFull` is the single constant to make configurable.
Only completed paints spend the device budget. Only a full pass or a cleanup
resets it, and a status-only Home update never touches it.

**Cold-open feedback.** Before a cold open (no imported receipt for the book),
the existing "Opening your book" screen is shown:

- one fast pass over black-and-white portrait;
- the inverse drive plus one fast pass when leaving Overlay Home, which counts
  as a cleanup;
- otherwise (Direct gray or landscape), a full refresh, which also counts as a
  cleanup.

The 20 s open budget restarts after the screen is painted, so the paint is not
charged to the open. A timed-out open stays timed out. Warm reopens show
nothing. The check is one FAT lookup, plus a second for the receipt's `.bak`
recovery copy when the receipt itself is missing.

Superseded by [Decision 075](075-slow-action-feedback.md): every owned open
now shows a working frame, and a black-and-white start is fast in either
orientation.

**Battery under Overlay.** The minute timer no longer redraws Home's battery
percentage while Home sits idle. The region update exists only for Direct
gray. The percentage updates with the next Home paint: any focus move or
return to Home.

**Revert.** `NEWT_HOME_OVERLAY=0` restores Direct gray Home with its battery
region update. It is a build-flag change, so it needs its own clean builds and
review.

## Device evidence (owner's X3, regional dev-usb, 2026-10-05)

Measured on `54cf1bc` (tag `evidence/round2-54cf1bc`), before the review fixes:

| Action | Before (Overlay experiment) | Now |
| --- | --- | --- |
| Home focus move | revert GC, settle and AA: 3 full passes | island only: 0.51–0.55 s |
| Home → Books, Apps or Your X3 | full GC | one full drive: 0.39 s display |
| Inside Books or Apps | 0.39 s DU | unchanged |
| Books, Apps or Your X3 → Home | revert GC, settle and AA | settle and AA: 0.94 s |
| Reader → Home | settle and AA | unchanged (0.94 s) |

- **Boot:** the first black-and-white paint after boot is still a GC, because
  the panel spends a boot-time full-refresh budget.
- **Cold open (book 1 after a purge):** the opening screen took 0.39 s, and the
  open completed in 8.8 s.
- **Warm reopen** of the same book: no opening screen, 0.95 s.

On the final `1d5e298` image (device action totals, one session):

- **Books → Home with a cleanup due** (after nine Library moves): 1.67 s.
  With no cleanup due it took 0.98 s. The 0.69 s difference matches one GC
  pass on this panel (693 ms at boot).
- **Reader → Home:** 1.03–1.07 s. **Home → Books:** 0.97–1.00 s.

**Owner judgement (2026-10-05):** the Overlay Home frosted grays look better
than Direct. Ghosting over longer use is still the owner's to judge.

## Host evidence

- **`home_refresh_regional_test`:** the real patched UC8279 driver on a
  recording bus.
  - The Overlay island stages OLD (the on-panel content) and NEW before the DU,
    then synchronizes OLD.
  - Gray inside the island, a black-and-white rebase, a cleanup or a resync
    rejects the island without writes.
  - From black and white, an Overlay base is one settle refresh; with the
    due cleanup's resync, it is a GC plus the settle.
- **`frosted_home_test`:** built in three variants (ordinary, regional Direct
  and regional Overlay).
  - Overlay call order, with the LSB mask before the MSB mask.
  - The base equals the MSB plane, and the masks mark exactly the dark and gray
    pixels.
  - Rebase only for a shown Overlay with no cleanup due; a due cleanup
    requests a resync instead.
  - The island keeps the gray.
  - Nine island moves, then a cleanup.
- **`home_refresh_test` and `reading_refresh_test`:** one device budget, plus
  the reader's own cap, which device paints never spend.
