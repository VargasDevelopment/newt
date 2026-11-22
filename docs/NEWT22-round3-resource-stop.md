# NEWT-22 round 3: required resource stop

Historical checkpoint, resumed under the Lead's round-3c restructuring rule.
See [the current report](NEWT22-round3c-report.md) for subsequent results.

Historical checkpoint. Round 3b supersedes this stop; see
[its measured resource report](NEWT22-round3b-resource-stop.md).

The round began clean at `683fe1db99963f573ddfd03aa32f64a5765147e2` on
`fix/unsaved-stuck`. This is a partial implementation checkpoint, **not a passing
candidate**. Round 3 requires stopping when a selected stack measurement exceeds
its existing ceiling. The first clean default build triggered that rule:

| Default lane | Previous | Round-3 measurement | Existing ceiling |
| --- | ---: | ---: | ---: |
| `setup()` selected frame | 384 B | **400 B** | **384 B** |
| SRAM occupancy | 133,232 B | 133,232 B | unchanged |
| RTC occupancy | 300 B | 300 B | unchanged |
| Frosted Home selected renderer | 496 B | 496 B | 496 B |
| Stage Home selected renderer | 320 B | 320 B | 320 B |
| Flash image | 2,160,464 B | 2,161,232 B | 2,160,464 B |

Every other inherited selected default frame equals its previous measurement.
The native compiler/linker and NVS/sleep guards passed; the resource command
failed on flash (+768 B, requiring review) and `setup()` (+16 B, prohibited).
The raw report and command log are archived under `artifacts/newt22-r3-*`.
They measure the dirty partial implementation, not a clean release receipt.
No ceiling, history pin, exclusion, adapter, partition, file limit or selected
measurement boundary was changed. Existing accepted record 41 and all previous
evidence remain intact; no second flash exception was added.

## Partial implementation

`ReadingSaveNotice` centralizes page and destination uncertainty in the recovery
owner, replacing its existing one-byte saved flag without increasing its size.
RTC v7 retains uncertainty in unused bits of the existing recovery word; its
record remains 68 bytes. Actual startup restores the RTC live checkpoint into
recovery, separately from acknowledged durable progress. Reopening/publishing a
page cannot retire uncertainty. Verified checkpoint and destination results
update their respective notice state. Sleep repaints when the warning changes.
Settings attempts an explicit verified retry before retiring uncertain recovery.

The footer constant is `SAVE?`. The three detailed strings remain:

- `Save unconfirmed. Restart may use old or new page.`
- `Page save unconfirmed. Restart returns Home.`
- `Home save unconfirmed. Restart may reopen book.`

Home reuses its existing hint line for `Last save unconfirmed`, through an unused
flag bit and its existing refresh metadata. Default-lane RAM and both selected
Home frames show zero growth. The other three variants were not measured after
the stop, so the optional Home sub-item has not passed four-lane acceptance.

The descriptor rule is unchanged: before each routed SD state transaction,
the revision-session owner releases only the optional Pages read lease, leaving
one of the three mounted descriptors for durable state. Source/receipt ownership,
validated cache reacquisition and producer behavior remain unchanged.

Inspected state/publication sites include `saveReaderProgress`, `openReader`
post-publication and source/session failures, `finishReaderExit` Back/Sleep,
`enterSleep` destination retry/repaint, canceled-sleep live recovery, Settings
reflow, `openSdSelection` publication, startup storage initialization and actual
`SleepSession::restore`, plus `LibraryReaderSelection` selection/open/source
storage assignments and checkpoint-authority warnings. Notice publication keeps
unrelated source/selection diagnostics; the uncertainty owner remains separate
from that display pointer. The complete sweep has not received final independent
review.

## Executed host evidence

The new `tests/reader_save_wake_test.py` passes with ASan/UBSan, both orientations:
rejected progress write, final-intent-only rejection, and failed Back followed by
verified Home Sleep retry. It reconstructs reader, selection and sleep owners,
clears ordinary application/recovery/acknowledgement/display/timer state, then
restores only RTC and the modeled durable backends before actual deep-sleep
startup and post-open publication. Tests assert durable bytes, acknowledged vs
live progress, warning/recovery survival, no hidden wake writes, later verified
retry, cold old-page control and cold new-page restoration.

The three cases independently fail at their intended runtime assertions on an
immutable `683fe1d` archive. The witness and successful run are archived.
`sleep_session_test.py`, `sd_power_on_resume_test.py`, the periodic checkpoint
suite including its four negative controls, and the default real cached
descriptor-escape regression also pass. The descriptor regression still records
a cache hit after saving, with no ordinary layout fallback.

The failed-chapter positive matrix passes all fourteen kind/orientation cases,
but its overall script fails because two negative-control mutation anchors still
target the former inline save-owner implementation. Repairing those anchors is
unfinished. The new gate registration therefore does **not** imply a passing
complete host gate.

## Deferred by the mandatory stop

- Repair and rerun the failed-chapter negative controls; complete warning-owner
  sweep, Settings/Library sibling coverage and review.
- Quick and full Linux gates, remaining focused/negative-control modes and
  revalidation of the original `213c28f` witnesses for this round.
- All four passing clean native lanes, reviewed exact flash values and any
  permitted tightening. Only default was measured; no other R3 sizes are claimed.
- Fresh turn-cost matrix, every-page parity and different-family bounded
  adversarial/maintainability review of `683fe1d..HEAD`.
- Read-only integration inspection of `fix/footer-legibility` at `6406c47`.
  The brief requires doing this last after fixes pass, so that worktree and its
  exact conflicts were not inspected this round.

USB-powered sleep/wake, unplugged battery sleep/wake, cold power-on and exact-source
Mac ImageIO evidence remain pending. No hardware was touched and no physical
behavior or owner-acceptance readiness is claimed.

The pinned release gate passes `213c28f..52348bb` (including the round-3 commit);
its log is archived as `artifacts/newt22-r3-release-range.log.gz`. The checkpoint
is committed locally with the required Conventional Commit subject and trailer.
