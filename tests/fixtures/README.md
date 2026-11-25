The Small Lantern is original synthetic reading material written for Newt's
bounded offline-reader proof. Its text and generated EPUB are dedicated to the
public domain under CC0 1.0. No user books, device files, fonts or images are
included.

`python3 tools/make-reader-fixture.py` generates the EPUB here and the same
bytes in `rust/firmware/sample.epub`. `--check` verifies reproducibility
without changing files. The archive has fixed dates and ordered ZIP_STORED
entries, with the uncompressed `mimetype` entry first and without extra fields.
It contains two chapters, metadata, EPUB 3 navigation and twelve short original
paragraphs. Stored entries deliberately avoid requiring a decompression window
in this first constrained memory experiment; general compressed EPUB support
must be measured separately.

`history/` holds the earlier Newt sources that compatibility, rollback and
negative-control tests compare against, copied from the named commits of the
private development history, which a fresh repository does not carry.
`history/manifest.tsv` maps each commit and path to `history/blobs/<sha256>`,
and `tests/history_fixtures.py` is the only reader. `history_fixtures_test.py`
checks the bytes and, where those commits exist, the history itself.

`apps/` holds original, invented card app definitions for the Apps offer
previews and tests: Quiet Hour uses every card action, and long-text fills
each text field to its limit. `tools/preview_driver.py --app-package` encodes
them with `tools/newt_package.py`; they are not shipped apps.

## Synthetic large-EPUB corpus (#61)

`tools/large_epub_corpus.py` generates deterministic, text-only CC0 EPUBs on
each side of Newt's current ZIP, 48 KiB bank, TOC, page-cache and file-size
limits (`#60`, `#61`). Large outputs are generated on demand into a scratch
folder (for example under `.build/`) and never committed.

```sh
python3 tools/large_epub_corpus.py --list
python3 tools/large_epub_corpus.py --out .build/corpus --subset fast
python3 tools/large_epub_corpus.py --out .build/corpus --subset full
```

Each case isolates one axis (`zip-entries`, `metadata-arena`, `central-dir`,
`member-size`, `expanded-total`, `file-size`, `zip64`, `name-length`,
`long-ids`, `toc-size`, `toc-depth`, `chapter-anchors`, `chapter-pages`, and
`long-web-novel`) and declares its expected `admission::admit` and
`PhasedReader::open_phased` statuses.

Host verification and timing measurements:

```sh
python3 tests/large_epub_corpus_test.py            # fast subset (gate default)
python3 tests/large_epub_corpus_test.py --negative-controls  # every negative control (its own gate phase)
python3 tests/large_epub_corpus_test.py --full --measure
python3 tests/large_epub_corpus_test.py --full --measure --sweep
```

To measure the same cases on a bench X3 under the device lease, generate the
wanted subset into a local folder with `--out`, copy the `.epub` files onto
the bench card's `Newt/Books` folder, and open them on the X3 while recording
serial telemetry. Host timings and 64-bit struct sizes do not establish X3
hardware behaviour.
