"""The historic-source fixtures are exact, complete and fail loudly when damaged.

Every repository checks the manifest against the stored bytes. A repository
that still has the private history (this one, until launch) also checks every
fixture against `git cat-file <commit>:<path>`; a fresh export has none of those
commits, and a history with only some of them fails.
"""
from contextlib import contextmanager
from hashlib import sha256
from pathlib import Path
import ast
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests"))
import history_fixtures


def rows() -> list[list[str]]:
    return [line.split("\t") for line in history_fixtures.MANIFEST.read_text().splitlines()
            if not line.startswith("#")]


def imports_helper(source: str) -> bool:
    """Any import form of the helper: plain, aliased or `from ... import`."""
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Import) and any(alias.name == "history_fixtures" for alias in node.names):
            return True
        if isinstance(node, ast.ImportFrom) and node.level == 0 and node.module == "history_fixtures":
            return True
    return False


@contextmanager
def fixture_copy():
    """A scratch copy of the whole fixture root that the helper reads instead."""
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "history"
        shutil.copytree(history_fixtures.FIXTURES, root)
        history_fixtures.manifest.cache_clear()
        try:
            with patch.multiple(history_fixtures, FIXTURES=root, MANIFEST=root / "manifest.tsv", BLOBS=root / "blobs"):
                yield root
        finally:
            history_fixtures.manifest.cache_clear()


class HistoryFixturesTest(unittest.TestCase):
    def test_manifest_and_blobs_match_exactly(self):
        digests = set()
        for row in rows():
            self.assertEqual(len(row), 4, row)
            commit, path, digest, tests = row
            self.assertRegex(commit, r"^[0-9a-f]{40}$")
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
            self.assertTrue(tests and all((ROOT / "tests" / test).is_file() for test in tests.split(",")), row)
            digests.add(digest)
        stored = {path.name for path in history_fixtures.BLOBS.iterdir()}
        self.assertEqual(set(stored), digests, "every blob is used and every used blob is stored")
        for name in stored:
            self.assertEqual(sha256(history_fixtures.blob(name).read_bytes()).hexdigest(), name)
        self.assertEqual(sorted(p.name for p in history_fixtures.FIXTURES.iterdir()), ["blobs", "manifest.tsv"])
        self.assertEqual(len(history_fixtures.manifest()), len(rows()))

    def test_each_reader_is_named_in_the_manifest(self):
        named = {test for row in rows() for test in row[3].split(",")}
        readers = {path.name for path in (ROOT / "tests").glob("*.py")
                   if imports_helper(path.read_text())} - {Path(__file__).name}
        self.assertEqual(named, readers)

    def test_the_reader_scan_sees_every_import_form(self):
        for source in ("import history_fixtures\n", "import os, history_fixtures as fixtures\n",
                       "def f():\n    from history_fixtures import text\n"):
            self.assertTrue(imports_helper(source), source)
        self.assertFalse(imports_helper("name = 'history_fixtures'\nimport history_fixtures_test\n"))

    def test_a_damaged_or_missing_fixture_fails_loudly(self):
        commit, path, digest, _tests = rows()[0]
        with fixture_copy() as root:
            self.assertEqual(sha256(history_fixtures.read(commit[:7], path)).hexdigest(), digest)
            stored = root / "blobs" / digest
            stored.write_bytes(stored.read_bytes() + b"\n")
            with self.assertRaisesRegex(history_fixtures.HistoryFixtureError, "no longer matches its sha256"):
                history_fixtures.read(commit[:7], path)
        with self.assertRaisesRegex(history_fixtures.HistoryFixtureError, "no fixture for"):
            history_fixtures.read(commit, path + ".missing")
        with self.assertRaisesRegex(history_fixtures.HistoryFixtureError, "names 0 fixture commits"):
            history_fixtures.read("0" * 40, path)

    def test_an_edited_manifest_fails_every_read(self):
        lines = history_fixtures.MANIFEST.read_text().splitlines(keepends=True)
        dropped = next(index for index, line in enumerate(lines)
                       if line.startswith("fe4c4fa") and "\tplatformio.ini\t" in line)
        commit, path, digest, tests = lines[dropped].split("\t")
        other = next(row[2] for row in rows() if row[2] != digest)
        edits = {"a deleted row": lines[:dropped] + lines[dropped + 1:],
                 "a row repointed to another valid blob": [*lines[:dropped], "\t".join((commit, path, other, tests)),
                                                          *lines[dropped + 1:]]}
        for label, edited in edits.items():
            with self.subTest(label), fixture_copy() as root:
                (root / "manifest.tsv").write_text("".join(edited))
                with self.assertRaisesRegex(history_fixtures.HistoryFixtureError, "not the reviewed manifest"):
                    history_fixtures.read("fe4c4fa", "platformio.ini")

    def test_a_linked_blob_fails_even_with_the_right_bytes(self):
        commit, path, digest, _tests = rows()[0]
        with fixture_copy() as root:
            outside = root.parent / digest
            stored = root / "blobs" / digest
            stored.rename(outside)
            stored.symlink_to(outside)
            with self.assertRaisesRegex(history_fixtures.HistoryFixtureError, "is a link or leaves the fixture root"):
                history_fixtures.read(commit, path)

    def test_fixtures_equal_the_private_history_where_it_exists(self):
        commits = sorted({row[0] for row in rows()})
        present = [commit for commit in commits if subprocess.run(
            ["git", "cat-file", "-e", commit + "^{commit}"], cwd=ROOT, capture_output=True).returncode == 0]
        if not present:
            print("history fixtures: no source commit in this repository; manifest checked alone")
            return
        self.assertEqual(present, commits, "a history that has some fixture commits must have all of them")
        objects = [f"{commit}:{path}" for commit, path, _digest, _tests in rows()]
        batch = subprocess.run(["git", "cat-file", "--batch"], cwd=ROOT, check=True, capture_output=True,
                               input="".join(name + "\n" for name in objects).encode()).stdout
        for name, (_commit, _path, digest, _tests) in zip(objects, rows()):
            header, _, batch = batch.partition(b"\n")
            kind, size = header.split()[1:3] if header.count(b" ") == 2 else (b"missing", b"0")
            self.assertEqual(kind, b"blob", name)
            data, batch = batch[:int(size)], batch[int(size) + 1:]
            self.assertEqual(sha256(data).hexdigest(), digest, name)
        print(f"history fixtures: {len(objects)} files equal {len(commits)} commits' history")


if __name__ == "__main__":
    unittest.main()
