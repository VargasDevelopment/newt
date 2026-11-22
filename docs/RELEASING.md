# Newt firmware versions

The firmware's About screen reads `rust/firmware/native/firmware_version.h`. Official
Python [Commitizen](https://commitizen-tools.github.io/commitizen/) owns that
label and the matching `.cz.toml` version. It uses Commitizen's `semver2` scheme, with `alpha.N`,
`beta.N` and `rc.N` prereleases starting at zero. Pocket Pause's app version
(`apps/pocket-pause.json`), `.newt` format and USB/DIAG schema numbers are separate
contracts; a firmware bump does not migrate them.

The selected current-runtime snapshot
`6911ddb16b50d657dd779036fc3eefd96756be86` had no firmware release tag
or firmware release version: About said “M1 UX candidate”; DEV USB used a Git
revision, optional regional-experiment suffix and dirty suffix. Archived ESP
image metadata's `arduino-lib-builder` version `8cabf2c` identifies the pinned
Arduino provider, not a Newt release. `0.1.0-alpha.0` is therefore an unreleased
initial development label. No old build is renamed, retagged or reflashed.
This integrates the reviewed Commitizen candidate
`b86ddeed54f1038a348bc17e36bdf2d6226f4798` onto that current runtime. Its original
`da723f5be94abfe299e5e15d1e18be311424c60d` adoption boundary is historical
provenance; current-runtime legacy commits through `6911ddb` remain unchanged.
The original tooling boundary was `1e352f752979ec3cff8e28e21f6a0bd6e6716744`.
The additional source-import boundary is
`fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d`, the immutable published #16 draft
checkpoint. It preserves original author/date and subjects from the separate
unpublished runtime branch as an explicit bounded frozen-source import exception; its `Source-Commit` trailers identify those
frozen source handoffs. The source-only import excludes historical device/build
artifacts. This bounded closure does not exempt new implementation commits or
add an allowed prefix. The release gate pins this same boundary;
every later commit, including future merges, must satisfy Conventional Commits.
The integration must retain this ancestry (a normal merge, not a squash that
would discard the pinned boundary). No published commit or remote history is
rewritten. This closure requires independent source review with the integration.

A repository started from a clean export (`python3 -B tools/clean_export.py
--rev REV --out DIR --commit 'NAME <EMAIL>'`) has no such commit. Its single
root commit carries the `Newt-Snapshot: <exported commit> sha256:<tree digest>`
trailer the export prints, exactly once, and the gate recomputes that digest
from the root's own tree. `--commit` makes that root itself after a clean scan:
exactly the exported entries, including tracked files the `.gitignore` matches
and the FreeInk pin, which plain `git add -A` would leave out. Only then is
that root the boundary: every later commit must satisfy Conventional Commits,
and the root builds on no earlier release until CI tags it with its version. A shallow clone, a malformed or repeated trailer, a trailer on
a later commit, or a digest that the root's tree does not reproduce fails closed.

## Project-local setup and CI

Use the project's approved interpreter and Git. The fresh Linux CI path requires
exact Python 3.12.6 and materializes the pinned official release wheels from the
platform-selected lock. Reuse an already approved project-local release venv for
local work. New dependency acquisition still requires scoped authorization;
this document does not grant it. No hooks, shell settings or global tools are installed.

```sh
python3 -m venv .release-venv
PIP_CONFIG_FILE=/dev/null PIP_CACHE_DIR="$PWD/.build/release-pip-cache" \
  .release-venv/bin/python -m pip install --index-url https://pypi.org/simple \
  -r requirements-release.txt
.release-venv/bin/python -I -B -m commitizen commit
python3 -B tools/check.py
```

`tools/check.py` remains the canonical host path. It fails on version
drift, work built on an untagged release ([Tag the release](#tag-the-release)),
a missing/wrong Commitizen install, malformed new commit messages, or
release-tool regression tests. The current reviewed Actions workflow runs fresh
host gate and the one firmware build's native resource gate; Linux coverage also
requires the separate exact-source Mac ImageIO cover check. See [CI.md](CI.md)
for pinned setup, platform coverage and evidence limits. Versioning adds no
credentials or publication job. CI's one write permission is the small job
that tags a release ([Tag the release](#tag-the-release)).
History through the adoption baseline is exempt. Subsequent commits are checked
in full, including merge/fixup subjects; use Conventional Commits or squash/rebase
them before integration. CI must fetch enough history to include the adoption
baseline, and the release tags; a missing baseline or tag fails closed. For an
explicit review range:

```sh
python3 -B tools/release_tools.py --rev-range BASE..HEAD
```

The release-tag guard checks the range's end, so name the planned merge there.
A range that ends in a merge is a planned merge onto `origin/main`: its first
parent must already be on your `origin/main`, so `git fetch origin` first.

Set a merge's subject explicitly, for example
`gh pr merge NUMBER --merge --subject 'fix(scope): describe the change'`.
GitHub's default `Merge pull request ...` envelope fails the same history check,
even if every parent commit passes. Before publishing, construct the planned
two-parent merge locally and check its full adoption range with the pinned gate.
If it changes `code-health-baseline.json`, also run `python3 tools/code_health.py
--variant regional-dev-usb --adopt BASE` on that merge, on its clean build
receipts, with `BASE` the destination branch tip, never an older merge base
(docs/CODE_HEALTH.md, "Memory policy").
Do not repair published history or relax this rule without specific owner approval.

`feat:` adds a feature and bumps minor. `fix:`, `perf:` and `refactor:` bump patch.
`docs:`, `test:`, `chore:`, `build:`, `ci:` and `style:` alone do not request a bump.
Declare a breaking change deliberately with `!` or a `BREAKING CHANGE:` footer
that explains the affected contract and migration. While major is zero, breaking
changes bump minor; a reviewed transition to stable 1.0 must remove
`major_version_zero` using a deliberate breaking-change commit. Inspect the
classified range during release review: labels cannot prove compatibility,
record migration, recoverability or physical acceptance.

## Prepare a reviewed release

`v0.1.0-alpha.0` is the reviewed baseline tag (owner decision 10 in
[082](082-ota-updates.md)). Each owner update bumps to the next alpha. Newt
offers newer versions from the card; reinstalling the same version needs an
extra confirm. Commitizen classifies the history since the last tag, so never
bump from a branch that lacks it.

On a clean release branch from `main`, check messages and version consistency
before generating files:

```sh
python3 -B tools/release_tools.py
.release-venv/bin/python -I -B -m commitizen bump --get-next --prerelease alpha
.release-venv/bin/python -I -B -m commitizen bump \
  --version-files-only --check-consistency --yes --prerelease alpha
git diff -- .cz.toml rust/firmware/native/firmware_version.h CHANGELOG.md
python3 -B tools/check.py --tier full
```

The release merge's CI runs the full tier ([CI.md](CI.md#tiers)), and a
failure there blocks every merge after it, so run the full tier on the release
branch first.

Use `beta` or `rc` for a deliberate phase transition. Omit `--prerelease` only
when intentionally preparing the stable version. Commitizen's default linear
progression retains the core version for patch changes during a prerelease and
increments its counter; a feature/breaking change can advance the core version.
`--version-files-only` updates version/changelog files without creating a commit
or tag. Always review the diff, then make a local Conventional Commit. Do not use
bare `cz bump`: it commits and tags, and its tag would land on the release
branch's commit, not on the reviewed release merge that CI checks. The release
gate rejects such a tag. No command here publishes anything.

The small changelog template omits wall-clock release dates, so the same ordered
history and configuration produce the same text. Each bump adds the entries
since the last tag (Commitizen's incremental changelog); earlier candidate
history is never regenerated into releases, so `.cz.toml` names no start
revision and a full `cz changelog` is never run. Keep generated entries plus
the adoption note in `CHANGELOG.md`.

## Exact identity and acceptance

The About Version label is not an image identifier; About's build line under
it shows the DEV build ID and its commit date
([081](081-about-build-whats-new.md)). DEV USB's exact Git/variant/dirty
build ID stays unchanged. `tools/firmware-build.py --clean --health-report`
also writes `artifacts/firmware-identity-regional-dev-usb.json`: firmware version, full
Git revision, dirty flag, variant, binary SHA-256 and existing source/toolchain
resource identity. Capture occurs before compilation and is rechecked afterwards;
the image hash comes from the verified clean resource report. Receipt generation
does not alter the acceptance report schema. Old Arduino app descriptor metadata
is left alone; use the Newt receipt for release identity.

Before each owner update, update What's new: edit
`rust/device-ui/whats_new_notes.h` with the update's date and its changes in
plain words, newest first, and commit it before the clean build, so the notes
ship with the build they describe. Keep the last few updates; when About
no longer fits them (check the About preview), drop the oldest. The update's
date is the UTC date on the day the release branch is prepared
(`date -u '+%b %-d, %Y'`, for example "Nov 2, 2026"), whatever anyone's local
time zone is, so every release dates the same way.

Compile the one firmware build with its resource gate
(`python3 tools/firmware-build.py --clean --health-report`), preserving the
reviewed RAM, stack and flash policy.
Commit the source first,
then rebuild for a clean release identity; a dirty build is development evidence.
Archive receipts with the exact candidate before transferring it under separate
authorization. A version/changelog or successful compile confers no flash or
publication permission. Preserve the existing [reading acceptance gates](reading-acceptance.md),
mandatory independently validated latency admission with a reviewed proper
baseline, physical/battery/optical/button acceptance, recovery requirements, source/license
review and publication exclusions. Release review must inspect compatibility and
deliberate breaking declarations. Nothing here releases or flashes
automatically; the release tag below is the one thing CI pushes.

## Tag the release

Merge the reviewed release branch like any other change: an explicit
Conventional subject and both parents kept. That release merge is the release.
CI tags it. Once a `fresh-gates.yml` push run's gate job passes, the run's
`release-tag` job runs `python3 -B tools/release_tag.py --ci-run RUN_ID COMMIT`
with this run's id and the commit its gate checked. If that commit is not the
release, it says so and changes nothing. Otherwise it confirms through the API
that this run is a push run on main for exactly that commit and that its gate
job succeeded, then tags as `github-actions[bot]`. A manual dispatch never
tags: it may test a commit other than its own.

The tool finds the release merge: the oldest commit on origin/main's
first-parent history with main's version. It checks that commit's own version
files and refuses unless a `fresh-gates.yml` push run for exactly that commit
has a successful gate job. It then creates the annotated tag `v<version>` (its
message names the version, the source commit, the CI run and that the tag makes
no hardware acceptance claim) and pushes exactly that one tag to origin. It
never moves, deletes or force-pushes a tag and never uses `--tags`. A rerun
after success changes nothing; after an interrupted run it pushes or fetches
the same tag.

The manual command is the fallback when CI's tag job did not finish:

```sh
git fetch origin
python3 -B tools/release_tag.py --dry-run
python3 -B tools/release_tag.py
```

`--dry-run` makes the same checks and prints the plan. A newer push to main
never cancels the release's run: its gate job has a concurrency group of its
own ([CI.md](CI.md)). If its tag job fails after its gate job passed, re-run
its failed jobs in Actions or run the manual command. If the gate job did not
pass, re-run that push run in Actions; CI then tags it. Until the tag exists, the guard below blocks every
merge.

The release gate makes a skipped tag impossible to miss. `tools/check.py`, CI
and the planned-merge check above refuse anything built or merged on top of an
untagged release, and name the missing tag and this command. The release they
check is the version at the newest origin/main commit the work contains through
any parent (each one, if git finds several): the destination tip for a planned
merge, the fork point for a branch, or main itself once a branch merges it in.
A commit already on origin/main is checked at its first parent. That version's
tag must name a commit with that version on origin/main's first-parent
history. A release branch and its release merge pass before their tag exists.
If a release merge's CI fails for a real reason, nothing can merge after it
until that is resolved, which needs the owner's specific approval like any
published mistake.

## Publish an update

An update is two files: `newt.bin`, the image, and `newt.manifest`, a small
text file with its version, 12-hex build ID, size and SHA-256
([083](083-ota-sd-install.md)). The
same two files serve a home-network folder or a GitHub Release
([Publish a GitHub release](#publish-a-github-release)) unchanged.

1. Commit the source, then build clean:
   `python3 tools/firmware-build.py --clean --health-report`.
2. Write the pair into a new folder:
   `python3 -B tools/update_publish.py newt --out .build/update/VERSION`.
   It reads the build's `firmware.bin` and its identity receipt. For a reviewed
   bench candidate, pass `--candidate DIR --manifest-sha256 HASH` instead.
   It refuses a dirty build, an image that differs from its receipt, a version
   that is not SemVer 2.0.0 or is over 23 characters, a damaged or oversized
   image, and a folder that already holds either file.
3. Copy `newt.bin` and `newt.manifest` to `/Newt/Update/` on the card.
4. Send the owner the version and build it printed.

Another firmware needs no manifest: its own `.bin` in `/Newt/Update/` is
offered after Newt checks it (docs/083). Publishing grants no flash, push or
release permission.

## Publish a GitHub release

Updates are GitHub Releases in the repository the release's own firmware
reads: `LATEST_RELEASE_URL` in `rust/core/src/update_online.rs`, read at the
release commit. Until launch that is `VargasDevelopment/newt-releases`, because
this source repository is private; from launch it is the public
`VargasDevelopment/newt` ([Launch](#launch-releases-move-to-the-public-repository)).
A device reads `GET /repos/<repository>/releases/latest`, then that release's
`newt.manifest` and `newt.bin`. `tools/release_publish.py` checks every rule
below and publishes the release there; only the Lead runs it with `--publish`.


### First Rust release (NRUST-86)

This records the alpha.16 to Rust crossover. The C++ Newt was deleted in
NEWT-90; alpha.16's source lives only in git history (tag `cpp-final`).

The shipping image is the full Rust `regional-dev-usb` build: regional display,
DEV USB, Stage, Wi-Fi/update and the embedded sample. The update files retain
their format and names. Publication requires that image's clean identity and
resource receipt; a C++ image or a Rust bench build with omitted features is
not a release candidate.

Before the release merge, use `bench_flash.py` on the bench with its real card:
install exact alpha.16, establish the reading anchor, preferences, statistics,
theme and a Test of the saved network, then install the exact Rust candidate.
Require VALID boot, Home, the expected version/build and preserved durable
values; report cache reuse or rebuild and reading heap/stack. Record the actual
written ranges: USB installation can replace the existing Newt slot and write
a fallback selection, whereas the updater writes the inactive slot. USB
crossover does not prove an HTTPS download or an OTA installation. Separately
check the Rust pair against alpha.16's manifest, version-order, slot-size and
image-header/descriptor/checksum rules, citing alpha.16's source and exact pair.

There is no unpublished Wi-Fi test route. After the Lead merges, CI passes and
tags the source, the Lead publishes the three assets through the reviewed
process below. Before notifying the owner, run both real Wi-Fi transitions on
the bench: exact alpha.16 to the published Rust alpha, then a distinct lower
Rust version to that same published alpha. Repeat the durable-value, boot and
memory checks. Any failure stops the run and is fixed forward; never publish a
return to C++.

Use only `bench_flash.py` and `dev-usb.py`, one lease and at least 30 seconds
between port opens. Test may join the saved network; never Save, Forget, read
its record, or press sleep. Backups stay private. Neither this procedure nor a
passing build authorizes another device or agent publication.

### Rules

1. Publish only a release merge that CI tagged `v<version>` on origin/main,
   built clean from exactly that commit: its identity receipt is not dirty and
   the image's SHA-256 matches the receipt. The tag marks the reviewed release
   that passed its exact-source CI; nothing else has that identity.
2. Publish only the one firmware build, `regional-dev-usb`. It is the build the
   owner runs every day; plain and release variants are never shipped.
3. One GitHub Release per version. Once published, it is never edited or
   deleted; a mistake gets a new version. The tool never deletes a release. Devices and people may already hold
   its files, so its version and SHA-256 must keep meaning those bytes.
4. Names: the tag is `v<version>`, the same name as the source tag; the title
   is `Newt <version>`; the assets are exactly `newt.bin`, `newt.manifest` and
   `NOTICES.txt`, never others. Devices find the update by these names.
5. GitHub's pre-release flag stays off, and each release is marked Latest.
   `/releases/latest` skips pre-releases and drafts, so a flagged release would
   be invisible to devices. The SemVer tag (`alpha.N`) states the maturity.
   The tool refuses `--prerelease` until a reviewed device change adds release
   channels. Publishing goes through a draft, which is never public: the tool
   creates the draft with its three assets, verifies them, then publishes it
   with one request. (`gh release create` without `--draft` does those steps
   itself, and deletes the release when the publish answer is lost, even if
   GitHub published it.)
6. The notes, in plain words: the What's new lines this update adds: those no
   published release notes in the same repository have shown yet, so a first
   release there lists them all, then the build's identity: version, full
   source commit, 12-hex build ID, `newt.bin` size and SHA-256. The CHANGELOG
   stays out: it describes internal work. No links into the source
   repository while it is private (they 404; the tool refuses them until the
   firmware reads that repository), and no talk of the bench, the Lead,
   records or ledgers. Strangers read them.
7. `NOTICES.txt` is `docs/NOTICES.txt` at the release commit, shipped
   unchanged: every third-party component linked into `newt.bin`, with its
   license and notice text. The tool refuses while it contains
   `DRAFT: needs owner approval`, which marks any notice still awaiting the
   owner. Shipping `newt.bin` is shipping those components, and their
   licenses require the notices. For the LGPL-covered Arduino-ESP32 core the
   owner chose (2026-10-30) a written offer, valid at least three years: on
   request, the materials LGPL 2.1 section 6(a) names, so anyone can relink
   `newt.bin` with a changed core. Requests come as issues on the repository
   the firmware reads; no e-mail address. The tool refuses a release whose
   `NOTICES.txt` does not name exactly that repository's issues page
   (`https://github.com/REPOSITORY/issues`), so the offer never points at a
   private one. An offer made once stays valid for its three years, so the
   repository an earlier release named keeps its issues open. Whoever answers
   sends the release's object files, build configuration, linker scripts and
   core source.
8. Nothing about this computer leaves it. The tool scans `newt.bin` and the
   notes for absolute paths under the home folder or the checkout, the user
   name, the host name, `.build/private`, `private-books` and e-mail addresses
   that are not in `NOTICES.txt`. It builds these patterns when it runs, so no
   private string is committed.
9. Before publishing, the Lead installs the exact pair on the bench X3 and sees
   a VALID boot and Home. A public release reaches devices we cannot reach;
   ours goes first.

### Steps

```sh
git fetch origin
git switch --detach vVERSION            # the tag CI made; no local changes
python3 tools/firmware-build.py --clean --health-report
python3 -B tools/update_publish.py newt --out .build/release/bench-VERSION
# The Lead: the bench install check, then its receipt (below).
python3 -B tools/release_publish.py --bench-receipt .build/release/bench-VERSION.json
python3 -B tools/release_publish.py --bench-receipt .build/release/bench-VERSION.json --publish
```

The bench install check: install `newt.bin` and `newt.manifest` from
`.build/release/bench-VERSION` on the bench X3 (from the card, over Wi-Fi
once it can, or with `tools/bench_flash.py`, which writes the exact `newt.bin`;
owner approval 2026-10-31: no tool writes the bench card, and Wi-Fi needs a
published release), let it boot, and see the running slot's state is VALID and
Home is on the screen. Record it as JSON in `.build/release/bench-VERSION.json`
(private; never published):

```json
{"version": "0.1.0-alpha.2", "build": "0123456789ab",
 "newt_bin_sha256": "<as update_publish.py printed>",
 "newt_manifest_sha256": "<as update_publish.py printed>",
 "boot": "VALID", "home": true,
 "checked_at": "2026-11-02T10:15:00Z", "installed_by": "card"}
```

The tool requires the version, build, both SHA-256s, `"boot": "VALID"` and
`"home": true` to match; `checked_at` and `installed_by` (`"card"`, `"wifi"`
or `"bench_flash"`) are the record.

The dry run makes every check and prints the plan, the exact `gh` commands and
the notes; it writes nothing. Read the notes as a stranger would. `--publish`
repeats the checks, then:

1. writes the pair through `update_publish.py` (with its own checks) plus
   `NOTICES.txt` and the notes into a new folder (`.build/release/vVERSION` by
   default);
2. creates a draft with `gh release create ... --draft` and the three files;
3. reads the draft back by its id and checks it holds exactly `newt.bin`,
   `newt.manifest` and `NOTICES.txt`, with the expected sizes and SHA-256s
   (GitHub's digest, or the downloaded bytes);
4. publishes it with one
   `gh api -X PATCH repos/REPOSITORY/releases/ID -F draft=false -f make_latest=true`;
5. reads the release back by id (published means success, even if the PATCH
   answer was lost), then checks `/releases/latest` is this release with no
   pre-release flag and exactly the three assets.

A rerun that finds a draft for the tag resumes it only if its title, notes,
pre-release flag and assets all match what this run would create, because
publishing keeps the draft's own title and notes. It then publishes it without
creating another. Any difference is refused like a partial draft.

### When a step fails

- No tag, or the tag names another commit: `git fetch origin`. If CI has not
  tagged the release yet, see [Tag the release](#tag-the-release). Rebuild
  from the tag itself.
- Dirty build, or the image differs from its receipt: rebuild clean from the
  tag. Never edit a receipt.
- A host detail in `newt.bin` or the notes: do not publish. Fix its source
  (for example a `__FILE__` path, which the build should map with
  `-ffile-prefix-map`) in a new version; a tagged build cannot change.
- `NOTICES.txt` missing or still a draft: resolve it in the source (the owner
  approves license text) and release a new version.
- The bench check fails: do not publish; fix it in a new version. The failed
  version's source tag stays.
- A published release already exists in the repository, or the tag does in
  any repository other than `VargasDevelopment/newt` (where CI makes it): that
  version is taken for good. Release a new version. In
  `VargasDevelopment/newt` a missing tag, or one on another commit, is the
  first case above.
- `--hop` refuses until `VargasDevelopment/newt` has published the same tag
  with the same three files: publish there first, as launch step 6 says.
- A draft for the tag holds other files than this pair: the tool refuses and
  names them. A draft is never public, so the Lead deletes it by hand
  (`gh release delete vVERSION --repo REPOSITORY`, as the refusal prints, or
  the web page), checks the pair, and reruns `--publish`.
- Creating the draft fails: rerun `--publish` with a new `--out` folder. If a
  draft with all three files was made, the rerun resumes it; a partial draft
  is refused as above. (`gh` itself deletes a draft whose upload fails.)
- The draft's files differ after upload: the tool stops before publishing.
  Delete the draft by hand as above and rerun.
- The publish PATCH fails or its answer is lost: the tool reads the release
  back. Published means success. Still a draft means nothing is public: the
  tool reports it and stops; rerun `--publish` to resume it. If even the
  read-back fails, look at the release page: a draft resumes on rerun, a
  published release stands.
- The `/releases/latest` check fails: the release is public. Never delete or
  edit it; tell the owner, and fix it in a new version.

### Launch: releases move to the public repository

Until launch every release goes to `newt-releases`. At launch (NEWT-77; the
owner approves each public step) they move to the public `VargasDevelopment/newt`,
in this order:

1. **Export.** From the private `main`, once its latest release is tagged:
   `python3 -B tools/clean_export.py --rev origin/main --out DIR --terms TERMS
   --commit "$(git config user.name) <6808194+VargasDevelopment@users.noreply.github.com>"`,
   run in the owner's checkout: the root commit carries the owner's name with
   the GitHub no-reply e-mail. After a clean scan it prints the root commit. `LICENSE` names the
   copyright holder, so a TERMS file that lists the owner's name also needs
   one `allow<TAB>LICENSE<TAB>TERM` line for each such term, or the scan
   fails on `LICENSE`.
2. **The root.** Before its first CI run, set the public repository's `NEWT_RUST_ACQUISITION_APPROVED` variable to `sha256:7dbaa01b1773832093cfd8e64867b6aa0c253c85aa8132fd809eebd8ab796ca0` (the same owner-approved value; see [CI.md](CI.md)).
   Push it as the public repository's `main`. Its public CI push
   run passes and the release-tag job tags the root with the version it
   carries, the private repository's last release. That tag only marks the
   exported source: `newt-releases` already has that version, so it is never
   published. Everything after it builds on that tag. (Exported from a later
   commit than that release's merge, the root holds newer source under the
   same version; exporting right after a release keeps them equal.)
3. **The flip, in the release pull request.** A release branch from `main`
   whose first commit, `feat(update): read updates from the public source
   repository`, moves two lines: `LATEST_RELEASE_URL` in
   `rust/core/src/update_online.rs` to `https://api.github.com/repos/VargasDevelopment/newt/releases/latest`, and the written
   offer in `docs/NOTICES.txt` to `https://github.com/VargasDevelopment/newt/issues`
   (`tests/release_publish_test.py` fails if only one moves). README's
   Install section already points at the public releases, the install page
   and [INSTALL.md](INSTALL.md) (#365). Then What's new and the version bump
   as in [Prepare a reviewed release](#prepare-a-reviewed-release); the owner picks
   the version. Run the full tier, and the release gate on the planned merge
   from the root:
   `python3 -B tools/release_tools.py --rev-range "$(git rev-list --max-parents=0 HEAD)..MERGE"`.
4. **The release merge and its tag.** The owner merges it; CI tags it.
5. **The bench receipt**, as in [Steps](#steps).
6. **Publish to the public repository:** `release_publish.py --publish`, as
   in [Steps](#steps). The tool reads `VargasDevelopment/newt` from the
   release commit. CI has already tagged the release merge there, so the
   tool requires that tag to name the build commit, and still refuses an
   existing release.
7. **The hop:** the same command with `--hop`, first as a dry run, then with
   `--publish`. It publishes the same checked release to `newt-releases`. It
   refuses unless the firmware reads `VargasDevelopment/newt` and
   `VargasDevelopment/newt` has published that tag (not a draft) with the
   same `newt.bin`, `newt.manifest` and `NOTICES.txt` (compared by the
   GitHub API's SHA-256 digests; read only). Devices on older releases find
   it there, install it, and from then on read the public repository. It is
   the last release `newt-releases` gets: publish nothing there afterwards.
8. **Never archive `newt-releases`.** Releases up to the hop promise source
   on request through its issues for at least three years, and archiving
   makes issues read-only. Keep the repository and its issues open, so
   devices that never updated still find the hop and old offers stay
   answerable. Add a short note to its README pointing to
   `VargasDevelopment/newt` for new releases, the install page and new
   issues.
9. **Enable Pages:** Settings, Pages, Source: GitHub Actions. In Settings,
   Environments, `github-pages`, allow the tags `v*` as well: by default it
   allows only `main`, and a release's run runs on its tag. Step 6 ran before
   Pages existed, so its deploy failed; run the workflow once by hand
   (`gh workflow run install-page.yml --repo VargasDevelopment/newt`) and
   check the page offers the release's version.

## Install page

The install page ([092](092-install-page.md), `installer/`) is served by the
public repository's GitHub Pages, `https://vargasdevelopment.github.io/newt/`.
A web page cannot download a release asset itself: GitHub answers every step
of a release download (github.com's two redirects and the asset host) without
an `Access-Control-Allow-Origin` header (checked 2026-11-05 with an `Origin:`
header), so a browser refuses it. The page therefore carries its own copy of
the latest release's `newt.bin`, `newt.manifest` and `NOTICES.txt`.

`.github/workflows/install-page.yml` runs on each published release, and by
hand. Its build job reads the latest release, downloads those three files,
checks out that release's tag and builds the page (`pnpm build`, which
refuses a pair whose size, SHA-256 or identity disagree, as the page itself
checks it). Its deploy job publishes the site with GitHub's Pages actions.
Only the deploy job writes (`pages: write`, `id-token: write`); there is no
secret, and it runs only in the public repository. So the page follows each
release with no step of its own. If a deploy fails, rerun it in Actions; the
page keeps serving the previous release meanwhile. To try the same site
before launch, see `installer/README.md`, "Try the site locally".

## Releases in the public repository

- **Who.** The Lead prepares each release as a pull request (What's new and
  the version bump). The owner alone merges, outside contributions included.
  CI tags the release merge. The Lead builds from the tag, makes the bench
  install check and publishes. Nobody else publishes.
- **Outside contributions.** A pull request from anyone, forks included, runs
  public CI ([CI.md](CI.md), "Public CI") with a read-only token and no
  secret: the same gate as the owner's own work, at the local tier (every
  positive test plus the negative controls of the tests it changes). Every
  commit must be a Conventional Commit, which the gate checks, and firmware
  changes follow the same memory policy and records. The owner merges with
  an explicit Conventional subject and both parents.
- **Trust.** HTTPS to GitHub and the manifest's SHA-256. Releases are not
  signed or attested: a signing key would be one more credential to guard,
  and the notes, the manifest and the tag already name the exact source
  commit, build ID and image SHA-256, so anyone can rebuild and compare.
- **What stays manual.** Preparing a release, the bench install check and its
  receipt, publishing (`release_publish.py --publish`), the launch hop and
  `newt-releases`' README note, and Pages settings. CI's only writes are the release tag and the
  Pages deployment. CI never publishes a release, and releases need no new
  secret, token or credential.
