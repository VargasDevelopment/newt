# NEWT-41: slow action feedback

A press that waits on storage or the book now shows a working frame before
the work starts, so a long wait never looks like a missed press. The book
check that runs after an open is silent again; only its outcome is shown.

## The working frame

`acknowledgePress(SlowWork)` (`firmware/press_acknowledgement_runtime.h`) is
the one owner. It paints once per press, before the press's work.

One press record (`PressState`) runs from the press's acceptance to the paint
that shows where it led. `loop()` begins one for every consumed input, setup
begins one for the boot open, and a changed book's background reimport begins
one when no press is running. Nothing else starts or resets it. Nested work in
the press (Library → `openReader`, Settings Save → `openReader`, the reimport
of a book the open found changed) reads the record but cannot answer again.
The record ends at the next `paint()` or the next press.

When an open finds that the book changed, the loop does not paint the
unopened reader. The idle reimport continues the same press: one working
frame, then the new book's page.

| Work | Shown | Copy |
|---|---|---|
| `FirstOpen` (no import receipt, or a changed book's reimport) | the portrait system screen `renderBookOpeningUi` | "Opening your book" / "Getting your page ready. A book's first open takes a little longer." |
| `Opening` (every other owned open: Home, Library, wake) | `renderReaderWorkingUi`, one card in the reader's orientation | "Opening your book…" |
| `Saving` (Back or Menu → Home while the book check is still pending; [093](093-back-paints-home-first.md)) | same card | "Saving your place…" |
| `Applying` (reader Settings Save) | same card | "Applying settings…" |
| `Loading` (a turn held by the foreground book check) | same card | "Loading page…" |

The host preview runs the real firmware, so it draws the same frames; its
`--working`/`--opening-preview` switches are not ported (issue #357).

A failed or cancelled press still replaces its working frame: `loop()` marks
any acknowledged press as changed, so its result screen is painted in full.

## Refresh rule

The panel has no windowed refresh, so the frame costs one whole-screen pass.

- Leaving Overlay Home: the inverse drive plus one fast pass (a cleanup, as in
  [Decision 055](055-overlay-home-refresh-policy.md)).
- From a valid black-and-white receipt: one fast pass, in either orientation.
  The card has little ink, and the next screen decides its own cleanup.
- Otherwise (Direct gray, or an unknown panel at boot): a full refresh.

The pass is credited once, when the press ends, to the budget of the screen
then shown: the reader's `ReadingRefreshPolicy` while Reading,
`UiRefreshPolicy` otherwise. By then every budget reset on the way has run
(`openReader` resets the reader budget, also when nested in a Library open or
a Settings Save), so a cleanup frame counts as the new page's initial full and
the page follows as a fast pass. A fast frame leaves the initial full in
place. Paints inside a press (a changed book's reimport, the book check's
outcome, a timed-out turn) let these checks decide rather than forcing a full
refresh; only the halt screen and the sleep image force one. Every open
therefore completes exactly one full or cleanup pass:

| Start | Frame | First page |
|---|---|---|
| Overlay Home | drive + fast (cleanup) | fast |
| Library, or a reader menu | fast | full |
| Boot / wake (unknown panel) | full | fast |
| Direct gray | full | fast |

The 20 s open budget restarts after the frame, so the paint is not charged to
the open.

## The silent book check

The footer no longer replaces the chapter label with "Checking book..." while
the background check runs. "Book changed. Reopened." and "Book changed. Reopen
required." still take its place, and the failure and timeout notices are
unchanged.

Input is not ignored during the check. Idle steps yield to every press.
Turns within cached pages proceed. A turn past the cached pages waits for the
foreground check (Back or Power cancels it, within the 20 s budget); that turn
now shows "Loading page…" first.

## Measuring

DEV USB builds print one line per acknowledged press:

```
Reader ack: work=<SlowWork> us=<press to frame> refresh_ms=<panel pass> cleanup=<0|1>
```

`us` runs from the press's consumption to the end of the frame's blocking
refresh; on wake it runs from the boot open. `us=0` means no action was being
timed. `work` is 0 FirstOpen, 1 Opening, 2 Saving, 3 Applying, 4 Loading.

The aim is a visible answer within about 300 ms. The panel starts changing
when the pass starts, but `us` counts to its end, so a fast pass reads about
400 ms (`refresh_ms` near 390) and a full pass about 700 ms. Subtract
`refresh_ms` from `us` to get the time before the pass began.

## Not covered

- Home → Books (about 1 s). A frame would make it slower.
- Uncached chapter crossings and chapter-menu jumps. No cheap predicate says
  in advance that a turn will be slow, and the frame is lent during the turn.
- Preparing the chapter menu.
- Device Settings actions; they are expected to be fast.
