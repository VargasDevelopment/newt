# Static Gaussian-style background: host feasibility

This is a host-only experiment on `probe/host-gaussian`, based on preserved firmware checkpoint `550b43c`. No firmware source/configuration, hardware, panel state or dependencies changed. Run `python3 tools/blur-probe.py` with the existing tools. It renders the original synthetic EPUB page, compiles a standalone host harness with ASan/UBSan and `-O3`, checks filtering, then writes diagnostic comparisons and [measurements](../artifacts/blur-probe-results.json).

**Spatial blur and output quantization are separate.** The harness filters first, then produces continuous 8-bit host diagnostics, four-level scalar output, and one-bit Bayer output independently. The texture in the one-bit image comes from quantization, not from the Gaussian filter. Continuous images show the intended spatial softness; they do not promise continuous tones on X3.

| Filter | Explicit scratch | Weighted filter taps / page | Host blur median | Host blur + scalar four-level median |
| --- | ---: | ---: | ---: | ---: |
| Separable box, 5 taps | 3,764 B | 4,181,760 | 1.29 ms | 1.05 ms |
| Binomial, 9 taps | 5,876 B | 7,527,168 | 1.08 ms | 1.14 ms |
| Half-resolution binomial, 9 taps | 3,762 B | 1,881,792 | 0.81 ms | 0.91 ms |
| Gaussian, 25 taps | 14,324 B | 20,908,800 | 14.59 ms | 14.66 ms |
| Half-resolution Gaussian, 13 taps | **4,818 B** | **2,718,144** | **1.02 ms** | **1.11 ms** |

These are 21-run medians on this arm64 Mac with Apple Clang 21. The reproducible JSON contains exact latest medians/ranges; normal timing variation is expected. All cases read 418,176 source pixels. Half-resolution cases add 2,509,056 bilinear weighted terms plus output quantization/writes. Counts are work categories, not instruction or cycle counts. Mode-specific checksums/code generation differ, so do not subtract medians to infer isolated quantization overhead. No packed grayscale planes, SPI, display waveform, file/PNG I/O, allocation or source reset is timed. Host validation uses extra full arrays outside the candidate and timed region. C3 has a different compiler/processor and no measured timing here.

The proposed first candidate is **2× area downsample → 13-tap separable Gaussian → bilinear upsample → final quantization → crisp opaque controls**. Its coefficients approximate a sampled Gaussian at sigma 2 low-resolution pixels, normalized to 1,024. The kernel sigma is approximately 3.97 full-size pixels, compared with 3.93 for the 25-tap full-size kernel; resampling adds some softness. The two continuous fixture images differ by mean 0.58/255 and maximum 7/255. Integer row rounding differs from the floating reference using the same coefficients by at most 1/255 on this page. Boundary/checker and constant-image tests also pass. In-place and separate-source filtering match exactly; sanitized and optimized images match.

The fixed byte-row ring prefetches original future rows before overwriting the current packed row. It uses the **existing 52,272-byte page as a consumable snapshot**, without another full page or an eight-bit framebuffer. Re-render the saved semantic reading anchor on dismissal or cancellation. Keeping an additional untouched full snapshot would add 52,272 B and is a different budget. The earlier approximately 1 KiB box estimate used direct packed-row neighborhood sampling; this measured separable box trades more scratch for fewer repeated neighborhood reads.

The current firmware receipt leaves an arithmetic 63,736 B after linked RAM, native display frame, reader workspace and input-task stack, **before other SDK/RTOS/allocator/Expat costs**. Thus 4.8 KiB is a plausible candidate, not a measured heap guarantee. Allocate scratch explicitly outside the input task stack. Processing native 792×528 rows for four-level strip output would raise this candidate's fixed buffers to approximately 7,227 B, plus any additional packed-plane/strip storage. The host's portrait scalar grayscale output does not prove native plane encoding, baseline cleanup, cache ownership or display behavior.

## Responsiveness acceptance for any later device trial

- Separate blur CPU time, plane packing/upload and panel waveform measurements. Never interpret the host milliseconds as X3 latency or promise a two-second limit.
- Run short cooperative row batches with a measured time budget. Process Back/Sleep cancellation before committing the decorative image; queued input alone is insufficient if dispatch waits until all blur work finishes.
- In the monochrome path, computation can leave the visible/native old frame intact until commit. Cancelling destructive portrait work requires re-rendering the saved anchor. After native grayscale RAM uploads, cancellation additionally requires SDK cleanup/rebase even if glass refresh has not started.
- Compute once per page/layout/quantization identity. Repeated focus presses change only crisp controls within a fixed opaque panel; coalesce focus updates and latch one activation, without triggering repeated blur work. A persistent native four-level background/cache needs its own storage and panel-state proof; this probe does not implement it.
- Idle precomputation is a candidate only if it can reuse existing storage, invalidate on page/layout changes, and yield to input. If measured compute or refresh harms ordinary interaction, show the simple dialog immediately and omit this decoration, or reserve it for the sleep transition. Do not animate a blur through multiple waveforms.

The existing X3 grayscale paths cannot stage while busy, and their window-update API falls back to full-frame FAST. A four-level effect may reduce texture but still quantizes the Gaussian gradient; cheap dialog-only refreshes are unproven. Hardware acceptance must cover ghosting, gray-to-BW restoration, battery impact and sleep/wake. Blur is decorative and **never privacy protection**.
