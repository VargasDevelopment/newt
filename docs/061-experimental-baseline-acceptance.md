# Experimental X3 baseline acceptance (2026-10-12)

This is the current owner-acceptance record for the frozen installed image.
Decision 060 retains technical and historical measurements; its historical
pending/rejected statuses do not supersede this record. The candidate artifact
mirrors this image-bound acceptance for machine readers. Future status updates
must keep that mirror consistent rather than reclassifying historical evidence.

## Exact image and owner evidence

The owner adopts installed source `ea4570aff5ad09199f8545acb825ca8d5086b84d`
(`ea4570aff5ad-regional-experiment`) as the working experimental baseline.
The 1,619,664-byte image SHA-256 is
`814b202760f3031356e82620a5a89966bb91a2351da5b1e0f8aacf2edc58481d`.
This acceptance applies to that installed image on the owner's cracked-screen
X3; it is not a release, a different image's hardware evidence, or formal
reading-latency admission.

On October 12, the owner reported approximately 24 hours of actual reading,
including repeated sleep/wake while unplugged. The device faithfully retained
the reading position and the new Home UI. **Unplugged battery sleep/wake,
reading-position preservation and the installed Home UI are owner-accepted.**
Do not classify these as untested or request repetition merely to generate
instrumentation. The report is owner evidence, not an agent-observed reset trace.
Cold power-on remains a separate, unconfirmed condition.

Earlier owner feedback described greatly improved Home navigation, a new book
opening cold in approximately three seconds, snappy page turns, satisfactory
chapter crossings and navigation around the device. Those observations remain
informal estimates and usability judgments, not controlled latency samples.
The further defects recorded in #41, #42, #33, #35, #4 and #22 remain open.

## Scoped heap exception

The owner explicitly accepts the reported **50,468-byte boot-lifetime minimum
free internal heap** as an exception for this experimental baseline. The original
65,536-byte requirement **did not pass**. This does not establish a new universal
minimum, a safe largest-contiguous-block bound, coverage of every execution
path, or acceptance of a future regression.

Do not chase 65,536 solely to satisfy the number. Preserve allocation reserves,
fragmentation checks, transactional workspaces, cancellation/failure behavior,
all protected RAM/stack budgets and the independently enforced cover reserve.
Historical measurements and resource receipts are unchanged. Reconsider this
exception if a real allocation failure, fragmentation problem, declining
headroom or materially different workload appears; diagnose the concrete
failure rather than relaxing a guard.

The Mac ImageIO/renderer gate remains explicitly deferred and **untested**.
`docs/reading-latency-policy.json` remains unchanged, with no approved baseline
or review. Formal latency admission and issue #22 remain open. The working
experimental baseline is not a passing formal performance baseline or complete
cross-platform engineering acceptance.

## Installed source versus repository work

Before this acceptance pass, stack head
`3890e3392277855b7c441d5189207e44c5485ce0` differed from installed `ea4570aff5ad`
only in 16 documentation/evidence files: the two combined candidate artifacts,
four code-health reports, four firmware-identity reports, four linked NVS guard
reports, Decision 060 and the Stage implementation record. Firmware, shared
renderer, tests, SDK inputs, generated assets and build configuration were
identical. These later files record the frozen image's evidence; they were not
compiled into that image.

This repository-only pass records acceptance/review and authorizes the planned
merge sequence after the required repository checks and review reconciliation.
It neither installs a new image nor transfers the owner's device evidence to
different firmware code. Frosted remains the product default and fallback;
Stage stays an explicit experimental opt-in. Keep the tested firmware and both
complete 396-sector rollback images in the external recovery archive. No device
access, flash, card operation or reading-position change is part of this pass.

## Focused follow-up batch

1. **Lost/clipped text and separators (#35, #4).** Reproduce italic overhang,
   line/page-edge clipping and missing scene separators with authored EPUBs.
   Verify the original code points, markup and resource representation before
   assigning a cause. Preserve source coordinates, anchors and malformed-UTF-8
   rejection. A fallback marker does not satisfy the owner's full glyph-fidelity
   request. Exercise regular/bold/italic/bold-italic, both orientations, cached
   and freshly laid-out pages, and page boundaries.
2. **Indentation in #35.** Compare resolved EPUB markup/CSS, including text-indent,
   paragraph boundaries and list rules, with the rendered result before deciding
   the indent is wrong. Another reader's appearance is a reference, not source
   authority. Include intentionally indented and intentionally flush paragraphs.
3. **Canonical chapter identity (#33).** Use one book-facing resolution for Home,
   Reader and the chapter menu. Cover front matter, split spine entries and
   fragment targets; preserve bounded metadata reuse and saved-position behavior.
4. **Battery text spacing (#42).** Add target-scale shared-renderer regressions
   across one/two/three-digit percentages and changing digit counts. Keep glyph
   measurement, paint and erase consistent; do not infer a battery-value defect.
5. **Missing covers (#41).** Diagnose supported-cover discovery, generation and
   cache behavior separately from unsupported image formats. Exercise never-opened
   and previously opened books, interrupted work and transient I/O with generic
   fixtures. Do not assume PNG/progressive JPEG is the reported cause or silently
   broaden decoder scope. Carry the review's deferred cover limitations forward.
6. **Performance (#22).** Preserve the existing measured image-bound evidence and
   separate informal satisfaction from the contract's actual admission procedure.

All six issues stay open through the baseline merge. Close an individual defect
only with a linked fix, appropriate regression evidence and targeted verification
on the exact subsequent installed candidate. This is a plan, not implementation
or authorization to take over the reader during this repository-only pass.

## Next combined firmware-validation session

Repository checks and review reconciliation require no device. Combine the
following genuinely physical work with the next owner-authorized candidate
session, rather than interrupting today's reading:

- Cold power-on persistence, distinct from the accepted unplugged sleep/wake.
- Targeted optical checks for each actual follow-up fix: clipped italic/text,
  separators/glyphs, source-correct indentation, chapter labels/destinations,
  footer spacing and representative cover lifecycle cases.
- Required formal installed-reader journey and latency admission, including the
  remaining phase-specific publication/cancellation evidence and intermittent
  USB diagnostic-tail investigation; software results are not optical timing.
- Instrumented sleep-path stack coverage and supported powered-down card
  removal/reinsertion, which are not established by the accepted reading report.

No repetition of the accepted battery sleep/wake or position/Home behaviors is
required for this baseline. A subsequent code change in those paths may warrant
a focused regression check of the new candidate, with its own image binding.
