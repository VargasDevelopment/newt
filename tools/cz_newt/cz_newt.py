"""Newt's Commitizen rules (.cz.toml `name = "./tools/cz_newt"`): Conventional Commits,
with a changelog of one line per change that reached main.

Bump and `cz check` are Conventional Commits' own: bump still classifies every
commit since the release tag. The changelog keeps only the subject of each
commit on HEAD's first-parent history: a pull request's merge, whose subject is
the pull request's title, and a commit made directly on main, such as a release
bump. A merge's body and the commits it brought in through its second parent
list nothing. A plain `git merge` keeps a `Merge ...` subject, which is no
changelog type, so its change lists nothing either.

tools/release_tools.py installs this file into the release venv
(docs/RELEASING.md) and refuses an installed copy that differs from it."""
import re
import subprocess

from commitizen.cz.conventional_commits import ConventionalCommitsCz


class NewtCz(ConventionalCommitsCz):
    def __init__(self, config):
        super().__init__(config)
        self.mainline = None
        self.listed = set()

    def changelog_message_builder_hook(self, message, commit):
        # Commitizen parses a commit's subject, then each body paragraph; only a
        # message equal to the subject's own parse is the subject.
        if self.mainline is None:
            self.mainline = set(subprocess.run(["git", "rev-list", "--first-parent", "HEAD"], check=True,
                                               capture_output=True, text=True).stdout.split())
        subject = re.match(self.commit_parser, commit.title)
        if (commit.rev not in self.mainline or commit.rev in self.listed or subject is None
                or any(message.get(key) != value for key, value in subject.groupdict().items())):
            return None
        self.listed.add(commit.rev)
        return message
