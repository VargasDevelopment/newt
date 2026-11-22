# Home and Library composition latency

The isolated source baseline is `f849ffc9abdd0df825228014423e1da17354401c`,
including failed-chapter recovery `7f44c25`. The runtime checkout, device, USB,
card and private books were not touched. The approved installed toolchain was
copied locally; no dependencies were acquired or installed globally.

## Actual navigation path

Home Previous/Next changes selection and reaches `paintHome` then
`paintFrostedHome`. Ordinary builds compose both complete gray planes, rotate
them and upload 104,544 bytes before the blocking Direct gray activation.
Approved focus changes include gray artwork and antialiased label-weight
changes. A monochrome fast waveform would change those pixels. Existing
regional builds use the binary cursor island when eligible, with their existing
cleanup and fallback policy. Those display semantics remain unchanged.

Library Previous/Next changes the selected catalog index; it does not rescan
the directory or open a book. It already uses fast monochrome refresh except
when gray departure, orientation, explicit refresh or the nine-fast-paint
cleanup bound requires a full refresh. Each repaint previously reconstructed
418,176 base pixels and read each visible thumbnail twice in tiny scanlines.

## Focused changes

The tiled immutable base now expands eight pixels into one packed frame byte.
The same operation restores footer rows. This removes per-pixel tile lookup,
bit read/modify/write and a redundant frame clear. It adds no asset, cache,
framebuffer or heap allocation. Foreground composition is unchanged.

Prepared thumbnails now validate CRC during the one drawing pass. `begin`
admits the exact size/header/stat-bound stream; every row, including the final
CRC check, must succeed before artwork is accepted. Existing renderer rollback
clears the whole partial cover on failure. It adds two 32-bit CRC fields to the
short-lived adapter and removes the preliminary payload scan and seek. No
catalog, thumbnail, file or filename pointer survives rendering. There is no
reading/admission/progress/source-lifetime change.

## Host measurements

`python3 -B tools/cursor-latency-benchmark.py --candidate a91096e9b33ff382fc3b38090ace036818d42908`
compiles the baseline and candidate production paths at `-O2` with the project's
host compiler and the gate's tool flags (`tool_compile` in `tools/host_common.py`).
The figures below were recorded with Apple Clang.
It uses authored temporary fixtures, five process samples, 40 iterations per
sample and median timings. It compares
235 complete packed frames across Home planes/styles/focus/battery/runtime
metadata and Library selection/page/message/loading/missing/corrupt cover cases.
Both streams have SHA-256
`b17289aff7c6096eebe422e30768d3ff8506652a1aee59f3d17fd783f7f07cc7`.
The supporting Library sources are unchanged in this diff. The benchmark
extracts each revision's own headers and renderers from Git. Without
`--candidate` it measures the working tree, whose later intentional footer and
Home title pixels correctly fail identity against this baseline.

| Work per repaint | Baseline | Candidate | Reduction |
| --- | ---: | ---: | ---: |
| Home two-plane composition | 1.903 ms | 0.617 ms | 67.6% |
| Monochrome system base | 0.695 ms | 0.073 ms | 89.4% |
| Library four-cover composition | 2.242 ms | 1.325 ms | 40.9% |
| Cover reads | 1,508 | 756 | 49.9% |
| Cover bytes | 27,184 | 13,648 | 49.8% |

The exact recorded samples are in `artifacts/cursor-latency-host.json`. Host CPU
timing is not device response timing; SD and panel costs can dominate. Ordinary
Home still uses full Direct gray, and Library still rereads visible cover data
once per repaint. Further waveform or retained-frame caching work needs its own
display correctness and resource evidence.

## Verification and resources

The full host gate covers canonical approved Home/header/footer/wordmark,
every-pixel selector rotation, Home regional fallback/status/cleanup, sleep,
reader recovery and Library rendering. Cover tests additionally assert one-pass
I/O counts, exact placeholder pixels on CRC failure and header/early/last-row
short reads, handle closure and recovery to valid artwork. One fresh independent
adversarial and maintainability review ended clean; it independently checked all
256 source bytes, sanitizer cover tests and baseline frame equality.

All four `tools/firmware-build.py --clean --health-report` variants pass the
existing resource ceilings and linked NVS/sleep guards. No ceiling, policy,
selected stack entry, boundary or generated exclusion was relaxed.

| Variant | SRAM bytes (unchanged) | RTC bytes (unchanged) | App bytes | App reduction |
| --- | ---: | ---: | ---: | ---: |
| default | 133,264 | 300 | 1,240,464 | 48 |
| dev-usb | 134,080 | 300 | 1,255,712 | 48 |
| regional | 133,776 | 300 | 1,244,288 | 48 |
| regional-dev-usb | 134,592 | 300 | 1,259,568 | 32 |

Individual C3 frames are 304 B for Library rendering (unchanged), 448 B for
cover admission (formerly 480 B), 16 B for row reads (unchanged), and 16 B for
packed base expansion. These do not measure call-chain/task-stack/heap peaks.
The committed per-variant resource receipts bind the actual source and tools.

## Integration contract

Cherry-pick the source change onto the runtime owner's reviewed recovery head;
reconcile only `src/frosted_home.cpp`, `firmware/library_covers.h`,
`include/newt/library_cover.h` and `tests/library_cover_test.cpp` if those paths
have changed. The benchmark and documentation are host-only. No edits were made
to `firmware/main.cpp`, Home display/policy, chapter or battery recovery files.
The parent-message tool could not deliver ownership/findings updates, returning
`An earlier turn submission is not yet confirmed`; overlap acknowledgement
therefore remains pending before runtime integration.

Re-run the host gate and all affected clean native variants after integration;
resource receipts from this isolated source do not cover subsequent source
changes. The runtime owner retains exclusive device ownership and must measure
button-to-visible timing, SD costs, cursor clarity, gray preservation, ghosting
and cleanup, including Home/Library transitions and successful book opening,
return, sleep and recovery. This source work grants no installation or card
write authorization and supplies no hardware acceptance claim.
