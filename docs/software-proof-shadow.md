# Software-proof reuse: conservative shadow phase

Operational software-proof reuse remains unavailable. Every reading-ready state
still runs `tools/check.py` freshly. The opt-in observer measures and compares
available inputs; it cannot grant readiness or omit a gate.

```sh
python3 -B tools/reading-ready.py software-verified --software-proof-shadow
python3 -B tools/reading-ready.py installable --variant regional-dev-usb --software-proof-shadow
```

The default command is unchanged. There is no reuse/enable switch. The second
command verifies the existing current native resource receipt and actual linked
artifacts after its fresh software gate. It does not compile or install firmware.
Later states retain their existing live UI rendering/approval checks, exact
installation binding, physical journey freshness and separate owner acceptance.
This phase performs no physical journey, device, SD or private-book action.

## Observed inputs

`tools/software_gate_shadow.py` reuses the existing canonical collectors:

- `input_files` and `tree_hash` bind all files, names and executable caches under
  the owned source, header, firmware, tool, test/fixture, asset, app, patch and doc
  roots, plus tracked root configuration files.
- `verify_source_pin` verifies the actual official SDK checkout; the inventory
  additionally hashes its non-Git files.
- The existing compiler collector records host launchers/backends and versions.
- `runtime_identity` records the current and approved alternate Python base
  runtimes within the collector's established limits.
- Gate command, full ambient environment digest, platform and Git tracked/source
  discovery observations are recorded. Environment values are not printed or
  stored in plaintext.
- Tracked public artifact bytes are observed, including policy/review manifests
  and saved generated outputs. Private `.build` plans, books, installation and
  journey receipts are never inventoried.

The collector runs before and after the freshly observed gate process. Missing
inputs or collector failures produce an unavailable observation; changed inputs
invalidate historical equality. These observations are **not a complete key**.
Shadow stability is descriptive and does not introduce a new acceptance policy.
Gate success/failure still comes from the fresh canonical process.

## Why reuse stays off

The complete gate is a tree of separately authored test programs. Existing
collectors do not close these execution inputs:

| Gap | Actual insertion point |
|---|---|
| Fresh compiler environment and system dependencies | `host_common.py` fresh compilation inherits ambient SDK/include/default configuration; only `HostObjects` has explicit preprocessing closure |
| Nested executable selection | `home_refresh_regional_test.py` launches PATH-selected `python3`; upstream test runners select `CXX` or PATH-selected `c++` |
| Python startup imports | Most subprocesses use `-B`, which preserves startup imports; `runtime_identity` deliberately excludes site-packages |
| Linking and native runtime | Fresh host linking has no complete linker/SDK/sanitizer/loaded-library dependency manifest |
| Swift/macOS framework execution | `library_cover_prepare_test.py` and `prepare-library-covers.py` use Swift and ImageIO/CoreGraphics beyond the existing collectors |
| Git policy/discovery | `code_health.discover` depends on index/ignore visibility and external configuration |
| Complete outputs | Tracked artifact hashes do not cover every generated executable or temporary output |
| Prior success provenance | Editable JSON/logs can be rewritten together; matching hashes do not establish a trusted prior execution |

Closing this would require a broader execution manifest and provenance design.
It cannot be achieved by combining a source hash, compiler version and editable
`passed` field. No cryptographic attestation claim or second build framework is
introduced. The conservative rollout stop condition is reached: **always rerun**.
CI integration must retain this rule unless a separately reviewed complete
closure and provenance design exists.

## Diagnostic record and controls

`.build/software-gate-shadow/observation.json` contains before/after observations,
the fresh process exit/duration and diagnostic log hash. Every record explicitly
sets `complete_input_closure`, `reuse_enabled` and `reuse_eligible` to false, and
records zero avoided invocations/seconds. Historical equality describes observed
bytes only, including when the prior process failed. The report is invalidated
before collection/execution; missing, malformed, duplicate-key, oversized or
forged historical data never permits skipping. Output aliases are rejected.

The host gate exercises source/fixture/asset/generator/vendor/policy mutations,
added/deleted inputs and executable caches, compiler/runtime/environment/command
changes, missing collectors, changes during a gate and forged success. Cold and
warm expected failures execute again and return the same failure/output.
Record the measured shadow-history cold/warm full invocations separately from
object-cache compilation warmth. Eliminated software-gate cost is **zero** in
this phase; any future savings are a proposal, not a measured result.
