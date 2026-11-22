# Read-only SD source and compressed EPUB evidence

This slice adds an explicitly invoked Arduino SD adapter and bounded sorted
catalog for `/Newt/Books`. It does not connect that catalog to Home or replace
the embedded fixture reader. The source card, source backup, and device were
not accessed for writes during this implementation.

`FileBookSource<File>` implements the pinned FreeInk `BookSource` seam without
retaining container data. The application owns an immutable read-only file;
the source refuses empty, directory, closed and over-64-MiB files, and rejects
changes reported by the handle's `size()`.
It checks arbitrary 64-bit offsets before narrowing to Arduino's 32-bit seek,
clips reads to EOF and 4 KiB, and reports failed seeks or unexpected zero reads
as errors. Short reads are supported by the pinned engine. Arduino's read-only
file handle caches its file size, so external mutations of any length may be
undetectable by this adapter; the owner must enforce immutable contents for
an open session.

`LibraryCatalog` keeps at most 128 filenames of at most 95 bytes in a fixed
12,304-byte host object. It skips hidden files (including AppleDouble),
directories and non-EPUBs; rejects invalid paths, overlong filenames and
duplicates; and reports capacity failure without exposing an incomplete
catalog. Sorting is ASCII-case-insensitive byte ordering, independent of
directory enumeration order and locale. Metadata and books are never written.
The directory scanner uses the mounted filesystem's POSIX directory primitive
so read failure remains distinct from EOF; Arduino `openNextFile()` silently
conflates those conditions. Failed scans publish no partial catalog.

`firmware/sd_library.cpp` uses already installed Arduino SD/SPI, with read-only
unbuffered descriptor opens ([Decision 049](049-sd-source-descriptor.md)) and
`format_if_empty=false`. It reads the selected official X3 board profile,
deselects display/card, enables the existing SD rail and
mounts at 4 MHz on their shared SPI bus. It is opt-in and has no call from
`main.cpp`. The caller must release any book/session before scan/open/close,
because those operations invalidate the previous source. Mounting and card
presence have not been tested on hardware. No SdFat dependency was acquired.
Mount requires the caller to own an idle bus with no concurrent SPI operation.
It ends and reinitializes the same SPI object after deselecting both devices:
Arduino otherwise retains the display's original MISO-less bus configuration.
The host regression simulates display-first initialization and verifies that
SD MISO is attached; it also injects a directory I/O failure after one book to
prove incomplete scans are rejected.

The authored deflated test fixture exercises actual miniz/Expat/layout through
the same source template, including 31-byte short storage reads. ASan/UBSan
observed peaks: metadata 694 bytes, parse 45,824 bytes, layout 48,328 bytes,
transient Expat payload 10,024 bytes. The existing fixture renderer and budgets
are unchanged.

An explicitly invoked private host probe opened all 61 production EPUBs and
stream-parsed all 2,859 spine documents using the unchanged FreeInk pin
`cd6f5b831c8fad899723b6d406586f00c575f021`. Only aggregate counters are recorded
in `artifacts/sd-library-probe.json`; filenames, titles, text, progress and book
bytes are excluded. Reproduce with an explicitly supplied read-only input:

```sh
python3 tools/probe-library.py --books /private/source/Books --output artifacts/sd-library-probe.json
```

The largest observed resident book arena was 46,526 bytes, parse arena 45,824
bytes, Expat transient live payload 15,368 bytes, and largest Expat allocation
4,096 bytes. Source requests never exceeded 4 KiB. All XML allocations were
released after each book. These are host ABI measurements of arenas and
requested parser payloads, excluding allocator metadata, filesystem, runtime,
framebuffer, and fragmentation. Successful metadata/XML parsing does not
establish rendering or device behavior.

The probe's 64-KiB book and 56-KiB parse capacities, plus the existing 48-KiB
layout capacity, 12-KiB catalog and observed parser heap exceed credible C3
headroom. Existing guarded hardware evidence records 169,664 bytes free at
Home and a largest block of 114,676 bytes, before the 70,088-byte fixture
workspace; that evidence does not measure this new adapter. This is why the
runtime fixture remains unchanged. A subsequent
library reader should use exact bounded sizing or the pinned SDK's SD-backed
`BookCatalog`/page caches, separately allocated workspaces, incremental work,
and measured device heap headroom. The printable-ASCII font is another
independent limitation: production Unicode coverage remains required.
