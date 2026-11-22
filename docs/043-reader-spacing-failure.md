# Reader page rejection from Unicode spaces

The selected book's first Next rejected a page because `SelectingSink::onPage`
requires font coverage. Its first uncovered character was U+200B ZERO WIDTH
SPACE; accepting that exposed U+2003 EM SPACE on the same page. The
`PhasedReader::layout` glyph check produced `Unsupported`. This was a font
coverage failure, with no evidence of allocation or checkpoint corruption.

Source `29fffd243c63c0b75f19df33c247782aa2b8136b` handles these two characters
in `ReaderFont`: zero advance for U+200B, one clamped em for U+2003, zero
kerning and no ink for both. Semantic text and character anchors remain intact.
The bitmap atlas, parser, SDK, admission, recovery and protocol are unchanged.
Other unsupported characters retain explicit rejection.

The exact selected book was available in its existing immutable import archive.
The initial project-copy and mounted-card search missed this receipt source;
the earlier unavailable-input conclusion in the historical integration report
is superseded. Only the specific receipt entry was read. Its full digest was
matched to the current selection and verified before and after the private
host probe. No book content or identity is included in this repository.

The old production source reproduces `Unsupported` at spine 3, character 905
in both owned and borrowed native-parse paths. With the fix, first Next reaches
character 1309; three acknowledged forward pages cross a chapter, then exact
reverse returns to 3/905 with identical original frame pixels in both paths.
This is host evidence, not physical acceptance. Authored regression coverage
also exercises both orientations and both progress-bar settings. The original
341-glyph atlas proof remains exact; the two spaces have separate metric,
kerning and no-ink controls. The full sanitizer host gate passes.

A fresh post-failure 20,480-byte NVS capture confirms committed Reading intent
acknowledges the newest valid progress at spine 3, character 905. Selection,
CRC and record formats agree, with no newer unacknowledged progress or orphan
Newt chunks. Landscape, slim-bar and five-page refresh preferences survive.
No persistence restoration occurred. A separate normal return and read-only
app observation verified the still-installed old `1feff5199636` build at ready
Home. Raw capture and reports remain private.

Native measurements, exact replacement image and review receipts are recorded
in `artifacts/m1-reader-page-failure.json`. Parent authorized the exact replacement.
The first attempt failed before any flash read/write, closed serial and stopped.
After separately authorized connection diagnosis and a bounded procedure review,
the replacement installed successfully at 2026-10-03 18:07:40 UTC. Fresh agreeing
full-flash reads preceded the APP1-only write; full readback matches the image
and padding, and every outside byte is identical, including NVS. No persistence
restore occurred. The exact build booted at ready Home after normal USB return
(reset reason 11). This is not cold or battery-only resume evidence.

The installed device then completed an independently evaluable early navigation
stage and the shortened reader journey, with no duplicate navigation suite.
Navigation used 20 presses, opened no reader and returned Home. The following
saved Read opened the original private selected identity at spine 3, character
905. First Next reached 1309, three forward pages crossed to spine 4 / character
0, and exact reverse returned to 3/905. Two saved-page reopens succeeded; both
landscape 792x528 reader body captures match. The 12-press journey finished at
Home with serial closed. Private book/frame/backup data remain private; the
sanitized evidence is `artifacts/m1-reader-page-failure-device-verification.json`.

Existing protocol timestamps measure the same correctness actions. Completion
means host PRESS start through validated software RENDER_RETURN, excluding the
following STATUS/DIAG. STATUS refresh includes composition, conversion, transfer
and blocking driver/BUSY completion. Neither measures physical button onset or
optical settling. Existing driver tags distinguish nine Home island refreshes
from one full cleanup; mono Library/reader tags were absent and remain unknown.
Nearest-rank quantiles below describe this small sample only.

| Navigation action / observed driver tag | n | Completion p50 ms | p95 / max ms |
| --- | ---: | ---: | ---: |
| Home focus / gray island | 9 | 578.9 | 583.9 |
| Home focus / direct full gray | 1 | 1303.0 | 1303.0 |
| Library focus, same cover group / unknown | 6 | 849.8 | 853.9 |
| Library focus, new cover group / unknown | 2 | 849.0 | 859.0 |

Saved-book opening remains slow: initial Read completed in 18.8 seconds and
the two reopens in 12.7 seconds each, despite 850 ms reported refresh time.
Same-chapter forward pages completed in 679.8 and 684.9 ms; chapter crossing
took 1035.8 ms. Per-action reader timings retain unknown waveform classification
rather than pooling ordinary and cleanup refreshes by an inferred duration.

This closes the observed spacing failure for the bounded selected-book journey.
Generalized missing-glyph/control handling is owned by a separate parent worker.
The later settings attempt verified a portrait 528x792 body against the private
host golden at 3/456, forward to 3/937 and reverse to 3/456. Ordinary landscape
Save completed, and one Next restored exact 3/905. It then stopped during the
second Reader-Settings frame transfer on a same-boot/request but changed-session
`ERROR session` response. Serial closed; no input was retried. The final live
settings mask, exposed landscape pixels and final reopen sequence remain
incomplete. This occurred below both the audit and capture deadline limits;
the session cancellation cause remains unestablished. The earlier host audit
capacity failure, no-input boot-change stop and corrected read-only observer are
preserved separately. Sanitized partial evidence is
`artifacts/m1-settings-single-connection-attempt.json`. No sleep command was sent.

The separately reviewed minimal recovery then inspected ready Home/reset11,
opened the same selected book, restored exact 3/905, saved through ordinary Home
exit, verified another reopen at 3/905 and finished Home with serial closed.
It used at most five ordinary presses and no frames or settings changes.
`artifacts/m1-anchor-recovery.json` records this completed saved-page check;
the failed final settings-mask capture remains incomplete.

Opening performance, completion of physical settings checks, actual sleep/wake,
battery/cold recovery, optical ghosting and FAT thumbnail transfer remain gates.
The prior both-orientation host regression does not satisfy those hardware gates.
M1 and a trial freeze remain unaccepted.
