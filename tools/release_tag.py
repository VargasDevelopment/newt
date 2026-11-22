"""Tag the newest release on origin/main once its exact-source CI passed; the one tool that makes release tags.

The release is the oldest commit on origin/main's first-parent history with the
tip's version: its reviewed release merge. The annotated tag goes to origin as
exactly that one ref: never --tags, never forced, never moved or deleted.
--dry-run makes every check and prints the plan, but creates and pushes nothing.
CI's release-tag job runs it with --ci-run after its own gate job passed; by
hand it is the fallback."""
import argparse
import json
import re
import subprocess
import sys
import tomllib

from release_tools import DESTINATION, ROOT, committed_release, git_output, resolve_commit

REPOSITORY = "VargasDevelopment/newt"
# Each CI workflow and its gate job; that job's success, not the whole run's, is
# the evidence. Only the private repository runs fresh-gates.yml and only the
# public one runs public-ci.yml (docs/CI.md, "Public CI").
GATES = {"fresh-gates.yml": "fresh-gates", "public-ci.yml": "gate"}


def config_version(root, commit):
    """The .cz.toml version at a commit, or None where the file does not exist."""
    shown = subprocess.run(["git", "show", f"{commit}:.cz.toml"], cwd=root, capture_output=True, text=True)
    if shown.returncode:
        return None
    return tomllib.loads(shown.stdout)["tool"]["commitizen"]["version"]


def release_commit(root):
    """The oldest commit with the tip's version on origin/main's first-parent history."""
    history = git_output(root, "rev-list", "--first-parent", DESTINATION).split()
    version = config_version(root, history[0])
    release = history[0]
    for commit in history[1:]:
        if config_version(root, commit) != version:
            break
        release = commit
    return release


def origin_refs(root, *names):
    """Origin's current commit or tag object for each listed ref that exists."""
    refs = {}
    for line in git_output(root, "ls-remote", "origin", *names).splitlines():
        object_name, ref = line.split("\t")
        refs[ref] = object_name
    return refs


def gh_api(path):
    """One read-only GitHub API answer."""
    answer = subprocess.run(["gh", "api", path], capture_output=True, text=True)
    if answer.returncode:
        raise ValueError(f"Could not read {path} with gh: {answer.stderr.strip()}")
    return json.loads(answer.stdout)


def gate_passed(run, gate_job):
    """Whether the run's gate job, in its latest attempt, succeeded on the run's commit.

    The run's own conclusion also counts its release-tag job, so a failed or
    cancelled tag job must not hide a passed gate from the manual fallback."""
    jobs = gh_api(f"repos/{REPOSITORY}/actions/runs/{run['id']}/jobs?filter=latest")["jobs"]
    return any(job["name"] == gate_job and job["head_sha"] == run["head_sha"] and job["conclusion"] == "success"
               for job in jobs)


def exact_source_ci(commit, run_id=None):
    """The URL of a push run on main for exactly this commit whose gate job succeeded.

    Only push runs count: a manual run may test an approved_commit other than its
    head_sha. With run_id (CI's own release-tag job) only that run counts; it is
    still in progress, so its gate job is the evidence."""
    for workflow, gate_job in GATES.items():
        runs = gh_api(f"repos/{REPOSITORY}/actions/workflows/{workflow}/runs?head_sha={commit}&per_page=100")
        for run in runs["workflow_runs"]:
            if (run["head_sha"] == commit and run["event"] == "push" and run["head_branch"] == "main"
                    and run_id in (None, run["id"]) and gate_passed(run, gate_job)):
                return run["html_url"]
    named = f" {run_id}" if run_id else ""
    gates = " or ".join(f"{workflow} push run{named} whose {job} job succeeded" for workflow, job in GATES.items())
    raise ValueError(f"No {gates} on main for {commit}; it cannot be tagged yet")


def tag_message(version, commit, ci_url):
    return (f"Newt firmware {version}\n\nSource: {commit}\nExact-source CI: {ci_url}\n"
            "The tag itself makes no hardware acceptance claim.\n")


def tag_release(root=ROOT, dry_run=False, ci_run=None):
    """ci_run: (run id, the commit its gate checked) from CI's release-tag job.

    That job tags only when its run checked the release itself; any other
    commit is not a release, so it says so and changes nothing."""
    run_id, checked = ci_run or (None, None)
    if ci_run is not None:
        if not (re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[0-9a-f]{40}", checked)):
            raise ValueError(f"--ci-run needs a run id and a full commit, not {run_id!r} {checked!r}")
        run_id = int(run_id)
    tip = resolve_commit(root, DESTINATION)
    if tip is None:
        raise ValueError("Missing origin/main: git fetch origin")
    release = release_commit(root)
    if checked not in (None, release):
        print(f"Nothing to tag: this run checked {checked}, not the release of {config_version(root, release)}, "
              f"{release} (the oldest origin/main commit with that version)")
        return
    version, tag = committed_release(root, release)
    ref = "refs/tags/" + tag
    remote = origin_refs(root, "refs/heads/main", ref, ref + "^{}")
    if remote.get("refs/heads/main") != tip:
        raise ValueError("origin/main here is not origin's main: git fetch origin, then rerun")
    local = resolve_commit(root, ref)
    published = remote.get(ref + "^{}", remote.get(ref))
    for where, commit in (("here", local), ("on origin", published)):
        if commit not in (None, release):
            raise ValueError(f"Tag {tag} {where} names {commit}, not the release {release}; tags never move")
    ci_url = exact_source_ci(release, run_id)
    print(f"Release {version}: {release}, the oldest origin/main commit with that version")
    print(f"Exact-source CI: {ci_url}")
    if local is None and published is None:
        message = tag_message(version, release, ci_url)
        print(f"{'Would create' if dry_run else 'Creating'} annotated tag {tag} at {release}:\n{message}")
        if not dry_run:
            subprocess.run(["git", "tag", "-a", tag, "-m", message, release], cwd=root, check=True)
    if published is None:
        print(f"{'Would push' if dry_run else 'Pushing'} exactly {ref} to origin")
        if not dry_run:
            subprocess.run(["git", "push", "--no-follow-tags", "origin", f"{ref}:{ref}"], cwd=root, check=True)
            pushed = origin_refs(root, ref, ref + "^{}")
            if pushed.get(ref + "^{}", pushed.get(ref)) != release:
                raise ValueError(f"After the push, origin's {tag} does not name {release}")
    elif local is None:
        print(f"{'Would fetch' if dry_run else 'Fetching'} origin's {tag}")
        if not dry_run:
            subprocess.run(["git", "fetch", "--no-tags", "origin", f"{ref}:{ref}"], cwd=root, check=True)
    else:
        print(f"{tag} already names {release} here and on origin; nothing to do")
    if dry_run:
        print("DRY RUN: nothing created, fetched or pushed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true", help="check and print the plan; change nothing")
    parser.add_argument("--ci-run", nargs=2, metavar=("RUN_ID", "COMMIT"),
                        help="CI's release-tag job: this run's id and the commit its gate job checked")
    arguments = parser.parse_args()
    tag_release(dry_run=arguments.dry_run, ci_run=arguments.ci_run)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Release tag stopped: {error}", file=sys.stderr)
        sys.exit(1)
