"""What files may live in the repository, by kind (AGENTS.md, "Working notes
stay out of the repo").

- Working notes never go in: no logs, numbered or dated notes, review-round
  reports, handoffs or plans. They once grew into a diary in docs/, so a name
  that reads as one fails before review.
- Ordinary docs (Markdown and images directly in docs/) need no entry.
- Every other file under docs/ or artifacts/ is an input code reads, or the
  license notices, so it needs a reviewed entry in INPUTS.
"""
from pathlib import Path, PurePosixPath
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]

INPUTS = frozenset((
    "docs/NOTICES.txt", "docs/reading-latency-contract.json", "docs/reading-latency-policy.json",
    "docs/toolchain-linux-lock.json", "docs/toolchain-lock.json", "docs/toolchain-python-linux.lock",
    "docs/toolchain-python.lock",
    "artifacts/code-health-regional-dev-usb.json", "artifacts/firmware-identity-regional-dev-usb.json",
    "artifacts/library-ui-review.json", "artifacts/pocket-pause.newt",
    "artifacts/regional-dev-usb-nvs-guard-link-verification.json",
))
INPUT_FOLDERS = ("docs", "artifacts")
ORDINARY_DOC = (".md", ".png", ".jpg")

# File names no product file needs, anywhere outside vendor/.
NOTE_NAMES = (
    ("a numbered note", re.compile(r"^\d{3}-.*\.md$")),
    ("a review round", re.compile(r"(?:^|[-_.])round-?\d", re.IGNORECASE)),
    ("a dated note", re.compile(r"\d{4}-\d{2}-\d{2}")),
    ("a log", re.compile(r"\.log$", re.IGNORECASE)),
)
# Words that name a working note in docs/ or artifacts/. Code elsewhere may use
# them (a session type, a notes header), so they apply only there.
NOTE_WORDS = re.compile(r"(?:^|[-_.])(?:notes?|logs?|evidence|handoff|plans?)(?:[-_.]|$)", re.IGNORECASE)

HINT = ("Working notes, logs, measurements, evidence, review rounds and plans never go in the repository: "
        "put them in the pull request, its issue, or your own notes outside the checkout.")


def working_notes(paths):
    """(path, why) for each path whose name reads as a working note."""
    found = []
    for path in paths:
        parts = PurePosixPath(path).parts
        if parts[0] == "vendor":
            continue
        why = next((kind for kind, pattern in NOTE_NAMES if pattern.search(parts[-1])), None)
        if why is None and parts[0] in INPUT_FOLDERS and NOTE_WORDS.search(PurePosixPath(path).stem):
            why = "a working note"
        if why:
            found.append((path, why))
    return sorted(found)


def unlisted_inputs(paths):
    """Paths under docs/ or artifacts/ that are neither an ordinary doc nor listed."""
    extra = []
    for path in paths:
        parts = PurePosixPath(path).parts
        ordinary = len(parts) == 2 and parts[0] == "docs" and PurePosixPath(path).suffix in ORDINARY_DOC
        if parts[0] in INPUT_FOLDERS and not ordinary and path not in INPUTS:
            extra.append(path)
    return sorted(extra)


def repo_files():
    # Untracked files count too, so a new note fails before it is committed.
    out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                         check=True, capture_output=True).stdout
    return [path for path in out.decode().split("\0") if path and (ROOT / path).exists()]


class WorkingNotesTest(unittest.TestCase):
    def test_no_working_notes(self):
        found = working_notes(repo_files())
        self.assertEqual(found, [], f"{', '.join(f'{path} ({why})' for path, why in found)}. {HINT}")

    def test_negative_control_catches_each_kind_of_note(self):
        notes = ["docs/051-sd-reading-state.md", "tools/042-plan.md", "NEWT42-round-3.md", "docs/review_round2.md",
                 "docs/2026-11-22-bench.md", "artifacts/bench-run.log", "rust/out.LOG", "docs/handoff.md",
                 "docs/reader-notes.md", "docs/plans.md", "artifacts/flash-evidence.json"]
        ordinary = ["docs/CI.md", "docs/background2.png", "docs/planner-guide.md", "rust/device-ui/whats_new_notes.h",
                    "rust/tests/wake_ui/evidence.rs", "vendor/sdk/2026-01-01.log", "tests/fixtures/round.epub"]
        self.assertEqual([path for path, _ in working_notes(notes + ordinary)], sorted(notes))


class InputsTest(unittest.TestCase):
    def test_every_input_is_listed(self):
        extra = unlisted_inputs(repo_files())
        self.assertEqual(extra, [], f"Unlisted files: {', '.join(extra)}. A file under docs/ or artifacts/ that "
                                    "is not Markdown or an image directly in docs/ is an input code reads: give it "
                                    f"a reviewed entry in tests/repo_files_test.py INPUTS. {HINT}")

    def test_every_listed_input_exists(self):
        # A stale entry would let a removed input come back without review.
        missing = sorted(path for path in INPUTS if not (ROOT / path).is_file())
        self.assertEqual(missing, [], "Listed but missing; remove the entry")

    def test_negative_control_catches_an_unlisted_input(self):
        # Ordinary docs pass; a nested file, other kinds in docs/ and any artifact need an entry.
        paths = ["docs/CI.md", "docs/new-guide.md", "docs/photo.png", "docs/NOTICES.txt", "docs/plans/next.md",
                 "docs/new-lock.json", "artifacts/bench.json", "tools/notes.md"]
        self.assertEqual(unlisted_inputs(paths), ["artifacts/bench.json", "docs/new-lock.json", "docs/plans/next.md"])


if __name__ == "__main__":
    unittest.main()
