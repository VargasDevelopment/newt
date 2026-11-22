# NEWT-22 round 3b: required DEV reader stack stop

Historical checkpoint, resumed under the Lead's round-3c restructuring rule.
See [the current report](NEWT22-round3c-report.md) for subsequent results.

Started clean at `688b5c95dc9c9fd033b89fda1f0f9d702aa11576` on
`fix/unsaved-stuck`. The Lead authorized a startup helper and new footer placement,
while requiring another STOP for any selected stack violation. All four clean
commands completed. **This partial implementation is not a passing candidate.**

| Variant | Flash | SRAM | RTC | `setup()` actual / ceiling | `openReader()` actual / ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,161,552 B | 133,232 B | 300 B | 384 / 384 B | 144 / 144 B |
| DEV USB | 2,301,696 B | 134,080 B | 304 B | 384 / 400 B | **592 / 560 B** |
| regional | 2,168,256 B | 133,728 B | 300 B | 384 / 384 B | 144 / 144 B |
| regional DEV USB | 2,310,400 B | 134,576 B | 304 B | 384 / 400 B | **592 / 560 B** |

The existing `SleepSession::restore` coordinator is now explicitly noinline.
It measures 112 B in all lanes; the original selected-entry lists are unchanged.
Both DEV `paint()` frames decrease from 48 to 32 B. All remaining inherited
selected frames equal the prior measurements. No selected entry is missing
(counts remain 350 / 379 / 352 / 386). No ceiling, protected-history record,
exclusion, adapter or measurement selection was changed. These individual frames
do not bound their combined call chains.

SRAM and RTC equal the previous actuals in every lane. Frosted Home remains
496 B and Stage Home 320 B in every lane, so the reused Home hint itself fits
the existing measured budgets. Its wording remains `Last save unconfirmed`.

All compiler/linker and NVS/sleep guard checks passed. Resource commands failed
on the two prohibited DEV frames and on unreviewed flash growth: +1,088 / +1,104 /
+1,104 / +1,104 B relative to the current record 41. The usable partition limit
remains 7,016,448 B. Record 41 is unchanged; no duplicate exception or new ceiling
was added to obtain a pass. Raw four-lane reports, command logs and a compact
resource summary are archived as `artifacts/newt22-r3b-*`. They measure dirty
partial source, not clean acceptance receipts. Previous accepted receipts were
preserved in their original artifact paths.

## Footer implementation and classification

One production constant defines `PG SAVE ERR`. An explicit page-uncertainty bit
from `ReadingSaveNotice` is projected into `DeviceInfo`, using its existing
padding. No text matching decides whether page/progress persistence is uncertain.
While that bit stands, the center chapter field shows the 103 px message at
16 px in both its 140 px portrait and 200 px landscape fields. Overall percent
and page fraction keep their normal renderer and positions. No token occupies
the right column. The separate source-branch estimated-percent tilde remains
unchanged because the Lead explicitly prohibited merging main.

When no page-save uncertainty stands, any remaining `info.storage` diagnostic
shows the short generic center label `STORAGE`. With neither state, the normal
chapter label returns. Detailed diagnostics remain available through Your X3;
an unavailable reader also retains its existing explanation. Thus destination-only
uncertainty is not mislabeled as a page-save error. A concurrent foreign diagnostic
cannot hide an actual page uncertainty: the explicit Page bit takes precedence.

The inspected non-page cases are:

- Reading/Home destination commit uncertainty and unavailable/corrupt resume
  authority: `Save unconfirmed...`, `Home save unconfirmed...`, and
  `Saved wake destination unavailable. Existing data kept.` The shared Reading
  message can describe destination-only uncertainty, so its text is insufficient.
- `sdSelection.storageProblem()`: `Book selection not verified. Reading position
  is kept.`, `Saved book selection unavailable. Existing data kept.`, or the NVS
  diagnostic. A verified page checkpoint does not imply selection acknowledgement.
- Source reopening: `No SD card. Insert it, then press Read.`, or
  `Saved SD book unavailable or changed. Position kept.`
- Reader-session admission: `Reader session unavailable. Position kept.`
- NVS: unsupported version, recovery required, or unavailable storage, each with
  the existing data-kept wording.
- DEV Home preferences: unavailable durable preference with retained warm choice
  or Frosted fallback, invalid saved preference, and unconfirmed preference commit.
  Preference persistence remains independent of reading acknowledgement.

The recovery owner still preserves page and destination uncertainty separately
across warm wake; reopening a page is not durable acknowledgement. The descriptor
rule remains unchanged: the owning revision session releases its optional Pages
lease before routed durable transactions, leaving one of the existing three SD
slots for state. Source/receipt validation and later cache reacquisition remain
unchanged. See the preceding R3 stop report for the inspected startup, save, exit,
Settings, Library, source and warning-publication sites.

## Tests executed before STOP

- Footer regression PASS: both geometries compare exact 16 px center glyphs;
  page-save changes affect only the center; percent/fraction/battery stay intact;
  foreign and destination diagnostics produce `STORAGE`; clearing uncertainty
  restores chapter metadata.
- `reader_save_wake_test.py` PASS, including full ordinary-RAM loss, both save
  failure types, verified retries and the new explicit page-state assertions.
- `failed_chapter_sleep_test.py` PASS, including both repaired coordinator-header
  negative controls and the old opened-only sleep control. It mutates private
  header copies and requires intended runtime failures, not compiler failures.
- `sd_power_on_resume_test.py` positive matrix PASS.
- Its `--negative-controls` mode remains incomplete: controls 0 and 5 replace
  discarded call expressions with bare `false;`, which fails compilation under
  `-Werror`. The other controls reached their intended runtime failures. No
  further repair was made after the mandatory resource STOP.

## Still pending

The two DEV selected-frame violations must be resolved under a subsequent Lead
instruction. Then finish the remaining negative controls, authored rendered footer
evidence, quick/focused/full Linux gates, original old-head witness revalidation,
turn-cost matrix, every-page parity, fresh different-family bounded adversarial
and maintainability review of `683fe1d..HEAD`, one reviewed exact flash record,
and four passing clean receipts. The old R3 witnesses on `683fe1d` and earlier
descriptor witnesses on `213c28f` remain archived; no new old-head run is claimed.

The required final read-only integration inspection against main `7cc92ab` was
not performed: the brief places it after fixes pass. The user reports overlapping
footer edits and another flash record 41, but their exact conflict has not been
verified this round. Main was not merged and the battery worktree was untouched.

No fresh adversarial review or acceptance approval is claimed. USB-powered
sleep/wake, unplugged battery sleep/wake, cold power-on, and exact-source Mac
ImageIO checks remain pending. No hardware was touched.

The pinned release gate passes `213c28f..153a631`, including this round's source
checkpoint. Its log is archived as `artifacts/newt22-r3b-release-range.log.gz`.
