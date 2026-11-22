# Library navigation preparation

NEWT-28 / #83. Source starts at `f6fba84`; implementation is `359306a`.
Hardware is untested. This candidate still needs the Lead's device checks before
owner acceptance.

A move within the visible four-book shelf now preserves its cover pixels in the
existing logical framebuffer. Shared C++ repaints the old/new focus, tile labels,
selected title, counter, footer and system header. There is no cover cache or new
heap allocation. Full shelf composition remains the path for entry, shelf
changes, warnings, background cover publication and invalid/unavailable views.

The two frame-ownership fields fit existing Library state padding and start at
zero, preserving BSS placement. Browse entry and non-navigation actions discard
ownership. A shelf crossing invalidates it even if another queued press returns
to that shelf before paint. The paint result says whether covers were composed;
a focus paint cannot acknowledge the idle producer's deferred new artwork.
The SD adapter and focus painter are out of line. Focus-only scratch is absent
from full shelf rendering; selected stack measurements remain unchanged. Slot
geometry, focus sizes, counter and footer have one implementation for both
paths. All four lanes measure `paintLibraryUi` at 32 B (formerly 224 B) and
`renderLibraryUi` at 240 B (formerly 304 B). Individual frames do not prove
call-chain or task-stack safety.

The pinned X3 UC8253 driver has no `displayWindow` override: `PanelDriver` falls
back to a full-frame Fast display. This change therefore keeps the existing
blocking FAST/FULL display path and shared UI cleanup budget: nine FAST paints
before FULL. Focus paints still spend that budget. No faster waveform or optical
result is claimed.

Clean measurements at `359306a`, in bytes:

| Lane | App image | Growth | SRAM | RTC | paintNonHomeScreen stack |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,168,352 | 1,504 | 133,232 | 300 | 80 |
| dev-usb | 2,308,512 | 1,488 | 134,080 | 304 | 96 |
| regional | 2,175,056 | 1,504 | 133,728 | 300 | 80 |
| regional-dev-usb | 2,317,200 | 1,472 | 134,576 | 304 | 96 |

SRAM, RTC and every previously selected stack measurement equal the base reports.
One reviewed flash audit record (47) retains records 1–46 and all protected
budgets. Its exact raw reports are `artifacts/newt28-raw-code-health-*.json.gz`.
Fresh Opus 5.5 high adversarial and maintainability review ran twice. Round 1
was correct with three low stack/layout/comment findings; `359306a` fixed them,
and the fresh round 2 returned CLEAN. Both reviews are committed under
`artifacts/newt28-review-r*.md`. All four final clean health lanes passed on audit commit `fa0850f`, with clean
source identities and the exact image sizes above. Their resource, NVS guard
and firmware identity receipts are committed in `artifacts/`; the final native
log is `artifacts/newt28-native-final.log.gz`.

Host evidence lives in `/var/tmp/newt28-evidence/`. Eleven complete rendered
frames match the actual `f6fba84` Library and Frosted renderers byte for byte,
including shelf crossings, a sparse shelf and changing battery labels. PBM and
PNG pairs are named `baseline-*`, `after-*` and `reference-*`. Only authored
fixtures are used. The new regression exercises valid/missing covers, failed
rows 0/100/187, batched input, reversal, warning removal, fresh art, reentry and
unavailable views. Disabling incremental painting fails its cover-I/O assertion.
Within-shelf navigation performs zero cover opens and zero scanline reads.

Host timing is preparation only, with authored in-memory covers and sanitizers;
it does not model SD latency or panel refresh. Across seven matched runs, median
preparation was 2,375 us on the base, 2,407 us for a new full render and 372 us
for a focus paint (80 alternating moves per run). See `navigation-r2.log` and
`host-timing.json` for seven matched runs and their medians. The
existing DEV USB Reader action Composition and BlockingDisplay telemetry stays
around the real firmware path so the Lead can measure both separately.

Verification passed: focused UI, Library regression/negative control, structural
ratchet, quick and full Linux gates (ASan/UBSan), the complete 24-fixture turn-cost
matrix (3,456 observations, 1,152 cells), 18-fixture every-page parity (13,408
comparisons), and the
pinned release-history gate. The Linux profile omits arbitrary ImageIO decoding;
the existing exact-source Mac cover gate remains separate. The post-review full
Linux run also passed (1,499.993 s).

The Lead should compare USB key round-trip and Reader action preparation within
a shelf against the 345–370 ms preparation baseline, separately from the roughly
481 ms display baseline. Also check both shelf directions and the last sparse
shelf, repeated focus moves across FULL cleanup, idle-produced covers, missing
or damaged covers, Home/book/reentry and failed/cancelled opening. Check ordinary
page turns, Home and heap/stack telemetry; retain the 50,468-byte heap floor.
`device-checks.txt` in the evidence directory has the detailed checklist. No
physical, battery, optical or runtime heap acceptance is claimed here.

If the card is removed, a same-shelf move retains already displayed art until
a full paint. No read is needed to preserve those pixels; opening still uses
the existing storage validation and failure path.
