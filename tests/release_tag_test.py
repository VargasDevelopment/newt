"""tools/release_tag.py, the one tool that makes release tags, in disposable repos
with a local bare origin and a stub gh; never tags or pushes the checkout."""
import io
import itertools
import json
from contextlib import redirect_stdout
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_tools_test import ReleaseFixture, release
import release_tag


class ReleaseTagTests(ReleaseFixture):
    RUN_IDS = itertools.count(40000000001)

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


if __name__ == "__main__":
    unittest.main()
