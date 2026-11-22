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
