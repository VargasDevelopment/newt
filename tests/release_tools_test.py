"""Exercise pinned official Commitizen, the release-tag guard and the tag command in
disposable repos with a local bare origin and a stub gh; never tag or push the checkout.
--negative-controls removes each guard in a copy of its tool and requires its test to fail."""
import argparse
import os
import hashlib
import importlib.util
import io
import itertools
import json
from contextlib import ExitStack, redirect_stdout
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--tool-root", type=Path, default=ROOT / "tools")
parser.add_argument("--single", help="run one test (a negative control)")
parser.add_argument("--negative-controls", action="store_true")
args, _ = parser.parse_known_args()
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(args.tool_root))
import ci_gate
import check as host_check
import gate_tier
import release_tag
import release_tools as release
import daily_firmware

PYTHON = release.release_python()
REAL_RUN = subprocess.run


def fixture_version(root, version, major_zero=True):
    """Fixture labels do not depend on the checkout's current/future release."""
    config = root / ".cz.toml"
    text = re.sub(r'^version = "[^"]+"$', f'version = "{version}"', config.read_text(), flags=re.MULTILINE)
    if re.search(r"^major_version_zero =", text, re.MULTILINE):
        text = re.sub(r"^major_version_zero = (?:true|false)$", f"major_version_zero = {str(major_zero).lower()}", text, flags=re.MULTILINE)
    else:
        text = text.replace("[tool.commitizen]", f"[tool.commitizen]\nmajor_version_zero = {str(major_zero).lower()}")
    config.write_text(text)
    header = root / "rust/firmware/native/firmware_version.h"
    header.write_text(re.sub(r'^#define NEWT_FIRMWARE_VERSION "[^"]+"$',
                            f'#define NEWT_FIRMWARE_VERSION "{version}"', header.read_text(), flags=re.MULTILINE))


class LocalLinksTests(unittest.TestCase):
    def test_linked_local_folders_leave_a_checkout_clean(self):
        """Worktrees often link .release-venv and installer/node_modules from another checkout; the
        repository's .gitignore must match a link as well as a folder, or every build is -dirty."""
        for kind in ('link', 'folder'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root, elsewhere = Path(directory) / 'repo', Path(directory) / 'elsewhere'
                elsewhere.mkdir()
                for name in ('.gitignore', 'rust/firmware/native/firmware_version.h'):
                    (root / name).parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(ROOT / name, root / name)
                (root / 'installer').mkdir()

                def git(*args):
                    subprocess.run(['git', '-c', 'user.name=Release fixture', '-c',
                                    'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false', *args],
                                   cwd=root, check=True, capture_output=True)
                git('init', '-q')
                git('add', '-A')
                git('commit', '-qm', 'chore: start')
                for name in ('.release-venv', 'installer/node_modules'):
                    if kind == 'link':
                        (root / name).symlink_to(elsewhere, target_is_directory=True)
                    else:
                        (root / name).mkdir()
                        (root / name / 'kept').write_text('local\n')
                self.assertFalse(release.source_identity(root)['dirty'])


class ReleaseTests(unittest.TestCase):
    RUN_IDS = itertools.count(40000000001)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.environment = {**os.environ, "GIT_AUTHOR_DATE": "2026-01-01T00:00:00+00:00",
                            "GIT_COMMITTER_DATE": "2026-01-01T00:00:00+00:00"}
        for name in (".cz.toml", "requirements-release.txt", "rust/firmware/native/firmware_version.h", "tools/changelog.md.j2"):
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, target)
        fixture_version(self.root, "0.1.0-alpha.0")
        (self.root / ".release-venv").symlink_to(PYTHON.parent.parent, target_is_directory=True)
        (self.root / ".gitignore").write_text(".release-venv/\nartifacts/\n")
        self.git("init", "-q")
        self.git("config", "user.name", "Release fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "core.hooksPath", str(self.root / "no-hooks"))
        self.git("add", ".")
        self.git("commit", "-qm", "chore: start release fixture")
        self.base = self.git("rev-parse", "HEAD").stdout.strip()

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.environment,
                              check=True, capture_output=True, text=True)

    def cz(self, *args, ok=True):
        result = subprocess.run([str(PYTHON), "-I", "-B", "-m", "commitizen", *args],
                                cwd=self.root, env=self.environment, capture_output=True, text=True)
        if ok:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def baseline(self, version, major_zero=True):
        fixture_version(self.root, version, major_zero)
        self.git("add", ".")
        self.git("commit", "--allow-empty", "-qm", "chore: establish fixture version")
        self.git("tag", "v" + version)

    def commit(self, message):
        self.git("commit", "--allow-empty", "-qm", message)

    def head(self):
        return self.git("rev-parse", "HEAD").stdout.strip()

    def release_history(self):
        """main: the tagged 0.1.0-alpha.0 baseline, then the untagged 0.1.0-alpha.1 release merge."""
        self.git("checkout", "-qB", "main")
        self.git("tag", "-a", "v0.1.0-alpha.0", "-m", "Newt firmware 0.1.0-alpha.0")
        tagged = self.head()
        self.git("checkout", "-qb", "release")
        fixture_version(self.root, "0.1.0-alpha.1")
        self.git("add", ".")
        self.git("commit", "-qm", "chore(release): 0.1.0-alpha.0 -> 0.1.0-alpha.1")
        bump = self.head()
        self.commit("build(release): record the release flash")
        record = self.head()
        self.git("checkout", "-q", "main")
        self.git("merge", "--no-ff", "-qm", "chore(release): 0.1.0-alpha.1", "release")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        return SimpleNamespace(tagged=tagged, bump=bump, record=record, release=self.head())

    def branch(self, name, start):
        self.git("checkout", "-qb", name, start)
        self.commit(f"fix(reader): {name} change")
        return self.head()

    def planned_merge(self, destination, change):
        self.git("checkout", "-q", "--detach", destination)
        self.git("merge", "--no-ff", "-qm", "fix(reader): merge the change", change)
        return self.head()

    def assert_untagged(self, head):
        with self.assertRaisesRegex(ValueError, "no tag v0.1.0-alpha.1") as raised:
            release.check_release_tagged(self.root, head)
        self.assertIn(release.TAG_COMMAND, str(raised.exception))

    def published_history(self):
        """release_history plus a later main commit, published to a local bare origin."""
        history = self.release_history()
        self.commit("fix(reader): change after the release")
        remote = tempfile.TemporaryDirectory()
        self.addCleanup(remote.cleanup)
        subprocess.run(["git", "init", "-q", "--bare", remote.name], check=True)
        self.git("remote", "add", "origin", remote.name)
        self.git("push", "-q", "origin", "main", "refs/tags/v0.1.0-alpha.0")
        self.git("fetch", "-q", "origin")
        history.tip = self.head()
        return history

    def origin_listing(self):
        return set(self.git("ls-remote", "origin").stdout.splitlines())

    def stub_gh(self, runs, status=0):
        """Put a gh on PATH that lists these runs and serves each run's "jobs"."""
        if not hasattr(self, "gh"):
            directory = tempfile.TemporaryDirectory()
            self.addCleanup(directory.cleanup)
            self.gh = Path(directory.name)
            script = self.gh / "gh"
            script.write_text('#!/bin/sh\nhere=$(dirname "$0")\nprintf \'%s\\n\' "$*" >> "$here/calls"\n'
                              'case "$2" in\n*/jobs*) run=${2%%/jobs*}; cat "$here/jobs-${run##*/}.json" ;;\n'
                              '*) cat "$here/runs.json" ;;\nesac\nexit "$(cat "$here/status")"\n')
            script.chmod(0o755)
            path = patch.dict(os.environ, {"PATH": f"{self.gh}{os.pathsep}{os.environ['PATH']}"})
            path.start()
            self.addCleanup(path.stop)
        listed = [{key: value for key, value in run.items() if key != "jobs"} for run in runs]
        (self.gh / "runs.json").write_text(json.dumps({"workflow_runs": listed}))
        for run in runs:
            (self.gh / f"jobs-{run['id']}.json").write_text(json.dumps({"jobs": run["jobs"]}))
        (self.gh / "status").write_text(str(status))

    def ci_run(self, commit, gate="success", event="push", branch="main", run_id=None, jobs=None):
        """A completed fresh-gates.yml run whose gate job concluded `gate`."""
        return {"id": run_id or next(self.RUN_IDS), "head_sha": commit, "event": event, "head_branch": branch,
                "conclusion": gate, "html_url": f"https://ci.invalid/{commit[:8]}-{event}-{gate}",
                "jobs": [{"name": "fresh-gates", "head_sha": commit, "conclusion": gate}] if jobs is None else jobs}

    def tag(self, dry_run=False, ci_run=None):
        with redirect_stdout(io.StringIO()) as output:
            release_tag.tag_release(self.root, dry_run=dry_run, ci_run=ci_run)
        return output.getvalue()

    def test_firmware_config_drift_and_invalid_semver_fail(self):
        self.assertEqual(release.check_version(self.root), "0.1.0-alpha.0")
        header = self.root / "rust/firmware/native/firmware_version.h"
        original = header.read_text()
        for bad in ("0.1.0", "01.1.0", "0.1.0-alpha.01", "0.1.0a0", "0.1.0+git", "M1 UX candidate"):
            with self.subTest(version=bad):
                header.write_text(original.replace("0.1.0-alpha.0", bad))
                with self.assertRaises(ValueError):
                    release.check_version(self.root)
        header.write_text(original + '#define NEWT_FIRMWARE_VERSION "0.1.0-alpha.0"\n')
        with self.assertRaises(ValueError):
            release.check_version(self.root)

    def test_a_commit_before_the_header_moved_reads_its_own_header(self):
        # Releases before NEWT-90 bind include/newt/firmware_version.h; the gate
        # still reads their version there, and only at the path each binds.
        config, header = self.root / ".cz.toml", self.root / "rust/firmware/native/firmware_version.h"
        old = self.root / release.HISTORICAL_VERSION_HEADER
        old.parent.mkdir(parents=True)
        self.git("mv", str(header.relative_to(self.root)), release.HISTORICAL_VERSION_HEADER)
        config.write_text(config.read_text().replace(release.VERSION_HEADER, release.HISTORICAL_VERSION_HEADER))
        self.git("commit", "-qam", "chore: an earlier header home")
        self.assertEqual(release.committed_release(self.root, self.head()), ("0.1.0-alpha.0", "v0.1.0-alpha.0"))
        config.write_text(config.read_text().replace(release.HISTORICAL_VERSION_HEADER, "include/newt/other.h"))
        self.git("commit", "-qam", "chore: an unreviewed header binding")
        with self.assertRaisesRegex(ValueError, "version binding"):
            release.committed_release(self.root, self.head())

    def test_message_checks_and_range_reject_malformed_commits(self):
        for message in ("feat(reader): add offline reading", "fix(reader)!: change progress contract",
                        "fix: change progress\n\nBREAKING CHANGE: old records require migration", "docs: describe release gates"):
            self.cz("check", "--message", message)
        for message in ("added stuff", "Feat: case mismatch", "fix:no space", "fix: ",
                        "fixup! feat: temporary", "Merge branch 'unchecked'"):
            self.cz("check", "--message", message, ok=False)
        self.baseline("0.1.0")
        before = self.git("rev-parse", "HEAD").stdout.strip()
        self.commit("feat(reader): add offline reading")
        release.check_commits(self.root, [before + "..HEAD"])
        self.commit("malformed message")
        with self.assertRaises(subprocess.CalledProcessError):
            release.check_commits(self.root, [before + "..HEAD"])

    def test_missing_install_and_wrong_pin_fail_closed(self):
        lock = self.root / "requirements-release.txt"
        lock.write_text("commitizen==0.0.0\n")
        with self.assertRaisesRegex(ValueError, "differs"):
            release.release_python(self.root)
        (self.root / ".release-venv").unlink()
        with self.assertRaisesRegex(ValueError, "Missing"):
            release.release_python(self.root)

    def test_a_merge_may_keep_its_default_subject_and_only_a_merge(self):
        self.baseline("0.1.0")
        first = self.git("rev-parse", "HEAD").stdout.strip()
        (self.root / "procedure.py").write_text("STEP = 1\n")
        self.git("add", "procedure.py")
        self.commit("fix(tests): bind ZIP codec before cover evidence starts")
        second = self.git("rev-parse", "HEAD").stdout.strip()
        tree = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        release.check_commits(self.root, [self.base + ".." + second])
        for message, accepted in (
                ("Merge pull request #43 from owner/fix\n\n"
                 "fix(tests): bind ZIP codec before cover evidence starts", True),
                ("Merge branch 'main' into fix", True),
                ("fix(tests): bind ZIP codec before cover evidence starts (#43)", True),
                ("bind ZIP codec before cover evidence starts", False)):
            with self.subTest(message=message):
                merge = self.git("commit-tree", tree, "-p", first, "-p", second,
                                 "-m", message).stdout.strip()
                self.assertEqual(self.git("show", "-s", "--format=%P", merge).stdout.strip(),
                                 first + " " + second)
                self.assertEqual(self.git("rev-parse", merge + "^{tree}").stdout.strip(), tree)
                if accepted:
                    release.check_commits(self.root, [self.base + ".." + merge])
                else:
                    with self.assertRaises(subprocess.CalledProcessError):
                        release.check_commits(self.root, [self.base + ".." + merge])
        # The same subject on a commit that joins nothing would leave a change unclassified.
        single = self.git("commit-tree", tree, "-p", second, "-m", "Merge pull request #44 from owner/fix").stdout.strip()
        with self.assertRaisesRegex(ValueError, "Only a merge may skip"):
            release.check_commits(self.root, [self.base + ".." + single])

    def test_the_gate_checks_the_commits_versioning_reads(self):
        """Commitizen reads the commits since the release tag; with tags, the gate checks
        exactly those. Without tags, it checks what head adds to origin/main."""
        self.git("checkout", "-qB", "main")
        self.commit("unconventional history before the release")
        self.git("tag", "-a", "v0.1.0-alpha.0", "-m", "Newt firmware 0.1.0-alpha.0")
        self.commit("fix(reader): a change on main")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.branch("feature", "main")
        self.assertEqual(release.gate(self.root), ["v0.1.0-alpha.0"])
        self.assertIsNone(release.gate(self.root, tags=False))
        # An unconventional commit on main after the tag: Commitizen reads it.
        self.git("checkout", "-q", "main")
        self.commit("unconventional change on main")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.branch("later", "main")
        with self.assertRaises(subprocess.CalledProcessError):
            release.gate(self.root)
        # main's push CI checked that commit when it landed; a branch's routine run checks its own.
        self.assertIsNone(release.gate(self.root, tags=False))
        self.commit("unconventional change on the branch")
        with self.assertRaises(subprocess.CalledProcessError):
            release.gate(self.root, tags=False)

    def test_a_routine_run_reads_no_tag(self):
        history = self.release_history()
        change = self.branch("feature", history.release)
        self.assert_untagged(change)
        self.assertIsNone(release.gate(self.root, tags=False))
        for tag in self.git("tag", "--list").stdout.split():
            self.git("tag", "-d", tag)
        self.assertIsNone(release.gate(self.root, tags=False))
        with self.assertRaisesRegex(ValueError, "no tag v0.1.0-alpha.1"):
            release.gate(self.root)

    def test_ci_and_only_an_explicit_request_check_the_release_tags(self):
        class Stop(Exception):
            pass
        for flags, expected in (([], ["--no-tags"]), (["--release-tags"], [])):
            with self.subTest(flags=flags), patch.object(host_check, "phase_run", side_effect=Stop) as run:
                arguments = SimpleNamespace(suite="quick", tier="local", release_tags=bool(flags))
                with self.assertRaises(Stop):
                    host_check.gate(arguments)
                command = run.call_args.args[0]
                self.assertEqual((Path(command[2]).name, command[3:]), ("release_tools.py", expected))
        for part in ("all", "0/7"):
            with self.subTest(part=part):
                host = ci_gate.commands(ROOT, sys.executable, "push", part)[0]
                self.assertEqual(host[0], "host")
                self.assertIn("--release-tags", host[1])

    def test_fixture_reset_survives_future_release_and_maturity_policy(self):
        for future in ("0.1.0-alpha.1", "0.2.0-rc.5", "1.2.3"):
            fixture_version(self.root, future, major_zero=False)
            self.assertEqual(release.check_version(self.root), future)
            fixture_version(self.root, "0.1.0-alpha.0")
            self.assertEqual(release.check_version(self.root), "0.1.0-alpha.0")
        config = self.root / ".cz.toml"
        config.write_text(config.read_text().replace("major_version_zero = true\n", ""))
        fixture_version(self.root, "0.1.0-alpha.0")
        self.assertIn("major_version_zero = true", config.read_text())

    def test_classification_drives_versions_and_requires_explicit_breaking(self):
        self.baseline("0.1.0")
        baseline = self.git("rev-parse", "HEAD").stdout.strip()
        for message, expected in (("fix: restore reading", "0.1.1"), ("perf: reduce parse work", "0.1.1"),
                                  ("refactor: simplify state", "0.1.1"), ("feat: add offline shelf", "0.2.0"),
                                  ("fix!: remove old progress records", "0.2.0"),
                                  ("fix: change progress\n\nBREAKING CHANGE: old records require migration", "0.2.0")):
            with self.subTest(message=message):
                self.git("reset", "--hard", baseline)
                self.commit(message)
                self.assertEqual(self.cz("bump", "--get-next").stdout.strip(), expected)
        self.git("reset", "--hard", baseline)
        self.commit("docs: clarify reading")
        self.cz("bump", "--get-next", ok=False)
        self.git("reset", "--hard", baseline)
        fixture_version(self.root, "1.2.3", major_zero=False)
        self.git("add", ".")
        self.git("commit", "-qm", "chore: establish mature fixture")
        self.git("tag", "v1.2.3")
        self.commit("fix!: remove old progress records")
        self.assertEqual(self.cz("bump", "--get-next").stdout.strip(), "2.0.0")

    def test_prerelease_progression_files_only_changelog_and_no_git_mutation(self):
        self.baseline("0.1.0-alpha.0")
        self.commit("fix(reader): restore reading")
        for phase, expected in (("alpha", "0.1.0-alpha.1"), ("beta", "0.1.0-beta.0"), ("rc", "0.1.0-rc.0"),
                                (None, "0.1.0")):
            flags = ["--prerelease", phase] if phase else []
            before = self.git("rev-parse", "HEAD").stdout
            tags = self.git("tag", "--list").stdout
            self.assertEqual(self.cz("bump", "--get-next", *flags).stdout.strip(), expected)
            self.cz("bump", "--version-files-only", "--check-consistency", "--yes", *flags)
            self.assertEqual(release.check_version(self.root), expected)
            changelog = (self.root / "CHANGELOG.md").read_text()
            self.assertIn("## v" + expected, changelog)
            self.assertIn("restore reading", changelog)
            self.assertEqual(self.git("rev-parse", "HEAD").stdout, before)
            self.assertEqual(self.git("tag", "--list").stdout, tags)
            self.git("add", ".")
            self.git("commit", "-qm", "chore(release): fixture candidate")
            self.git("tag", "v" + expected)
            self.commit("fix(reader): restore reading")

    def test_changelog_is_deterministic_without_wall_clock_dates_or_old_history(self):
        self.baseline("0.1.0")
        self.commit("feat(reader): deterministic shelf")
        first = self.cz("changelog", "--dry-run", "--unreleased-version", "v0.2.0").stdout
        self.assertEqual(first, self.cz("changelog", "--dry-run", "--unreleased-version", "v0.2.0").stdout)
        self.assertIn("deterministic shelf", first)
        self.assertNotIn("2026-", first)
        self.assertNotIn("start release fixture", first)

    def test_source_version_is_distinct_from_revision_and_dirty_state(self):
        self.baseline("0.1.0")
        original = release.source_identity(self.root)
        self.assertEqual(len(original["git_revision"]), 40)
        self.assertFalse(original["dirty"])
        self.assertEqual(original["firmware_version"], "0.1.0")
        (self.root / "artifacts").mkdir()
        (self.root / "artifacts/generated.json").write_text("generated evidence")
        self.assertEqual(release.source_identity(self.root), original)
        (self.root / "authored-input.cpp").write_text("new input")
        dirty = release.source_identity(self.root)
        self.assertTrue(dirty["dirty"])
        self.assertEqual(dirty["git_revision"], original["git_revision"])
        self.assertEqual(dirty["firmware_version"], original["firmware_version"])
        self.git("add", "authored-input.cpp")
        self.git("commit", "-qm", "fix: update input")
        changed = release.source_identity(self.root)
        self.assertFalse(changed["dirty"])
        self.assertNotEqual(changed["git_revision"], original["git_revision"])
        self.assertEqual(changed["firmware_version"], original["firmware_version"])

    def test_commit_during_native_clean_is_rejected_before_compilation(self):
        spec = importlib.util.spec_from_file_location("release_native_driver", ROOT / "tools/firmware-build.py")
        driver = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(driver)
        shutil.copyfile(ROOT / 'platformio-rust-app.ini', self.root / 'platformio-rust-app.ini')
        bundle = self.root / '.build/regional-dev-usb/crt/x509_crt_bundle'
        bundle.parent.mkdir(parents=True)
        bundle.write_bytes(b'release certificate fixture')
        with (self.root / '.gitignore').open('a') as stream:
            stream.write('.build/\n.toolchain/\n')
        self.git("add", ".")
        self.git("commit", "-qm", "chore: add native fixture configuration")
        commands = []

        def run(command, **kwargs):
            if command[0] == "git":
                return REAL_RUN(command, **kwargs)
            commands.append(command)
            if command[-2:] == ["-t", "clean"]:
                self.commit("chore: concurrent source checkpoint")
            return subprocess.CompletedProcess(command, 0)

        with ExitStack() as context:
            context.enter_context(patch.object(daily_firmware, 'CRT_BUNDLE_SHA256',
                                                hashlib.sha256(b'release certificate fixture').hexdigest()))
            context.enter_context(patch.object(driver, "ROOT", self.root))
            context.enter_context(patch.object(driver, "TOOLCHAIN", self.root / ".toolchain"))
            context.enter_context(patch.object(driver, "require_toolchain", return_value=self.root / "mock-pio"))
            context.enter_context(patch.object(driver, "build_command", return_value=["mock-pio"]))
            context.enter_context(patch("code_health.read_baseline", return_value={}))
            context.enter_context(patch("code_health.base_failures", return_value=[]))
            context.enter_context(patch.object(subprocess, "run", side_effect=run))
            context.enter_context(patch.object(sys, "argv", ["firmware-build.py", "--clean", "--health-report"]))
            with self.assertRaisesRegex(RuntimeError, "before compilation"):
                driver.main()
        self.assertEqual([command for command in commands if command[0] == "mock-pio"],
                         [["mock-pio", "--project-conf", str(self.root / ".build/regional-dev-usb/platformio.ini"), "-t", "clean"]])
        self.assertFalse((self.root / "artifacts/firmware-identity-regional-dev-usb.json").exists())

    def test_a_planned_merge_onto_an_untagged_release_fails_until_it_is_tagged(self):
        history = self.release_history()
        change = self.branch("feature", history.tagged)
        self.assertEqual(release.check_release_tagged(self.root, change), ["v0.1.0-alpha.0"])
        planned = self.planned_merge(history.release, change)
        self.assert_untagged(planned)
        # The merge procedure names the planned merge by its review range, not HEAD.
        self.git("checkout", "-q", "feature")
        self.assertEqual(release.gate(self.root, f"{self.base}..HEAD"), ["v0.1.0-alpha.0"])
        with self.assertRaisesRegex(ValueError, "no tag v0.1.0-alpha.1"):
            release.gate(self.root, f"{self.base}..{planned}")
        self.git("tag", "-a", "v0.1.0-alpha.1", "-m", "fixture release tag", history.release)
        self.assertEqual(release.check_release_tagged(self.root, planned), ["v0.1.0-alpha.1"])
        self.assertEqual(release.gate(self.root, f"{self.base}..{planned}"), ["v0.1.0-alpha.1"])

    def test_a_release_branch_and_its_release_merge_pass_before_their_tag_exists(self):
        history = self.release_history()
        # As the release branch sees it, before its merge: main ends at the tagged baseline.
        self.git("update-ref", "refs/remotes/origin/main", history.tagged)
        for head in (history.bump, history.record):
            self.assertEqual(release.check_release_tagged(self.root, head), ["v0.1.0-alpha.0"])
        planned = self.planned_merge(history.tagged, history.record)
        self.assertEqual(release.gate(self.root, f"{self.base}..{planned}"), ["v0.1.0-alpha.0"])
        # As main's CI sees the release merge itself.
        self.git("update-ref", "refs/remotes/origin/main", history.release)
        self.assertEqual(release.check_release_tagged(self.root, history.release), ["v0.1.0-alpha.0"])

    def test_a_planned_merge_fails_while_origin_main_is_behind_its_destination(self):
        history = self.release_history()
        change = self.branch("feature", history.tagged)
        planned = self.planned_merge(history.release, change)
        # This checkout's origin/main predates the release merge it lands on.
        self.git("update-ref", "refs/remotes/origin/main", history.tagged)
        with self.assertRaisesRegex(ValueError, "origin/main is behind the planned merge's destination"):
            release.gate(self.root, f"{self.base}..{planned}")
        # A plain branch commit's first parent is its own history, not a destination.
        self.assertEqual(release.check_release_tagged(self.root, change, planned_merge=True), ["v0.1.0-alpha.0"])
        self.git("update-ref", "refs/remotes/origin/main", history.release)
        with self.assertRaisesRegex(ValueError, "no tag v0.1.0-alpha.1"):
            release.gate(self.root, f"{self.base}..{planned}")

    def test_merging_an_untagged_release_into_a_branch_fails(self):
        history = self.release_history()
        self.branch("feature", history.tagged)
        self.git("merge", "--no-ff", "-qm", "fix(reader): take the release", history.release)
        self.assert_untagged(self.head())
        self.git("tag", "-a", "v0.1.0-alpha.1", "-m", "fixture release tag", history.release)
        self.assertEqual(release.check_release_tagged(self.root, self.head()), ["v0.1.0-alpha.1"])

    def test_a_branch_from_an_untagged_release_fails(self):
        history = self.release_history()
        first = self.branch("feature", history.release)
        self.assert_untagged(first)
        self.commit("fix(reader): second change")
        self.assert_untagged(self.head())

    def test_a_tag_on_another_version_or_off_main_history_fails(self):
        history = self.release_history()
        change = self.branch("feature", history.release)
        side = self.branch("side", history.record)
        for target, reason in ((history.tagged, "whose version is not 0.1.0-alpha.1"),
                               (history.bump, "not on origin/main's first-parent history"),
                               (side, "not on origin/main's first-parent history")):
            with self.subTest(target=target):
                self.git("tag", "-a", "v0.1.0-alpha.1", "-m", "misplaced fixture tag", target)
                with self.assertRaisesRegex(ValueError, reason):
                    release.check_release_tagged(self.root, change)
                self.git("tag", "-d", "v0.1.0-alpha.1")

    def test_the_baseline_tag_may_follow_its_version_on_main(self):
        # v0.1.0-alpha.0's shape: the tagged commit's parent already has the same version.
        self.git("checkout", "-qB", "main")
        self.git("add", ".")
        self.git("commit", "--allow-empty", "-qm", "chore(release): establish the baseline changelog")
        self.git("tag", "-a", "v0.1.0-alpha.0", "-m", "Newt firmware 0.1.0-alpha.0")
        main = self.planned_merge("main", self.branch("feature", "main"))
        self.git("update-ref", "refs/remotes/origin/main", main)
        later = self.branch("later", main)
        self.assertEqual(release.check_release_tagged(self.root, later), ["v0.1.0-alpha.0"])
        self.assertEqual(release.check_release_tagged(self.root, self.planned_merge(main, later)),
                         ["v0.1.0-alpha.0"])

    def export_shaped_main(self):
        """main with only files a clean export writes: the fixture's venv link leaves the index."""
        self.git("checkout", "-qB", "main")
        self.git("rm", "-q", "--cached", ".release-venv")
        (self.root / ".git/info/exclude").write_text(".release-venv\n")
        self.git("commit", "--amend", "-qm", "chore: start release fixture")

    def snapshot_trailer(self, revision="a" * 40, digest=None):
        """The trailer clean_export.py prints for this checkout's HEAD tree."""
        return f"{release.SNAPSHOT_TRAILER}: {revision} sha256:{digest or release.snapshot_digest(self.root, 'HEAD')}"

    def assert_no_snapshot_boundary(self, head="HEAD"):
        self.git("update-ref", "refs/remotes/origin/main", self.head())
        self.assertIsNone(release.snapshot_root(self.root))
        root = self.git("rev-list", "--max-parents=0", "HEAD").stdout.strip()
        with self.assertRaisesRegex(ValueError, "has no parent"):
            release.check_release_tagged(self.root, root)

    def test_a_snapshot_root_is_the_boundary_only_when_its_tree_reproduces_its_trailer(self):
        self.export_shaped_main()
        self.git("commit", "--amend", "-qm", "chore: start from a clean export", "-m", self.snapshot_trailer())
        snapshot = self.head()
        self.git("update-ref", "refs/remotes/origin/main", snapshot)
        self.assertEqual((release.snapshot_root(self.root), release.gate(self.root)), (snapshot, []))
        self.assertIsNone(release.gate(self.root, tags=False))
        # After the root, the release rules apply exactly as after ADOPTION_BASE.
        self.commit("fix(reader): change")
        self.assertIsNone(release.gate(self.root, tags=False))
        with self.assertRaisesRegex(ValueError, "no tag v0.1.0-alpha.0"):
            release.check_release_tagged(self.root)
        self.git("tag", "-a", "v0.1.0-alpha.0", "-m", "Newt firmware 0.1.0-alpha.0", snapshot)
        self.assertEqual(release.gate(self.root), ["v0.1.0-alpha.0"])
        self.commit("malformed message")
        with self.assertRaises(subprocess.CalledProcessError):
            release.gate(self.root)
        # A shallow clone cannot prove where its history starts.
        clone = self.root.parent / (self.root.name + "-shallow")
        self.addCleanup(shutil.rmtree, clone, True)
        subprocess.run(["git", "clone", "-q", "--depth", "1", "file://" + str(self.root), str(clone)], check=True)
        self.assertIsNone(release.snapshot_root(clone))

    def test_a_malformed_or_mismatched_snapshot_trailer_is_no_boundary(self):
        self.export_shaped_main()
        exported = release.snapshot_digest(self.root, "HEAD")
        for label, trailer in (("garbage", f"{release.SNAPSHOT_TRAILER}: nonsense"),
                               ("wrong digest", self.snapshot_trailer(digest="0" * 64)),
                               ("short revision", self.snapshot_trailer(revision="a" * 12)),
                               ("repeated", self.snapshot_trailer() + "\n" + self.snapshot_trailer())):
            with self.subTest(label):
                self.git("commit", "--amend", "-qm", "chore: start from a clean export", "-m", trailer)
                self.assert_no_snapshot_boundary()
        with self.subTest("the exported digest on a different tree"):
            (self.root / "added.txt").write_text("not exported\n")
            self.git("add", "added.txt")
            self.git("commit", "--amend", "-qm", "chore: start from a clean export",
                     "-m", self.snapshot_trailer(digest=exported))
            self.assert_no_snapshot_boundary()
        with self.subTest("a valid trailer on a later commit"):
            self.git("commit", "--amend", "-qm", "chore: start release fixture")
            self.git("commit", "--allow-empty", "-qm", "chore: claim a snapshot", "-m", self.snapshot_trailer())
            self.assertEqual(release.snapshot_digest(self.root, "HEAD"), release.snapshot_digest(self.root, "HEAD~1"))
            self.assert_no_snapshot_boundary()

    def test_the_guard_fails_closed_without_origin_main_or_tags(self):
        self.git("checkout", "-qB", "main")
        self.commit("fix(reader): change")
        with self.assertRaisesRegex(ValueError, "needs origin/main"):
            release.check_release_tagged(self.root)
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        with self.assertRaisesRegex(ValueError, "git fetch origin tag v0.1.0-alpha.0"):
            release.check_release_tagged(self.root)

    def test_tag_dry_run_checks_everything_and_changes_nothing(self):
        history = self.published_history()
        self.stub_gh([self.ci_run(history.release)])
        before = self.origin_listing(), self.git("show-ref").stdout
        output = self.tag(dry_run=True)
        self.assertEqual((self.origin_listing(), self.git("show-ref").stdout), before)
        for expected in (f"Release 0.1.0-alpha.1: {history.release}", self.ci_run(history.release)["html_url"],
                         "Would create annotated tag v0.1.0-alpha.1",
                         "Would push exactly refs/tags/v0.1.0-alpha.1", "DRY RUN"):
            self.assertIn(expected, output)

    def test_tag_pushes_only_the_release_tag_and_a_rerun_changes_nothing(self):
        history = self.published_history()
        url = self.ci_run(history.release)["html_url"]
        self.stub_gh([self.ci_run(history.tip), self.ci_run(history.release)])
        before = self.origin_listing()
        self.tag()
        tag = self.git("rev-parse", "refs/tags/v0.1.0-alpha.1").stdout.strip()
        self.assertEqual(self.git("cat-file", "-t", tag).stdout.strip(), "tag")
        self.assertEqual(self.git("rev-parse", "v0.1.0-alpha.1^{commit}").stdout.strip(), history.release)
        self.assertEqual(self.git("tag", "-l", "--format=%(contents)", "v0.1.0-alpha.1").stdout.strip(),
                         release_tag.tag_message("0.1.0-alpha.1", history.release, url).strip())
        self.assertEqual(self.origin_listing() - before, {f"{tag}\trefs/tags/v0.1.0-alpha.1",
                                                          f"{history.release}\trefs/tags/v0.1.0-alpha.1^{{}}"})
        after = self.origin_listing(), self.git("show-ref").stdout
        self.assertIn("nothing to do", self.tag())
        self.assertEqual((self.origin_listing(), self.git("show-ref").stdout), after)
        self.assertIn(f"head_sha={history.release}", (self.gh / "calls").read_text())

    def test_tag_finishes_an_interrupted_run_without_a_second_tag(self):
        history = self.published_history()
        self.stub_gh([self.ci_run(history.release)])
        self.git("tag", "-a", "v0.1.0-alpha.1", "-m", "made before a failed push", history.release)
        tag = self.git("rev-parse", "refs/tags/v0.1.0-alpha.1").stdout.strip()
        self.assertIn("Pushing exactly refs/tags/v0.1.0-alpha.1", self.tag())
        self.assertIn(f"{tag}\trefs/tags/v0.1.0-alpha.1", self.origin_listing())
        self.git("tag", "-d", "v0.1.0-alpha.1")
        self.assertIn("Fetching origin's v0.1.0-alpha.1", self.tag())
        self.assertEqual(self.git("rev-parse", "refs/tags/v0.1.0-alpha.1").stdout.strip(), tag)

    def test_tag_refuses_a_tag_on_another_commit_or_a_stale_origin_main(self):
        history = self.published_history()
        self.stub_gh([self.ci_run(history.release)])
        self.git("update-ref", "refs/remotes/origin/main", history.release)
        with self.assertRaisesRegex(ValueError, "git fetch origin"):
            self.tag()
        self.git("update-ref", "refs/remotes/origin/main", history.tip)
        # A bare `cz bump` would have tagged the release branch's commit.
        self.git("tag", "v0.1.0-alpha.1", history.bump)
        with self.assertRaisesRegex(ValueError, "here names .*never move"):
            self.tag()
        self.git("push", "-q", "origin", "refs/tags/v0.1.0-alpha.1")
        self.git("tag", "-d", "v0.1.0-alpha.1")
        with self.assertRaisesRegex(ValueError, "on origin names .*never move"):
            self.tag()
        self.assertIsNone(release.resolve_commit(self.root, "refs/tags/v0.1.0-alpha.1"))
        self.assertIn(f"{history.bump}\trefs/tags/v0.1.0-alpha.1", self.origin_listing())

    def test_tag_refuses_without_a_successful_exact_source_push_run(self):
        history = self.published_history()
        before = self.origin_listing()
        for runs, status in (([], 0), ([self.ci_run(history.release, gate="failure")], 0),
                             ([self.ci_run(history.release, event="workflow_dispatch")], 0),
                             ([self.ci_run(history.tip)], 0), ([self.ci_run(history.release)], 1)):
            with self.subTest(runs=runs, status=status):
                self.stub_gh(runs, status)
                with self.assertRaisesRegex(ValueError, "fresh-gates.yml"):
                    self.tag(dry_run=False)
                self.assertIsNone(release.resolve_commit(self.root, "refs/tags/v0.1.0-alpha.1"))
        self.assertEqual(self.origin_listing(), before)

    def test_the_manual_fallback_accepts_a_passed_gate_whose_tag_job_failed(self):
        history = self.published_history()
        run = {**self.ci_run(history.release), "conclusion": "failure"}
        self.stub_gh([run])
        self.tag()
        self.assertEqual(self.git("rev-parse", "v0.1.0-alpha.1^{commit}").stdout.strip(), history.release)
        self.assertIn(f"Exact-source CI: {run['html_url']}",
                      self.git("tag", "-l", "--format=%(contents)", "v0.1.0-alpha.1").stdout)

    def test_ci_tags_the_release_its_own_run_checked_once_its_gate_job_passed(self):
        history = self.published_history()
        # In progress: the release-tag job runs inside it, so only its gate job has concluded.
        this = {**self.ci_run(history.release), "conclusion": None}
        self.stub_gh([self.ci_run(history.release, event="workflow_dispatch"), this])
        before = self.origin_listing()
        self.assertIn(f"Exact-source CI: {this['html_url']}", self.tag(ci_run=(str(this["id"]), history.release)))
        tag = self.git("rev-parse", "refs/tags/v0.1.0-alpha.1").stdout.strip()
        self.assertEqual(self.origin_listing() - before, {f"{tag}\trefs/tags/v0.1.0-alpha.1",
                                                          f"{history.release}\trefs/tags/v0.1.0-alpha.1^{{}}"})
        self.assertIn(f"actions/runs/{this['id']}/jobs?filter=latest", (self.gh / "calls").read_text())
        after = self.origin_listing(), self.git("show-ref").stdout
        self.assertIn("nothing to do", self.tag(ci_run=(str(this["id"]), history.release)))
        self.assertEqual((self.origin_listing(), self.git("show-ref").stdout), after)

    def test_ci_changes_nothing_when_its_run_did_not_check_the_release(self):
        history = self.published_history()
        self.stub_gh([])
        before = self.origin_listing(), self.git("show-ref").stdout
        for checked in (history.tip, history.bump, history.tagged):
            with self.subTest(checked=checked):
                output = self.tag(ci_run=("40000000001", checked))
                self.assertIn(f"Nothing to tag: this run checked {checked}, not the release of 0.1.0-alpha.1, "
                              f"{history.release}", output)
        self.assertEqual((self.origin_listing(), self.git("show-ref").stdout), before)
        self.assertFalse((self.gh / "calls").exists())

    def test_ci_refuses_all_but_its_own_push_run_on_main_whose_gate_job_passed(self):
        history = self.published_history()
        this = 40000000000
        before = self.origin_listing()
        for runs, status in (([], 0), ([self.ci_run(history.release)], 0),
                             ([self.ci_run(history.release, event="workflow_dispatch", run_id=this)], 0),
                             ([self.ci_run(history.release, branch="topic", run_id=this)], 0),
                             ([self.ci_run(history.tip, run_id=this)], 0),
                             ([self.ci_run(history.release, gate="failure", run_id=this)], 0),
                             ([self.ci_run(history.release, gate=None, run_id=this)], 0),
                             ([self.ci_run(history.release, run_id=this, jobs=[
                                 {"name": "fresh-gates", "head_sha": history.tip, "conclusion": "success"}])], 0),
                             ([self.ci_run(history.release, run_id=this, jobs=[
                                 {"name": "release-tag", "head_sha": history.release, "conclusion": "success"}])], 0),
                             ([self.ci_run(history.release, run_id=this)], 1)):
            with self.subTest(runs=runs, status=status):
                self.stub_gh(runs, status)
                with self.assertRaisesRegex(ValueError, "fresh-gates"):
                    self.tag(ci_run=(str(this), history.release))
                self.assertIsNone(release.resolve_commit(self.root, "refs/tags/v0.1.0-alpha.1"))
        for ci_run in (("", history.release), ("0", history.release), ("4e1", history.release),
                       (str(this), ""), (str(this), history.release[:12]), (str(this), "main")):
            with self.subTest(ci_run=ci_run), self.assertRaisesRegex(ValueError, "--ci-run needs"):
                self.tag(ci_run=ci_run)
        self.assertEqual(self.origin_listing(), before)


# (tool, old, new, test): removing the guard must make its test fail.
CONTROLS = (
    ("release_tools.py", "            raise ValueError(f\"Release {version} ({base[:12]} on origin/main) has no tag",
     "            continue\n            raise ValueError(f\"Release {version} ({base[:12]} on origin/main) has no tag",
     "test_the_guard_fails_closed_without_origin_main_or_tags"),
    ("release_tools.py", "        if committed_release(root, tagged)[0] != version:", "        if False:",
     "test_a_tag_on_another_version_or_off_main_history_fails"),
    ("release_tools.py", "        if tagged not in mainline:", "        if False:",
     "test_a_tag_on_another_version_or_off_main_history_fails"),
    ("release_tools.py", "    for rev_range in ranges:", "    for rev_range in ranges[:0]:",
     "test_message_checks_and_range_reject_malformed_commits"),
    ("release_tools.py", "        ranges = [f\"refs/tags/{tag}..{head}\" for tag in built_on]", "        ranges = []",
     "test_the_gate_checks_the_commits_versioning_reads"),
    ("release_tools.py", "        ranges = [f\"{base}..{head}\" for base in destination_bases(root, head)]", "        ranges = []",
     "test_the_gate_checks_the_commits_versioning_reads"),
    ("release_tools.py", "        if unmerged:", "        if False:", "test_a_merge_may_keep_its_default_subject_and_only_a_merge"),
    ("check.py", '*([] if args.release_tags else ["--no-tags"])', '"--no-tags"',
     "test_ci_and_only_an_explicit_request_check_the_release_tags"),
    ("ci_gate.py", "'--release-tags', ", "", "test_ci_and_only_an_explicit_request_check_the_release_tags"),
)


def negative_controls():
    for tool, old, new, test in CONTROLS:
        with tempfile.TemporaryDirectory(prefix="release-control-") as scratch:
            source = (args.tool_root / tool).read_text()
            if source.count(old) != 1:
                raise RuntimeError("Negative control anchor drift: " + test)
            # A copied release_tools.py finds the release venv beside its own tools folder.
            tools = Path(scratch) / "tools"
            tools.mkdir()
            (tools / tool).write_text(source.replace(old, new))
            (Path(scratch) / ".release-venv").symlink_to(PYTHON.parent.parent)
            shutil.copyfile(ROOT / "requirements-release.txt", Path(scratch) / "requirements-release.txt")
            result = subprocess.run([sys.executable, "-B", __file__, "--tool-root", tools, "--single", test],
                                    capture_output=True, text=True)
            if result.returncode == 0 or "FAIL:" not in result.stderr:
                raise RuntimeError("Guard removal was not caught: " + test + "\n" + result.stderr)
            print("PASS guard-removal control: " + test, flush=True)


if __name__ == "__main__":
    loader = unittest.defaultTestLoader
    suite = (loader.loadTestsFromName(args.single, ReleaseTests) if args.single
             else loader.loadTestsFromModule(sys.modules[__name__]))
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        sys.exit(1)
    if args.negative_controls or (not args.single and gate_tier.negative_controls(__file__)):
        negative_controls()
