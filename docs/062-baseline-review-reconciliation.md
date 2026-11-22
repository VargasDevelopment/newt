# Baseline review reconciliation (2026-10-12)

## PR39 threads

A repository-only independent review reconciled every unresolved PR39 thread
against head `441836355a4e8a594b027bf8b64fdc3c4fb6175f` and installed
`ea4570aff5ad09199f8545acb825ca8d5086b84d`: **20 verified fixes, two documented
deferred limitations, no newly identified unresolved blocker**. Outdated UI
state was not treated as proof of a fix. All 20 fixing commits are ancestors of
PR39 itself, so none requires merging layer 40 to repair that layer.

Existing Codex bot responses reported no major issues on the exact current
#37 (`1772e3b`), #38 (`0e3ff5f`) and #39 (`4418363`) heads. The #40 bot response
binds only historical groundwork `f7d64aa`; later firmware changes rely on their
separately recorded independent source/resource/evidence reviews and installed
image evidence, not an approval transferred from that older response. Bot
boilerplate containing `@codex review` is not a human review request.

| Thread | Disposition | Fix or retained evidence |
| --- | --- | --- |
| [1](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236187581) | Fixed | `38d386b`: tests/cover_decode_native_sizes.py calls trusted_host.verify_source_pin before and after evidence generation; later EvidenceReport binding remains present. |
| [2](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236187586) | Fixed | `38d386b`: The native-size probe validates its output hierarchy and files with native_output helpers; df882981 later strengthened this to owned_output_tree before any compiler child. |
| [3](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236209182) | Fixed | `8e59c5c`: Compiler and ABI-size checks are explicit RuntimeError branches, and therefore survive python -O. |
| [4](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236225713) | Fixed | `5a4606d`: The budget probe validates its entire fixed output tree before fixture or compiler writes. |
| [5](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236225717) | Fixed | `5a4606d`: The budget probe verifies the actual pinned SDK checkout before compiling and again before publishing. |
| [6](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236225718) | Fixed | `5a4606d`: Budget evidence predicates use an explicit require helper; native_output also refuses evidence writes from an optimized interpreter. |
| [7](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236255875) | Fixed | `5ae7159`: cover_job_test validates the complete fixed-name output tree and object directory before parallel compilation or report writes. |
| [8](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236255877) | Fixed | `a1c917a`: The verdict-route digest includes include/newt/library_cover.h, explicitly covering the NLC2 format, route constant, dimensions and CRC implementation. |
| [9](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236303598) | **Deferred; not fixed** | `f5cc80a`: Current coverDecode still returns Unsupported/Incomplete/Invalid before CoverCheckedImage.finish can verify the member CRC. docs/059 lines 1670-1683 precisely track successful reads returning wrong bytes, persistence lifetime, and two possible remedies; e99889e corrected overstatements. |
| [10](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236303601) | Fixed | `41483d1`: cover_job_test uses explicit require checks for parsed output, resource bounds and every required PASS marker; optimized evidence writes are also refused centrally. |
| [11](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236303606) | Fixed | `18c6e9e`: The patch sanitizer runs with a fixed environment and fail-fast sanitizer options. |
| [12](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236303612) | **Deferred; not fixed** | `f5cc80a`: Current CoverIdleTargets/CoverIdleState retain only a 32-bit FNV view and compare only that value before reusing settled bits. docs/059 lines 1684-1690 track chosen-name collisions, their consequence, and the RAM/catalog-version tradeoff; e99889e corrected the probability claim. |
| [13](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236378238) | Fixed | `c7ddd0b`: cover_job_test verifies the SDK byte/origin pin before work and again before its EvidenceReport write. |
| [14](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236440360) | Fixed | `df88298`: The patched-decoder native probe validates its whole output tree before compiling; EvidenceReport records only compiler-produced .su files. |
| [15](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236440362) | Fixed | `df88298`: The native-job probe validates the whole output tree before any compiler; its stack report reads EvidenceReport.produced('.su') rather than a directory glob. |
| [16](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236469640) | Fixed | `12417b3`: All reviewed compilers run through compiler_environment, a positive allowlist that omits DEPENDENCIES_OUTPUT, SUNPRO_DEPENDENCIES and other inherited compiler-output variables. |
| [17](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236469643) | Fixed | `12417b3`: Native-job compiles emit dependency rules and EvidenceReport captures/rechecks the full transitive dependency set; e81e0d3 unified and retained the captured maps. |
| [18](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236469645) | Fixed | `12417b3`: The budget probe compiles under EvidenceReport dependency binding, so include/newt/library_cover.h is captured from the compiler's dependency rule and rechecked before report publication. |
| [19](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236580767) | Fixed | `e81e0d3`: Every parallel EvidenceReport.compile retains its own captured dependency map under a lock; report check compares those same maps before writing. |
| [20](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236580768) | Fixed | `e81e0d3`: EvidenceReport.compile discovers named prebuilt object inputs, requires them to be recorded outputs, and checks their digests before and after the link compile. |
| [21](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236580769) | Fixed | `e81e0d3`: EvidenceReport snapshots imported project Python modules, including cover_job_test.py and cover_job_fixtures.py, rechecks them, and writes procedure_sha256. |
| [22](https://github.com/VargasDevelopment/newt/pull/39#discussion_r4236659853) | Fixed | `4418363`: Objcopy now runs through EvidenceReport.derive, which binds the objcopy executable, the recorded object input and the extracted output through report publication. |

The two deferred limits remain tracked under [#41](https://github.com/VargasDevelopment/newt/issues/41)
and [Decision 059](059-automatic-cover-port.md#risks-and-deferred-work):

- A read can report success while supplying wrong bytes. A negative JPEG or
  metadata verdict may then persist a false no-art result for an otherwise valid
  cover. Independent-read verification or leaving suspect negatives unrecorded
  needs a bounded design; the original finding is not represented as fixed.
- Two different shelves can collide in the 32-bit settled-view memo and postpone
  generation on the second shelf until the view/screen changes. Exact identity
  requires a deliberate bounded representation or catalog-version design.

Neither limitation diagnoses the owner-reported missing covers. The follow-up
must classify actual image support, discovery, generation, cache and I/O behavior
with generic fixtures. The two deferred threads remain unresolved; verified
fixes can be resolved after applicable regressions pass.

## Superseded PR audit

The independent ancestry/source-trailer/requirements audit found no genuinely
lost unique implementation in #18→#37, #19→#38 or #20→#39. Old commit SHAs remain
unique because the implementation was ported to the supported runtime as new
Conventional Commits; source authorship and lineage are retained.

| Old PR | Replacement | Source lineage and retained requirements |
| --- | --- | --- |
| [#18](https://github.com/VargasDevelopment/newt/pull/18) | [#37](https://github.com/VargasDevelopment/newt/pull/37) | Original `e4b27aca` / draft `f84fab0` maps to `736f8f7` and `32ef38a`; matching reading faces, bounded styling, cache identity and failure/oracle coverage are retained and extended. |
| [#19](https://github.com/VargasDevelopment/newt/pull/19) | [#38](https://github.com/VargasDevelopment/newt/pull/38) | Original `f1dcdc2` / draft `53bd0b7` maps to `fbb08ca`; Home cover adapter and lifetime/refresh/read-path requirements are retained. Decision 049 becomes 057. |
| [#20](https://github.com/VargasDevelopment/newt/pull/20) | [#39](https://github.com/VargasDevelopment/newt/pull/39) | Source `fd678dd`, `4f24832`, `c2df314` map to `1071fe1`, `a1faee5`, `ccc3708`; producer, lifecycle, failure coverage and resource guards are retained and extended. Decisions 034/035 become 058/059. |

The snapshot contains no unresolved inline review feedback on the three old PRs.
Their requirements continue in replacement records and the open follow-up issues.
Close only these three PRs after their replacements merge, with replacement links.
Preserve all old and replacement branches; closing a PR is not deleting its work.

## Issue29 is separate

[#29](https://github.com/VargasDevelopment/newt/issues/29) tracks Stage and remains
open. Its contract-only status is historical: Stage is implemented and installed
as a durable experimental opt-in. The owner accepted its Home UI and repeated
unplugged sleep/wake on `ea4570aff5ad`. The original moving halo/white-band concept
was superseded by owner-directed quiet focus with an 8 px top edge and no strip.
Frosted remains the default; no default-switch or complete issue closure follows
from merging the stack. Cold power-on and the explicitly deferred Mac test remain
separate. Preserve the issue’s Books/Apps and other residual product requirements.

## Applicable checks and merge policy

The fresh complete Linux host gate **passed** in 1,480.923 seconds:

```sh
.toolchain/platformio/penv/bin/python -B tools/check.py --profile linux
```

It covers styling/face parity, cover discovery/job/idle/fault routes, Stage
startup/focus, allocation/failure behavior and renderer checks. Log SHA-256:
`3220309af60ac1fe10db6996a85f45e3a2b1c8f8c84be2e8ff93603c9cab57b1`.
After it completed, these checks ran serially and **passed** under the same
approved Python: `tests/native_output_test.py`,
`tests/cover_decode_native_sizes.py`,
`tests/cover_decoder_patch_native_sizes.py` and
`tests/cover_job_native_sizes.py` (the three size probes used the installed
C3 compiler; the job probe also used the approved package directory).
The complete gate includes the evidence-report, sanitizer/destination and
cover-job regressions. These results validate the review fixes; native probes
are component/ABI evidence, not fresh whole-firmware images or device evidence.

All 622 tracked non-documentation/evidence files were compared directly with
the installed revision's Git blobs, including actual filesystem bytes rather
than relying on Git status flags. They match. Independent acceptance review
completed three bounded rounds: six documentation/status findings corrected,
then CLEAN. Protected budgets and formal latency policy are byte-unchanged.

This pass changes documentation/acceptance metadata only. The four frozen clean
resource receipts describe installed source `ea4570aff5ad`, not new builds of
later commits. Firmware and build inputs are unchanged; no resource ceiling,
allocation reserve or fragmentation protection is relaxed. Merge retargeting
must preserve each layer’s remaining diff and the tested implementation tree.
A source/conflict resolution that changes runtime inputs requires new applicable
tests and resource evidence, and does not inherit owner hardware acceptance.

Merge #37→#38→#39→#40 using merge commits after review/check reconciliation.
After each merge, retarget the next existing PR to `main` before merging it,
verify that its remaining diff is exactly its intended layer, and check the
resulting runtime-tree parity. Create no replacement PR and preserve every source
branch. Applicable tests must pass; runtime changes need fresh evidence rather
than inheriting a prior source's result.
Keep #41, #42, #33, #35, #4 and #22 open. See
[Decision 061](061-experimental-baseline-acceptance.md) for acceptance scope,
the scoped heap exception, the focused follow-up plan and the next combined
physical session. No device access or release publication is part of this pass.
