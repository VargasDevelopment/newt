"""docs/ and artifacts/ hold only an explicit list of files (AGENTS.md,
"Working notes stay out of the repo").

Docs describe the current system; artifacts are inputs the code reads.
Working notes, logs, measurements, evidence, review rounds and plans grew
into a diary here once, so a new file needs a reviewed entry below.
"""
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]

DOCS = frozenset((
    "CI.md", "CODE_HEALTH.md", "INSTALL.md", "NOTICES.txt", "RELEASING.md", "RUST_APP_BUILD.md",
    "UPDATES.md", "bench-flash.md", "dev-usb-bridge.md", "provenance.md", "reading-acceptance.md",
    "software-proof-shadow.md", "toolchain-proposal.md",
    "newt-wordmark-dark.png", "newt-wordmark-light.png",
    "screen-books.png", "screen-home.png", "screen-reader.png", "screen-settings.png",
    "reading-latency-contract.json", "reading-latency-policy.json",
    "toolchain-linux-lock.json", "toolchain-lock.json", "toolchain-python-linux.lock", "toolchain-python.lock",
))
ARTIFACTS = frozenset((
    "code-health-regional-dev-usb.json", "firmware-identity-regional-dev-usb.json",
    "library-ui-review.json", "pocket-pause.newt", "regional-dev-usb-nvs-guard-link-verification.json",
))
ALLOWED = {"docs": DOCS, "artifacts": ARTIFACTS}

HINT = ("Working notes, logs, measurements, evidence, review rounds and plans never go in the "
        "repository: put them in the pull request, its issue, or your own notes outside the checkout. "
        "A doc that describes the current system, or a file the code reads, needs a reviewed entry in "
        "tests/repo_docs_allowlist_test.py.")


def unlisted(paths):
    """Paths under docs/ or artifacts/ that the allowlist does not name."""
    extra = []
    for path in paths:
        top, _, rest = path.partition("/")
        if top in ALLOWED and rest not in ALLOWED[top]:
            extra.append(path)
    return sorted(extra)


def repo_files():
    # Untracked files count too, so a new note fails before it is committed.
    out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard",
                          "--", *ALLOWED], check=True, capture_output=True).stdout
    return [path for path in out.decode().split("\0") if path and (ROOT / path).exists()]


class DocsAllowlistTest(unittest.TestCase):
    def test_only_listed_files(self):
        extra = unlisted(repo_files())
        self.assertEqual(extra, [], f"Unlisted files: {', '.join(extra)}. {HINT}")

    def test_every_listed_file_exists(self):
        # A stale entry would let a removed doc come back without review.
        missing = [f"{top}/{name}" for top, names in ALLOWED.items() for name in sorted(names)
                   if not (ROOT / top / name).is_file()]
        self.assertEqual(missing, [], "Listed but missing; remove the entry")

    def test_negative_control_catches_a_note(self):
        # A dated note, a nested file and an artifact log all fail; other trees are not this test's.
        paths = ["docs/CI.md", "docs/2026-11-22-round-4.md", "docs/plans/next.md",
                 "artifacts/bench-run.log", "tools/notes.md"]
        self.assertEqual(unlisted(paths), ["artifacts/bench-run.log", "docs/2026-11-22-round-4.md", "docs/plans/next.md"])


if __name__ == "__main__":
    unittest.main()
