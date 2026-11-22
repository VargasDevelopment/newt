# Source hardening handoff

Only this new ordinary ignored packet was written. The original page-renderer/
packet and concurrent layout worker's files were left untouched. No compiler,
test, gate, network, acquisition, device, planner, commit or subagent ran.
Existing rustfmt formatted Rust files; Python AST parsing is syntax inspection
only. Everything below describes source intent, not execution evidence.

| Finding | Integration files | Source change / pending witness |
| --- | --- | --- |
| 1, license closure | frozen/libs/font/FreeInkFont/third_party/freetype/FTL.TXT, provenance.json, NOTICE | Exact local SDK git-show bytes at cd6f5b831c8fad899723b6d406586f00c575f021; all prior frozen files/notices/digests retained. Choose FTL for this frozen source distribution. |
| 2, sparse coverage | core/page_renderer.rs, target/borrowed_caller.rs, tests/safety.rs, CONTRACT.md | Font::covers is required, with no true default. Concrete sparse adapter delegates to has_glyph; host witness queries through dyn Font for present 65 and absent 66. Metrics-only fakes must explicitly choose true. No automatic equality enforcement between arbitrary user methods is claimed. |
| 3, discriminating witnesses | tests/corpus.py, tests/mutations.json, host/main.rs, tests/oracle.cpp, PROTOCOL.md | Repeated registrations select regular fallback for missing 65 but bold for covered 66/67, preserving object identity. Clipped [100,100] diffuses before clipping. Partial [100] then [100,150] requires full placement-width row transfer. Measured 2 and bitmap 7 are separate fields. Fresh proposed controls compare full Selection for kern/ligature/result, skip clipped diffusion, truncate row transfer and ignore measured advance while preserving its call. |
| 4, rotation matrix | tests/corpus.py, tests/mutations.json | Asymmetric 3x2 coverage [60,255,0;120,0,255], full literal output for three formats times four rotations, including stride padding. A separate 12-case image matrix covers all formats/rotations too. Portrait maps logical fringe (0,0), Bayer15, to panel (0,3), Bayer240: panel indexing changes output. Actual-core Bayer-coordinate control uses this witness. |
| 5, response validity | tests/replies.py, tests/suite.py, PROTOCOL.md | Complete event signatures, per-field semantic bounds, request-specific bank/image IDs and query count, op-specific answer domains, selection membership, full frame length and canonical ASCII/LF. In-process malformed-response fixtures must raise before semantic comparison. All executable children continue through existing Run/inherited_child ownership. |
| 6, borrowed runtime caller | target/borrowed_caller.rs, target/page_renderer_borrowed.rs, tests/safety.rs, tests/suite.py | Concrete sparse font and row adapters borrow runtime inputs. Caller borrows all frame/error/arena output buffers. Host witness uses two nonempty glyphs and two decoder rows, specifies all 8 output bytes, verifies output [0,999,1,1,2,0] and decoder-written arena bytes. Nonzero arena reservation distinguishes mark restoration from zero initialization. Same concrete caller is built into the target probe, with no local owned frame/heap/static samples. |

Root integration mapping:

- core/page_renderer.rs -> rust/core/src/page_renderer.rs; export this module.
  Keep actual FontChain/PageRenderer algorithms and borrowed domains described
  in CONTRACT.md. Reconcile explicitly with the separate layout-source worker;
  this child made no layout-stage edits.
- host/main.rs -> root's actual-core host binary. Update its schema alongside
  tests/oracle.cpp, tests/corpus.py and PROTOCOL.md (bitmapAdvance is new).
- tests/safety.rs -> actual-core integration tests. Copy target/borrowed_caller.rs
  to support/borrowed_caller.rs relative to that test. The staged workspace
  generator performs this copy without changing the source.
- target/page_renderer_borrowed.rs and target/borrowed_caller.rs -> retained
  target-probe modules, side by side. Keep every public probe including
  page_borrowed_caller_runtime. Symbol sizes establish emitted code only;
  the host caller/output assertion supplies the proposed runtime witness.
- tests/suite.py, replies.py, corpus.py, rejection.py, mutations.json -> gate
  sources together. suite.py verifies frozen provenance and packet fingerprints,
  runs the five safety tests separately from parity, admits canonical complete
  replies, and records each fresh actual-core mutant receipt. These are plans
  for root execution; no mutation is claimed killed. The measured-advance
  control deliberately substitutes the fixture's bitmap-advance value 7 after
  calling measured advance; no extra rasterization trace is introduced.
- frozen/, LICENSE, NOTICE, provenance.json, fingerprints.json and the packet
  docs -> preserve with the root integration archive. fingerprints.json records
  all packet file bytes except itself; provenance.json retains frozen identity.

Pending root validation: Rust lifetimes/types, target body retention, C++
compilation/sanitizers, all literal arithmetic/traces, response rejection,
mutation unique-site/type-preservation/kill receipts, and actual-workspace
integration. The proposed 360-second runner budget is unmeasured, especially
with seven additional mutant builds; fit it to the locked parent gate budget
using measured evidence. No runtime-budget, host-parity, RV32-build, real TTF
rasterizer, PNG/JPEG decoder, firmware, device or owner-acceptance claim is made.
