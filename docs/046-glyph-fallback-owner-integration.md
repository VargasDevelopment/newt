# Glyph fallback owner integration

The runtime owner ported reviewed checkpoint
`6598fa543de1897137a9d1138991788161cee3f2` into the isolated
`fix/m1-glyph-fallback` worktree. Source commit
`d4befc97cc244a311fe3182c62749e45218deec6` follows the current device-evidence
head; all 24 changed files are byte-identical to the upstream checkpoint.
The old device-check checkout and installed `29fffd243c63` image remain unchanged.

Two fresh bounded integration reviews ended CLEAN at `158da9b`. They checked
the port, shared font metrics, strict UTF-8 validation, original captured text and
anchors, cache/source/storage lifetimes and recovery boundaries. Atlas data,
admission reserves, checkpoint formats and NVS startup-guard code are unchanged.

Private selected-book probes compiled and ran under ASan/UBSan with both owned
and borrowed storage. All 18 prior numeric golden rows are unchanged: original
3/905, first Next 3/1309, forward chapter crossing and exact reverse/body pixels;
portrait 3/456 to 3/937 and back, landscape containing 3/456, then one Next to
exact original 3/905 and original pixels. The authorized immutable archive digest
matches before and after. No book copies, private text/artwork or identity were
committed or uploaded. These are host proofs, not new-firmware hardware claims.

`artifacts/m1-glyph-fallback-owner-integration.json` records current gate status.
The full combined host gate passes with exit0 under `python3 -B tools/check.py`. The parent's exact reviewed minimal
budget-policy checkpoint `042929a42e20c69feb11042ffd5d2d40c4bc7471` is
now applied. All four fresh clean native resource receipts, linked guards and current
read-only receipt checks now pass. The exact candidate is
`9d8be8edb5e2-regional-experiment`, 1,264,048 bytes, SHA-256
`0ffae241ee647cfea41f6a9f8c941ccfd85852ff7f82e9c4118b86a08dc91ed5`.
Its canonical frozen copy is `.build/glyph-fallback-proposal/firmware.bin`
(mode0400), with 1,616 bytes FF padding in the existing 309-sector APP1 span
`[0x780000,0x8b5000)`. The stamped regional-development image measures
48 bytes above the installed spacing image, 16 bytes below the reviewed ceiling.
The source remains compiled from `9d8be8edb5e2947193fe19069fdd7dfd72f3d6c0`. The upstream flash delta is 64/48/48/64 bytes
for default/development/regional/regional-development; no RAM or stack relaxation
is authorized here. Source integration does not authorize a new flash.

The completed unchanged-image recovery separately verified ordinary saved-page
reopen at 3/905 and returned Home with serial closed. Its evidence and the
incomplete settings-mask capture are retained from the prior owner checkout.
No sleep command has been sent. The owner's stable-trial prerequisites are the
reviewed glyph fallback and battery-only recovery; opening/navigation goals stay
tracked without delaying that trial once those prerequisites pass.

The offline install preparation retains the previously successful procedure with
only five candidate constants changed. Both narrow procedure reviews are CLEAN;
20 controls and 4,643 bindings pass. Its grant remains false/null. Exact parent
authorization for APP1 installation, fresh full-flash preservation/readback and
the separate normal return is still required. No new image has been installed.


## NEWT-3 full coverage candidate, 2026-10-18

The candidate preserves the 341-glyph four-face legacy profile and adds 1,700
codepoints per face from approved, pinned OFL sources. Centered bold U+25A0
triplets now paint squares, and music U+2669–U+266F, all arrows and all mathematical
operators are covered. Lexend/Noto Sans lead; symbol priority is selected once per
Unicode block in manifest data. No source was fetched by the implementation agent.
The full source comparison and per-block/face costs are in the font README.

The Lead accepted ordinary source-bearing overflow with unchanged metrics/margins.
Memory safety, clipping disclosure and fresh/cache/repaint parity remain mandatory.
CJK, combining sequences, Greek Extended, complex shaping and embedded fonts remain
unsupported. Host checks and clean resource builds do not establish device behavior.
The full tables cost 638,817 added bytes before code/descriptors. The Lead authorized
a 700 KiB image-growth checkpoint; protected flash/RAM/stack budgets are unchanged.
No substring pooling or compact-ID format was introduced.

No device was connected, flashed or touched. NEWT-9 hardware checks and Lead review
of measured flash growth remain pending. Nothing changes earlier install authority.


Round-2 source generation uses the unified no-autohint profile (flags 32780),
which exactly reproduces every legacy glyph/metric/kerning pair in all faces.
Profile declarations are enforced in source import and stdlib-only generation.
Only the committed PNG proof is a generation input; the ignored PBM is optional.
A registered explicit-file-set regression exercises fresh generation and font
binaries without source import. No runtime format or resource ceiling changed.
