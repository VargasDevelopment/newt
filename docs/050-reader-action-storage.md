# Per-action SD checks and lent action storage

Reader actions on the X3 spent most of their non-display time on SD path
lookups and on re-reading authenticated 4 KiB blocks. Each FAT path lookup
walks a directory. Before this change, every receipt, metadata or cache read
repeated it, and every page turn probed the same missing chapter cache again.

## Path checks per action

`FilePort::beginDeferredPathChecks` and `finishDeferredPathChecks` bracket one
synchronous action: open, show, repaint, Library selection, or a derived
metadata load.

- **Inside the scope**, each read lease gets one complete path check at its
  first use. Later checks of the same lease compare only the open descriptor.
- **Complete checks repeat** when a lease is released and at the scope's
  checked exit, which always runs before a result is returned.
- **A failed exit** turns an accepted reader result into `Stale` and closes the
  reader, or turns a metadata load into a miss with cleared outputs.
- **Writers and provisional receipts** never defer.
- **The tradeoff:** a path changed during one action is caught at that action's
  exit instead of at the next read.

## Lent action storage

During one synchronous action the caller lends `kFrameBytes` of storage that
nothing else reads or writes until withdrawal:

- reader actions lend the published logical frame (`publishLendingFrame`);
- Library selection lends the same frame through `SdLibrary`.

`ImportedRevision` uses the loan as a larger work chunk and as up to 11 retained
authenticated blocks. Retained bytes are copied out only for blocks verified
under the current receipt lease. Lending, withdrawal, reset, failure and
import drop every retained block. A source change to a retained block is
therefore detected at the next lease, not at the next read within this one.

After a failed render that read lent bytes, the published frame is undefined.
`main.cpp` repaints it in the same loop iteration. The DEV USB frame capture
reads it only between iterations. Decision 047's 2 KiB parser-tail scratch
declines when a loan is active, because it requires the compact 256-byte
scratch.

## Verified cache absence

`SdChapterCacheStorage` remembers two names proven absent, meaning both the
canonical file and its `.bak` sibling were missing. Names are stored as folded
16-bit FNV-1a identities, which fit the existing padding. The memo survives
metadata loads and port release on the shared port. Writing that name clears its
entry, and the session boundary clears both. A collision can only skip an
optional cache; the ordinary verified chapter is rendered instead.

## Host work evidence

`tests/reader_storage_work_test.py` counts syscalls through the production
owners on authored book shapes. It bounds path lookups per action (`next-same`
8, crossings 12, `home-read` 20, `library-fresh` 48) and checks sampled pages
against an ordinary oracle pixel for pixel. A fault arm changes the receipt
inside a deferred action and requires the action to fail closed. The
`--negative-controls` phase rebuilds the `b50e577` owners and requires them to
exceed the bounds. Host counts do not establish device latency.

## Device observations

These are one-device software completion timings from the dev-USB action trace
on the owner's X3, using the same book and protocol for both images. They are
not optical settling times. Image A is `e241d97fd5eb-regional-experiment` (main
plus Decision 049); image B is `ccba73bcd8a7-regional-experiment` (A plus this
change).

| Action | A | B |
| --- | --- | --- |
| Saved Read from Home | 5.02 s | 2.07 s |
| Chapter crossing | 3.69–3.85 s | 0.85–0.98 s |
| Same-chapter turn | 0.91–1.02 s | 0.60–0.73 s |
| Full-refresh turn | 1.20–1.29 s | 0.95–0.98 s |

1.318 s of each saved Read is the gray-to-mono display transition. Turns
include about 482 ms of panel time, or 786 ms on full refreshes.

Across other books, image B measured same-chapter turns of 0.62–1.14 s and
warm reopens of 1.74–2.03 s. Cold opens of image-heavy books remain slow:
17.6 s for a 5.6 MB, 128-member book. That is SD-throughput and
strict-admission work, outside this change.
