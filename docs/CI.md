# Fresh software checks, separate physical acceptance

## Tiers

The gate has three tiers, each with a whole-run wall-time budget
(`tools/gate-time-budgets.json`). A run over its budget fails and names its
costliest phases. Cut that work or move it to the full tier; do not shard it
to fit. If the host was busy (5-minute load above its thread count, as when
two gates share it), the overrun is printed the same way but does not fail:
that time measures the host, not the gate. Time it again on an idle host.
Each run records its phases' wall seconds in `.build/gate-phase-seconds.json`
(CI keeps them in its store), and the next run starts the ready phase with the
longest remaining chain first, so no long phase starts last and runs alone.
The file only orders starts; a missing or malformed one means plan order.

| Tier | What runs | When | Budget |
|---|---|---|---|
| `local` | `push` plus changed-owner full-only Rust positives and the negative controls of changed tests | `python3 tools/check.py` (the default), once per candidate; public CI on a pull request | 12 min on an idle 16-thread host; about 10 min measured (target 8 min) |
| `push` | every positive test except the full-only Rust suites below, one clean firmware build with its resource gate, the release gate and the code-health ratchet | CI on every main push that is not a release | 24 min for the whole CI job on the runner's 8 CPUs; about 19-20 min measured (target 10 min: needs a 16-CPU cap or more cuts) |
| `full` | everything: every negative control and exhaustive matrix, fresh objects and freshly built toolchains | CI on a release merge, nightly and manual runs; `check.py --tier full` by hand | 80 min for `check.py`, 110 min for the CI job |
| Full-only Rust positives | `ft_font` is unreachable in the product; `inflate`, `epub_admission`, `reader_atlas` keep CI within budget. All four also run locally when their owned source or tests change. | `full`; `local` when their sources or tests change | Existing budgets unchanged |

There is one firmware, the Rust image, so every tier tests only it: the Rust
checks, the previews and reviewed renderer frames (the real firmware compiled
for the host over the C fakes in `rust/host/preview`), the native shims the
image links (`rust/**/native`), the FreeInk patches, the release, bench and CI
tools and every resource check. The C++ Newt (alpha.16 and earlier) was
deleted in NEWT-90; it lives only in git history. The
focused `--suite ui` runs the preview tests on that firmware host build.

CI picks the tier itself (`tools/gate_tier.py`): a commit whose `.cz.toml`
version differs from its first parent's (a release merge), a scheduled or a
manual run takes `full`; a pull request takes `local`, a candidate like any
other; any other push takes `push`. Nothing lowers it. Public CI checks a
pull request that changes only docs in the [docs lane](#docs-lane) instead. The
release-tag job tags only the release merge its own run checked, after that
run's gate job passed, so a release is never tagged without a full-tier pass.

A negative control breaks the code on purpose and expects its test to fail. It
is evidence about the test, so it runs when the test changes (`local`) and at
least once per release (`full`). A changed `tests/` file selects the controls
of each test whose sources name it (an include, a path or a module, directly or
through other `tests/` files) or whose name it starts with; a changed file no
test names selects every control. A test that mixes positive checks and
controls asks `gate_tier.negative_controls(__file__)`; run by hand, outside the
gate, it runs both.

The same selection includes Rust test packets and top-level validation/config
inputs under `rust/`, plus `rust-health/`. A family owns its adapter and packet;
named shared inputs select every family that uses them. Unowned inputs select
all controls. Rust product source changes alone do not select mutation controls.
Family owner keys retain their scope (`rust:xml_input`, for example), so repeated
packet filenames such as `controls.py` cannot replace another family's identity.

Every tier runs the bounded Rust landing phase and its contract and
source-privacy test (`rust/tests/landing_test.py`, a separate phase to fit the
CPU budget), the 15 other push-family
positive suites, one synthetic RV32 target-probe build and its verifiers, and
the read-only Rust health ratchet and its regression tests. Landing keeps the
product `newt-app-image` RV32 archive and foundation positive corpus. Selected
foundation mutations run as whole-module records, clock/frame, statistics and
persistence campaigns, without repeating that
corpus; each other
family runs its selected mutations alongside its own positives. The four full-only
positive suites remain in the full plan with every control. Their product-source
owners select positives locally without selecting mutations; changed test owners
select both. The existing
Rust snapshot seals and toolchain checks apply to every Rust phase. Rust phases
run in parallel, one `-j` job each (each averages about one CPU, two cargo
jobs at most). Each owns its receipt and a temporary folder in `.build/rust`,
removed when it ends, pass or fail (#345). Only landing and the target probes,
which share one Cargo target, take turns.
Scoped receipts never claim a complete standalone Rust gate; the current
ordinary gate run requires every planned phase to pass and does not assemble
old receipts. These phase boundaries have no new CPU or wall-time allowances;
their measured compliance remains subject to the existing budgets.

Per-push CI reuses two things from earlier runs, kept beside the workspace by
`tools/ci_persist.py`: the toolchains the last full run built from the same
locks (each copied file must match its recorded size, mode and SHA-256), and
host objects, whose keys bind the exact compiler, flags, environment and every
preprocessed input. The full tier reuses neither. Public CI keeps the same two
in GitHub's cache ([Public CI](#public-ci)).

Rust provisioning requires the repository-level GitHub Actions variable
`NEWT_RUST_ACQUISITION_APPROVED`. It records the owner's scoped approval to
download the official, checksum-verified Rust host toolchain and target archives
pinned in `rust/toolchain.json`; source changes alone grant no acquisition.
Before the first CI run, the owner sets its value to `sha256:` followed by the
SHA-256 of that file. Compute it from the candidate checkout:

```sh
python3 -c "import hashlib, pathlib; print('sha256:' + hashlib.sha256(pathlib.Path('rust/toolchain.json').read_bytes()).hexdigest())"
```

Both CI workflows pass this repository variable to their provisioning tools.
A missing or stale value fails before download or installation. Any change to
`rust/toolchain.json` needs the owner's renewed approval and matching variable
update. Toolchain caches remain bound to the pins and verifier, and every
restored Rust installation is verified before use.

`fresh-gates.yml` uses the owner’s self-hosted Ubuntu x86_64 runner, with
labels `[self-hosted, linux, x64, newt]`. Runner provisioning and
registration remain separate infrastructure work. Mac checks use approved local
tools; there is no paid GitHub-hosted Mac fallback. Local tests do not establish
Linux bootstrap, runner availability, Linux golden parity, extraction footprint
or CI duration.

## Public CI

`.github/workflows/public-ci.yml` is the public repository's CI. It runs the
same gate on GitHub-hosted runners (4 vCPU, 16 GB RAM, about 14 GB free disk),
split into parallel jobs, so a pull request's checks finish in about 10
minutes, or about 2 in the [docs lane](#docs-lane). It runs on every pull request into `main` (forks included; the gate
compares a candidate with `origin/main`), every push to `main` and nightly,
and by hand (`workflow_dispatch`, the full tier); there is no
`pull_request_target`.

**Which repository runs what.** Both repositories are named
`VargasDevelopment/newt`, so the guard is visibility, never the name. Each
public CI job runs only when `toJSON(github.event.repository.private)` is
`'false'`; each `fresh-gates.yml` job only when it is `'true'`. The values are
compared as JSON text because GitHub's `==` treats a missing value as equal to
`false`. A scheduled run's event carries no repository, so there the private
repository's ID (`1400798123`) decides. So GitHub-hosted minutes are never
billed to the private repository, and the self-hosted runner never runs a
public job. Never register the self-hosted runner on the public repository:
public CI runs fork code, and nothing there should reach that host. Once the
private repository is renamed `newt-archive`, none of its workflows runs (each
job requires the name `VargasDevelopment/newt`); that is intended, because it
becomes read-only history.

**Permissions.** The token is read-only (`contents: read`) for every job but
two, which run only on `main` for the owner after the `gate` job passed. On a
push, `release-tag` gets `contents: write` to push one release tag
(`tools/release_tag.py`, which accepts either workflow's gate). On a push or a
manual run, `nightly` gets `contents: write` to push one nightly tag and
publish its pre-release
([RELEASING.md](RELEASING.md#nightly-test-builds)). Actions are
first-party `actions/*` only, pinned to full SHAs in
`.github/ci-tool-linux-lock.json`. uv comes from the wheel
`docs/toolchain-linux-lock.json` pins (checked against its SHA-256), not from
a third-party action; pnpm comes from corepack.

**The install page.** `.github/workflows/install-page.yml` builds the install
page from each published release and deploys it to the public repository's
GitHub Pages ([RELEASING.md](RELEASING.md#install-page)). Its jobs run only in
the public repository, by visibility and name. The build job reads
(`contents: read`); only the deploy job writes, with `pages: write` and
`id-token: write`. Its two Pages actions, `actions/upload-pages-artifact` and
`actions/deploy-pages`, are pinned to full SHAs in the workflow itself rather
than in the tool lock, whose hash keys the toolchain caches;
`tests/public_ci_test.py` checks every pin there.

**Parts.** The `check` job is a matrix of eight parts: seven shards of the host
gate (`tools/ci_gate.py --part I/7`, which runs `tools/check.py --shard I/7`)
and `native`, the plain clean compile and the one clean
`firmware-build.py --clean --health-report` build. The `gate` job always runs
where public CI runs and passes only when its lane's jobs succeeded
([Docs lane](#docs-lane)), so a failed, cancelled or skipped part fails it
rather than skipping it; it is the one check to require and the one the
release-tag job reads. Each part has the tier's whole CI job budget
(`tools/gate-time-budgets.json`); no budget is split or edited. A pull
request that changes a test also runs that test's negative controls (the
`local` tier) in the part that holds them, so it can take longer.

**Docs lane.** A pull request that changes only docs skips the toolchain
and the gate's parts. The first job, `classify` (`tools/docs_lane.py`,
standard library only), lists every path the pull request changes since its
merge base with `main`, a rename as both its old and its new path. The lane is
`docs` only when every path is `README.md`, `AGENTS.md`, `CONTRIBUTING.md`,
`.github/pull_request_template.md` or a `.md`, `.png` or `.jpg` file directly
in `docs/`, as a regular file or deleted. This is an allowlist:
`CHANGELOG.md` (release-managed), `docs/toolchain-*` (they choose the
toolchain) and every other path take `full`, and so does a change set it
cannot list. A pull request runs its base's copy of the classifier, never its
own, so a change cannot choose its lane; a base without one takes `full`.
Pushes, nightly and manual runs always take `full`, so `release-tag` and
`nightly` never follow the docs lane.

In the docs lane the `check` job is skipped and the `docs` job runs instead:

- the release gate as CI's `check.py --release-tags` runs it (Conventional
  Commits since the release tag and the release-tag guard), with the pinned
  Commitizen alone
  (`ci_environment.py --prepare-release`);
- a links check: each relative link or image in a changed Markdown file, and
  each link from any tracked Markdown file to a changed path, must name a
  tracked file or folder (no anchors, no network);
- every gate check that reads docs text (`DOCS_CHECKS`).
  `tests/docs_lane_test.py` guards that list: a Python source that names a
  lane path or the `docs` folder in a string literal must be in it, or listed
  with the reason it is not a docs check. A path built at run time, or a
  reader that is not Python, escapes that scan.

`gate` passes only when `classify` chose a lane and that lane's job succeeded
while the other was skipped: `check` for `full`, `docs` for `docs`. Branch
protection still requires only `gate`. One lane's job is always skipped,
which makes `success()` false for every later job, so `release-tag` and
`nightly` read `needs.gate.result` instead.

**Pull request title.** GitHub's merge commit takes a pull request's title as
its subject, so each lane's job checks the title the event carries with the
pinned Commitizen (`tools/release_tools.py --pr-title`), and `gate` fails on a
title that is not a Conventional Commit. An edit of the pull request (title,
body or base) starts a new run, which cancels the older one, so the whole gate
runs again: about 5 minutes and 40 hosted runner-minutes in the full lane, one
job in the docs lane. A run that checked only the title would have to post
`gate` from an earlier run's result for the same head, and a `gate` it skipped
would count as passing.

**Shards come from the gate's own plan.** `tools/gate_tier.py` `shard()` takes
the tier's plan, keeps every phase with the phases it runs `after` in one
group, and deals the groups, heaviest first, to the lightest shard, weighted by
the CPU seconds in `tools/gate-shard-seconds.json` (a phase not listed weighs
the median). Phases that share a `uses` path wait for each other on one
machine, so a group goes to a shard that holds none of its paths when there is
one, and separate jobs run them in parallel. The result is deterministic, so every job computes the same
split, and `tests/public_ci_test.py` proves that the shards' union is the
tier's plan exactly, each phase once. Stale weights only unbalance the shards;
they never drop a phase. To refresh them, take the `CPU <phase>: <s>s / ...`
lines of a push-tier run's log into the same JSON shape.

**Toolchains and caches.** `tools/ci_hosted.py` makes the toolchains from the
pinned, verified downloads, as the self-hosted job does (`ci_toolchain.py`,
`ci_environment.py`). A cache never stands in for a check:

| Cache | Key | Reused when |
|---|---|---|
| toolchain trees | hash of the toolchain locks and scripts and the release venv's plugin (`tools/cz_newt`) | not the full tier, and every entry matches the manifest recorded when a fresh run made them (else set aside and made fresh) |
| raw archives | hash of the toolchain locks | a fresh materialization rechecks each against its pinned size and SHA-256 |
| host objects, per part | `host-objects-<part>-<sha>`, restoring main's latest | not the full tier; each object's key binds its exact inputs (`host_objects.py`) |

Only main pushes and nightly runs save caches; a pull request only restores
them, so it cannot write what a later trusted run reads. GitHub keeps 10 GB of
cache per repository and evicts the least recently used. Measured as
`tar | zstd`: the toolchain trees take 1.2 GB and the raw archives 2.6 GB;
each main push adds its parts' host objects (about 1.2 GB before compression
for all seven), so older pushes' objects are evicted first.

## Trusted source and execution

This section is about `fresh-gates.yml`, the private repository's
self-hosted workflow. Only owner-authored pushes to `main` and owner manual
dispatches from `main` can start the gate job; only such pushes start the
release-tag job after it. The workflow has no PR, fork or `pull_request_target`
trigger. A manual
`approved_commit` must be a full lowercase SHA already in remote `main` history;
empty selects the workflow commit. The shell ancestry guard runs before any
project script. The persistent runner must not execute unmerged candidate code
without a separately reviewed authorization and validation procedure. These
checks constrain events; they do not sandbox trusted code or isolate the host.

Checkout uses exact source, complete history, the release tags and no persisted
credentials. Full history and the tags let the release gate read the commits
since the release tag, as Commitizen does, and refuse work built on an untagged
release
([RELEASING.md](RELEASING.md#tag-the-release)). Source bytes and modes
are checked before acquisition and around gates. Generated receipts cannot
grant admission. The tracked `artifacts/library-ui-review.json` is a separate
committed acceptance authority and must remain exact throughout the gates.

CI uses all of its available CPU quota for local host-gate jobs.

The sequence remains mandatory:

1. `tools/check.py --tier <tier> --no-time-budget --profile linux` (`--fresh-objects` in the full
   tier): host tests, release checks,
   ASan/UBSan, real SHA vectors and the explicitly limited portable cover tests
   described below. After a failure the gate stops starting phases and only
   finishes those already running.
2. One plain clean native compile makes the platform's first-use package
   edits. This emits no acceptance receipt.
3. One clean `tools/firmware-build.py --clean --health-report` build of the one
   firmware build (regional DEV, `regional-dev-usb`). It validates existing
   RAM/flash/selected stack, NVS linkage and exact release identity.
4. Revalidate its output and committed source after the sequence.

Before any PlatformIO invocation, the firmware build uses `firmware-build.py`'s
native-lane preparation: approved configuration and output paths, the build's
composed SDK copy, and generated app/backdrop source checks.

Upstream Arduino first-use YAML rewriting makes the plain build necessary before
the health build. There is no retry, edited upstream recipe or excluded
identity input. All four acceptance builds subsequently capture and recheck
inputs. Failure or receipt drift stops the sequence; ceilings are unchanged.

## Explicit cover coverage boundary

**Linux.** The firmware uses its own baseline JPEG, progressive JPEG and PNG
decoders. The `jpeg_decoder` and `png_decoder` families, the core
`cover_tolerance` tests and the preview's cover flows
(`tests/preview_cover_test.py`) exercise those paths. The Linux gate also
verifies all frames in `artifacts/library-ui-review.json` using the firmware
host build. The prepared-cover cases use a test helper that accepts only four committed,
SHA-locked authored PNGs and returns their matching packed ImageIO output.
Unknown fixture bytes fail closed.

**Mac.** Run the host-only Swift ImageIO cover gate on the exact commit for
release acceptance and for any change to the ImageIO path. That path includes:

- `tools/prepare-library-covers.py`
- `tools/library-cover-image.swift`
- `tools/library-cover-fixture.py`
- `tests/fixtures/library-cover-images/`
- `cover_art` in `tools/library-redesign-preview.py`
- `tests/library_cover_prepare_test.py`
- `tools/check-mac-covers.py`

The gate runs the cover preparation suite with authored PNG and JPEG inputs
and compares the four locked fixtures against actual ImageIO output.
The Mac gate does not build the firmware or render frames; the firmware host
build is Linux x86_64 only.

```sh
python3 -B tools/check-mac-covers.py --expected-commit FULL_SHA
```

The Mac gate checks committed source before and after the suite and writes
`.build/mac-covers/report.json`. The Linux report marks
`complete_software_coverage: false` because it does not include ImageIO evidence.

## Locked inputs and fresh environments

`.github/ci-tool-linux-lock.json` pins official Actions by full commit, CPython
3.12.6, LLVM 22.1.8 x86_64 and OpenSSL 3.5.9 source by URL, size and SHA-256.
The Python bootstrap uses pinned official `astral-sh/setup-uv` and checksummed
uv 0.12.21 to avoid the Ubuntu 26 `setup-python` lookup gap for 3.12.6. Pinned
uv verifies its embedded CPython 3.12.6 baseline build 20240909 archive hash;
the lock records that archive's SHA-256 and size. It installs from the official
Astral GitHub release mirror with `--no-bin`, then selects only managed Python
with automatic downloads disabled. Tool, cache, Python and executable paths
stay under workspace `.build/python-bootstrap`; the setup action's caches are
disabled. Before adding Python to subsequent steps' PATH, the actual isolated
interpreter must import the standard libraries, report exactly 3.12.6, resolve
inside the managed directory and create and execute a real symlink venv. There
is no unmanaged interpreter fallback.

After bootstrap, preflight requires Ubuntu, glibc >=2.35, exact Python,
`/usr/bin/make` and `/usr/bin/perl` before native, release or host-toolchain
acquisition. Host C/C++ uses project-local LLVM, including
its full resource headers and compiler-rt sanitizer runtime. No system compiler
or sanitizer fallback supplies Linux host tests.

`docs/toolchain-linux-lock.json` locks 63 native inputs (640,088,158 bytes),
including 51 Linux Python wheels, official C3 GCC, Arduino and pioarduino inputs.
The offline Python installation performs no dependency resolution. Existing
native package/build/resource validators remain authoritative. Release tools
use 16 locked wheels (3,036,938 bytes). LLVM’s archive is 1,938,859,476 bytes;
OpenSSL’s is 53,279,637 bytes. Raw inputs total 2,635,264,209 bytes before
Python/Action setup.

The SHA adapter uses CommonCrypto on Mac and the freshly built static OpenSSL
provider on Linux. OpenSSL is built with pinned LLVM and ASan/UBSan, without
assembly, shared modules, DSO, engine or legacy providers. Ambient OpenSSL
configuration is disabled. Real vector, streaming, reset, failure and diagnostic
fingerprint tests must pass; a lock/version string supplies no runtime proof.
The pinned upstream C provider deliberately casts typed callbacks through generic
object-pointer signatures. Those calls are language-level function-type UB,
although the pointer ABI matches on the locked x86_64 target. Its C compilation
therefore excludes only UBSan's `function` check; ASan and the remaining UBSan
checks stay enabled and fatal. This is an explicit third-party compatibility
exception, not a claim that those upstream calls are type-correct. Newt and SDK
host code retain the full `address,undefined` profile, including function checks.
CI phases and the stripped host environment force `halt_on_error=1`, so a UBSan
diagnostic cannot silently recover into a passing receipt.
CI also forces ASan to abort on errors rather than accepting ambient exit-code
overrides. The stripped host environment never imports ambient sanitizer options.
ESP32’s existing SHA branch remains unchanged. The adapter’s two external
includes are narrowly admitted in code-health policy; no resource ceiling or
complexity exemption is added.

Raw `.toolchain/downloads` and `.build/ci-downloads` persist in the runner's
store ([Tiers](#tiers)). Every file must belong to its selected lock and match
size/SHA; unknown, nested, alias or corrupt entries fail. In the full tier,
installed environments, source materializations, objects, executables, native
outputs and reports are fresh; the per-push tier reuses the checked toolchain
snapshot and host objects described above. No saved-proof check exists. Mac locks remain for local compatibility. Read-only
plans acquire nothing:

```sh
python3 -B tools/ci_toolchain.py --plan --target linux-x86_64
python3 -B tools/ci_environment.py --plan
```

The environment plan selects the current OS; Linux selection can be inspected
through `settings(root, 'linux-x86_64')` without acquisition. Materialization is
restricted to GitHub Actions and the selected runtime. Existing execution and
provenance gaps in [software-proof-shadow.md](software-proof-shadow.md) still
prevent exact software-proof reuse; this is not a hermetic attestation system.

## Rollout and acceptance

The workflow default and the gate job have `contents: read`. Public CI has two
write permissions, each a job's own `contents: write`. The small `release-tag`
job (with `actions: read`) tags each release, so nobody has to remember
([RELEASING.md](RELEASING.md#tag-the-release)). The `nightly` job pushes one
nightly tag and publishes its pre-release; its token reaches only the step
that stamps (to read releases) and, after the build, the step that publishes,
which gets it the same way as below
([RELEASING.md](RELEASING.md#nightly-test-builds)). The `release-tag` job runs
only in a push run, after its gate job passed, on the same runner with the
same guards. It
runs no gate or build: only the standard-library `tools/release_tag.py`, with
the runner's `python3`, `git` and `gh`. Its token lasts only while the job
runs. Apart from the pinned checkout action's fetch, only the tag step gets it,
in its environment: no checkout keeps credentials, and git first drops every
helper the runner's own config may hold, then gets the token from
`gh auth git-credential`, so no stored helper can save it or answer instead. With that token, code in the step could push any
branch or tag here; the tool pushes exactly one new tag and never moves, forces
or deletes one. There is no other new secret, artifact
upload, release, deployment or setting change; the nightly pre-release is the
one release CI publishes. The public pinned SDK requires
no extra token. A newer push or nightly run cancels an older push or nightly
run's gate job. It never cancels a release merge's push run: a first small job
(`tools/gate_tier.py --release-push`) puts that run's gate job in a group of its
own, so CI always finishes and tags the release. That job runs on a
GitHub-hosted `ubuntu-latest` runner with stock `python3` and git, no toolchain
and only the read-only default token, so a push can cancel a nightly run at once
instead of waiting behind it for the one self-hosted runner. GitHub's job limit
is 120 minutes; the tier budgets
([Tiers](#tiers)) stop a slow run first and name its costliest phases.
Dedicated runner UID, official runner verification, systemd CPU/memory limits
and registration are an independently reviewed infrastructure task, not effects
of this repository workflow. Do not run PR code on the persistent host.

Complete acceptance of this canonical workflow must verify bootstrap,
the golden hashes its tiers run ([Tiers](#tiers)), the native identity/resources, disk/memory use and
measured duration.
Keep the candidate unaccepted on failure; fix its concrete cause without wider
permissions, exclusions, provider fallback or raised ceilings. Since this
workflow admits only main-history commits, premerge Linux verification requires
a separately authorized trusted procedure. Publication of these branches also
requires review of exact heads, privacy deltas and runner readiness.

CI does not flash, read hardware/SD, approve UI or establish battery, optical,
button, settings or physical reading behavior. A new Git build identity needs
fresh exact-image review. Historical device evidence remains tied to its
original candidate and cannot be transferred by source similarity or CI PASS.

## Gate scratch ownership

`check.py` establishes scratch before every prerequisite and planned phase.
An existing caller `TMPDIR` is resolved and checked for actual write access;
it is preserved on exit. With `TMPDIR` unset, the entry point creates one
private mode-0700 directory under the system's `/var/tmp`, exports its absolute
path to every child, and removes it on normal/failure exit. The runner's bounded
signal-stop cleanup includes this directory too. Tests use Python's default
temporary-directory APIs; native fixtures use the same inherited environment.
Compiler/native subprocess sanitization preserves this directory and the
private native mount collection. Bounded native storage fixtures keep their
physical data in default temporary directories, with short relative mount
names in a per-run private mount collection under `.build`. Each mount ends
in a real private directory, preserving the ports' `O_NOFOLLOW` checks. This models the device's
short mounted root independently of host TMPDIR length; the production
256-byte path limit and the native overflow oracle stay unchanged. The gate
removes both the physical scratch and its mount collection on exit/stop;
standalone fixture contexts remove their own mount collection. Existing
ROOT-working-directory and private-child-umask checks stay in force.
Producer probes use short canonical physical roots instead, because their
production `lstat` checks require every ancestor to be a real directory.
The per-run system-temporary parent uses only a random name; no ownership or
no-follow checks are relaxed to fit paths. An invalid
caller path fails before any prerequisite instead of silently falling back.

The cheap `tools/check_scratch.py` source guard runs before prerequisites and
rejects new fixed scratch paths or required `os.environ['TMPDIR']` lookups in
`tests/` and `tools/`. Its reviewed line exceptions cover the single allocator,
the persistent per-user flock pool, and inert rejection fixtures/comments.
It grants no whole-file exclusions.
Unused source-line exceptions fail the guard. An empty `TMPDIR` has the same
private per-run default as an unset value.

Compiler children retain `TMPDIR` and `NEWT_GATE_NATIVE_ROOTS`, but these two
scratch-placement variables are excluded from host-object content keys in one
place. Compiler, inputs, flags and the remaining compiler environment still
bind every object. The cheap `host_object_reuse_test.py` gate phase runs one
tiny real unit in two fresh gate processes with different scratch and native
roots, requiring exactly one compile and one cache hit. This catches future
per-run key leaks without listing their variable names in the regression.

CI's host command is `env -u TMPDIR python -B tools/check.py --tier <tier>
--no-time-budget --profile linux`, with `--fresh-objects` in the full tier
([Tiers](#tiers)); the job's own budget covers it.
CPU and wall-time evidence remain separate. Running this shape as the developer proves the
unset-environment path; it does not prove execution under the distinct CI uid.

## Host gate time budget

`tools/gate-cpu-budgets.json` is a hand-edited policy. Every planned phase,
including compilation, and each prerequisite checked by `check.py` has a
360-second CPU default; newly added phases inherit it. The independent
360-second ceiling bounds the longest single phase. An optional `phases` entry
has `seconds` and `reason`; the smaller of its budget and the ceiling applies.
Phase overrides must name an actual complete-plan or prerequisite/focused phase;
typos fail before any budget is applied. The 360-second policy includes at least
1.5x margin above the worst observed cold/loaded unchanged phase (227 CPU seconds).
Every policy edit needs a concrete reason in the file and normal code review.
The tool never updates budgets. The explicit cache-disabled `--fresh-objects`
witness (also `NEWT_HOST_OBJECT_CACHE=0`) uses a committed multiplier of 10/3:
1200 CPU seconds per phase/ceiling, since every mutation recompiles shared units.
An ordinary gate starting with an empty cache retains the standard limits.
Investigate regressions and split independent
cases before proposing more time. Missing or malformed policy fails the gate.

The runner measures each phase inside a separate wrapper process, summing its
own and its waited children's user+system CPU (including their waited
descendants). It never subtracts process-wide child usage in scheduler threads,
which would mix concurrent phases. A budget overrun fails that phase, blocks
its dependents, and follows the usual `--keep-going` policy. The existing
process-group stop protocol also covers the wrapper and its descendants.

`CPU` lines give each measurement and limit. `GATE CPU total` sums the scheduled
plan and names the longest single phase; prerequisite CPU appears separately.
These are CPU seconds, not elapsed seconds or the dependency graph's total
weighted path. The gate's existing elapsed timings remain informational: host
contention, slot waits, sleep and I/O are not charged as CPU. Report load,
object-cache warmth and wall time alongside CPU results when comparing runs;
a passing CPU budget cannot guarantee a six-minute wall time on a loaded host.
`--suite quick`, focused suites and `-j 1` retain their development semantics.

All participating checkouts for one user share
`/var/tmp/newt-host-job-slots-v2-<uid>/slot-N` via `flock`. Each pool must be a real,
non-symlink directory owned by the current uid with no group/other write access;
new pools are created with mode 0700. CI runs as a different user and has its own
pool: this caps one user's gates, not the combined interactive and CI load.
The capacity defaults to the smaller of the process's CPU affinity and its
cgroup v2 CPU quota, rounded up to at least one CPU. The gate reads its own
`/proc/self/cgroup` membership and walks `cpu.max` through every visible ancestor;
`max` leaves that level unlimited. Without readable cgroup limits it uses
affinity alone; without affinity support it falls back to `os.cpu_count()`.
Default local jobs are that usable CPU count, and standalone
host compiler helpers use the same CPU calculation. The `HOST slots capacity`
line reports the sizing source. Set
`NEWT_HOST_GLOBAL_JOBS=N` or `check.py --host-jobs N` to override it; use the same
N for concurrent gates (a smaller setting cannot constrain another gate using
a larger pool; gates in differently limited cgroups get different defaults, so
set N explicitly there too). Local `-j` still bounds each gate separately.
Existing gates on older code do not participate until updated/restarted.

A phase reserves one host slot. One nested compiler can borrow that reservation;
additional preprocessing/compile/link jobs acquire other host slots. Waiters
retry both credit and pool slots, so even N=1 makes progress without nested-token
deadlock. No pool file is unlinked or truncated: inode identity stays shared,
and a process crash releases its locks automatically. Each successful acquisition
refreshes the slot inode's mtime so normal age-based `/var/tmp` cleanup does not
remove actively reused files. Deliberately removing a pool or its files while
in use breaks the shared inode guarantee; never do that. The old v1 pool is
left untouched for gates still using it. `HOST slots waited …`
reports phase admission wait and aggregate nested-job wait on one line when
nonzero. Concurrent worker waits are summed, not presented as elapsed wall time.
`NEWT_HOST_GLOBAL_SLOT_DIR` isolates the fake-gate regression pools; normal gates
should retain the shared default. These are job limits, not OS thread limits.
