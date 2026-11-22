# Unbuffered SD book source

Reader admission requires the external reader allocations plus the unchanged
32 KiB XML and 32 KiB runtime reserves. On the C3 build, the borrowed-parse
allocations are 59,897 bytes at `04b49c0` and 62,676 bytes at `0732a7e`. The
increase is the 3,571-byte heap `SelectionSlots` owner, minus 792 bytes that
left `ExternalStorage`. The free-heap requirement therefore moved from 125,433
to 128,212 bytes.

On the owner's X3, Home had 133,456 bytes free before saved Read. Stock
`0732a7e5def6-regional-experiment` failed with `Reader route: terminal=3`
(SessionUnavailable). Afterwards it reported 127,796 bytes free while the
library source was still open. The installed `04b49c0` image had opened the
same book from the same Home heap.

Arduino `SD.open` wraps a stdio `FILE`. With `st_blksize == 0`, the framework
calls `setvbuf(..., 4096)`, and the linked newlib `setvbuf` allocates that
buffer immediately. It stays resident while a book is open. FATFS keeps its own
per-file buffer of `FF_MAX_SS` (4 KiB, sized for wear-levelled flash), which
caches one 512-byte SD sector.

`SdLibrary` now opens sources through `SdSourceFileImpl`, an Arduino
`fs::FileImpl` over a read-only POSIX descriptor. Opening walks the path once.
Size and cursor come from the descriptor. A failed read or seek leaves the
cursor unknown, so the next request seeks. Writes, directories and relative
seeks fail. `FileBookSource<fs::File>` and the development `ReaderProfileFile`
keep their types, so their selected stack keys are unchanged. The reserves,
admission rule, arenas, selection slots, receipt policy and full-SHA identity
are also unchanged.

The tradeoff: stdio used to merge smaller reads into 4 KiB refills. Now every
request goes straight to FATFS:

- Revision-authenticated reads are already whole 4 KiB blocks and are
  unaffected.
- FreeInk's 22- and 30-byte ZIP headers, 1 KiB directory and 2 KiB inflate
  reads, and the strict fallback's 1 KiB hash chunks now make more,
  smaller SD transfers.

The isolated latency effect is not measured.

On a preserving install of `e241d97fd5eb-regional-experiment`, saved Read
admitted with `free=131464 required=128212` and opened the selected book at
its saved page through the warm receipt route.

`tests/sd_source_file_test.py` covers the handle through counted syscall
seams. The SD lifecycle, open-phase and stable-integration fixtures run the
real adapter through a host model of the pinned Arduino `File`/`FileImpl`
forwarding. Host `fstat` reports live sizes and directory modes. FATFS instead
rejects directories at open and reports the size seen at open. Host tests do
not prove device heap headroom, fragmentation or latency.
