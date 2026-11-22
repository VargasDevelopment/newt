# Quick start (NEWT-83)

A new card shows a few short steps before Home, once. Settings' last row,
Quick start, runs them again.

## The steps

| Step | What it does |
|---|---|
| Welcome | The Newt sleep-screen art. Start, or Skip quick start. |
| How do you read? | Upright or Sideways. Picking saves the reader orientation, the same preference a book's menu saves, and moves on. |
| Your time zone | Opens the existing zone list. A pick saves the zone, as Settings does, and moves on. |
| Wi-Fi | Set up Wi-Fi restarts into update mode's Wi-Fi networks. After a verified Save it checks online at once, in the same restart; Back from that check returns here. With a network saved, the rows are Next and Change Wi-Fi. |
| Your books | How to copy books. Instructions only. |
| You're all set | Go to Home. Says where to run it again. |

Every step can be skipped. Back goes to the previous step. Back on Welcome
does nothing on a first run; on a run Settings opened it returns to Settings.
Settings' row opens a run only once its Welcome record verifies; otherwise
the row says "Could not confirm the save. After a restart it may change."
The steps are one ordered list, `kQuickStartOrder`
(`include/newt/quick_start_state.h`). A new step, such as a language, takes a
new id and a place in that list.

The screen and its presses are shared by the firmware and the host preview
(`src/quick_start.cpp`, and `pressQuickStart` in `newt/quick_start.h`, which
owns every transition and its commit). The firmware wiring is
`firmware/quick_start_runtime.h`; the card record is
`firmware/quick_start_store.h`.

## Once per card

The card holds the only record, `newt_quick` (`/Newt/State/newt-quick.nrs`,
NWQS v1): the step shown, whether Settings opened the run, and whether it
finished. There is no RTC or NVS copy, so the card decides in any X3.

At each boot, setup reads it after the zone, the reader preferences and the
book restore, before the first paint:

- A run in progress shows its recorded step.
- A finished run never shows.
- Without a readable record (missing or damaged), the card's history decides.
  This is NEWT-81's `cardHistory`: only a card it knows as new shows the
  quick start, from Welcome. A card that ran Newt, or no card, shows Home.
  A damaged record sits in the State folder, so that card has run Newt.
- A boot that reopens a book reads nothing; the run waits for the next boot.
- A new card whose history could not be read when the reader preferences
  loaded gets its orientation settled (NEWT-81) before the run's first
  record creates its State folder. A card still unknown here shows Home,
  and Home settles its orientation before its first note; that card gets
  no quick start, and can run it from Settings.

Deciding writes nothing. A card stays "new" until Home first records this
build (`newt_seen`, `update_seen.h`), and the quick start never shows Home
before it ends. So a power cut on Welcome shows Welcome again.

A step shows only after its record verifies: each step change records the
new step first, and Finish (Go to Home or Skip quick start) and Exit record
the run finished first. So the Wi-Fi step is always on the card before Set
up Wi-Fi restarts, and Newt comes back to it. That restart goes through
update mode's one entry (`enterUpdateMode`, `firmware/update_entry.h`), the
same as Settings' Wi-Fi row: an unconfirmed reader save keeps Newt open and
the step says so.

## Durability

Every write is one `DurableRecordStore` commit: one slot of the A/B pair,
synced and read back (`durable_commit.h`). A lost acknowledgement is
reconciled by one read-only load, only while the card vouches that what it
reads is durable. A readback of buffered bytes is never a commit.

- A step change, finish or exit that is not confirmed stays on the shown
  step and says "Could not confirm the save. After a restart it may change."
  It is never reported as unsaved. A restart finds the complete old record
  or the new one.
- The next press after that note leaves without its record: Home, or
  Settings for a run it opened. A card that cannot be written never traps
  its owner, and a later boot may show the quick start again.
- A damaged record of this schema is replaced by the next write
  (`kReplaceDamaged`), so a damaged card can still finish. A record from a
  newer build, with a step this build does not know, reads as damaged.

The orientation and zone choices use their own stores and rules: the
orientation through the reader preferences store in NVS, as a book's menu
saves it, and the zone through `commitTimeZone`. Each publishes only after a
verified commit and keeps the previous value otherwise, with the step's note.

## Wi-Fi and the clock

The Wi-Fi step restarts with a one-shot request, `QuickStartWifi` ("NWDQ",
`update_request.h`). Update mode opens Wi-Fi networks with the card's offers
read, as the Update list has them. After a verified Save it starts the same
online check the list's Check starts (`checkAfterSave`). A check that finds
a newer Newt shows it on the list, ready to install. Back leaves update mode,
and Newt resumes the quick start on its Wi-Fi step.

On that step, "the clock is set" is the Home clock's own read for that paint:
the time the X3's clock holds after the check. The Check sets the clock once
#268 lands; until then only another source sets it. Whether a network is
saved comes from the card's sealed Wi-Fi record, read without its key, on the
Wi-Fi and Done steps only.

## Memory

The quick start holds no RAM of its own, before, during or after a run. Its
state is 5 bytes in `AppState`'s padding: Home's focus and the sample's page
became a byte each, and `AppState` stays 24 bytes. Records are read and
written on the stack, only while the quick start shows or decides.

## Tests

- `tests/quick_start_test.cpp`: the shared order, Back, skips, zone list,
  Settings' row, commit-then-publish and where it ends.
- `tests/quick_start_firmware_test.py`: the real runtime, store, update entry
  and `NvsProgressBackend` routing (card records to the card, preferences to
  NVS) with main.cpp's setup, preferences, dispatch and paint lines, through
  cold boots that discard every global. New and existing cards, no card,
  resume on each step after a power cut, a reopened book, skip on each step,
  card swaps, a card unknown at load, failed, lost and unsynced commits, an
  unwritable card, a damaged record, the Wi-Fi restart and its reader-save
  guard, and a rerun from Settings. Negative controls include a step shown
  before its record, the record routed to NVS, the update entry's guard
  removed, and the orientation settle removed.
- `tests/update_screen_test.cpp`: only the quick start's verified Save checks
  at once, and Back from that check leaves.
- `tests/wifi_setup_test.py`: the real update mode, entered with the quick
  start's request, checks online after a verified Save in the same restart.

Host tests are not hardware evidence. The owner's check is a fresh card in
an X3.
