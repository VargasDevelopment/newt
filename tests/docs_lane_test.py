"""The docs lane (tools/docs_lane.py): only a pull request whose every changed
path is in the lane skips the gate's parts, anything else or anything unclear
takes the full gate; its link check; and DOCS_CHECKS, kept complete against
every source that names a lane path."""
import ast
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import docs_lane


AT = '@'  # a literal address would look like a leak to tools/clean_export.py


def git(root, *arguments):
    identity = ('-c', 'user.name=t', '-c', f'user.email=t{AT}t.invalid', '-c', 'commit.gpgsign=false')
    return subprocess.run(['git', *identity, *arguments], cwd=root, check=True, capture_output=True,
                          text=True).stdout.strip()


class Repository:
    """A scratch repository with a main commit, for pull requests against it."""
    def __init__(self, files):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)
        git(self.root, 'init', '-q')
        self.base = self.commit(files)

    def commit(self, files=(), deleted=(), links=()):
        for name, text in dict(files).items():
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_text(text)
        for name, target in dict(links).items():
            (self.root / name).symlink_to(target)
        for name in deleted:
            (self.root / name).unlink()
        git(self.root, 'add', '-A')
        git(self.root, 'commit', '-q', '--allow-empty', '-m', 'change')
        return git(self.root, 'rev-parse', 'HEAD')

    def lane(self, head):
        return docs_lane.lane(self.root, 'pull_request', self.base, head)[0]


MAIN = {'README.md': 'See [install](docs/INSTALL.md).\n', 'docs/INSTALL.md': 'Install.\n',
        'docs/CI.md': '![photo](photo.jpg)\n', 'docs/photo.jpg': 'jpg', 'CHANGELOG.md': '# Changelog\n',
        'docs/toolchain-linux-lock.json': '{}\n', 'tools/run.py': 'print()\n', 'CONTEXT.md': 'Words.\n'}


class Lane(unittest.TestCase):
    def setUp(self):
        self.repo = Repository(MAIN)
        self.addCleanup(self.repo.scratch.cleanup)

    def test_only_docs_changes_take_the_docs_lane(self):
        docs = {'README.md': 'New.\n', 'docs/CI.md': 'New.\n', 'docs/new.png': 'png'}
        self.assertEqual(self.repo.lane(self.repo.commit(docs)), 'docs')
        # A deletion and a rename within the lane stay in it.
        git(self.repo.root, 'mv', 'docs/INSTALL.md', 'docs/SETUP.md')
        self.assertEqual(self.repo.lane(self.repo.commit(deleted=['docs/photo.jpg'])), 'docs')

    def test_any_path_outside_the_lane_takes_the_full_gate(self):
        """Negative control: each of these beside a docs change gets the full gate."""
        cases = [{'tools/run.py': 'print(1)\n'}, {'CHANGELOG.md': '# Changelog\n\nNew.\n'},
                 {'docs/toolchain-linux-lock.json': '{"x": 1}\n'}, {'docs/toolchain-proposal.md': 'New.\n'},
                 {'docs/plans/next.md': 'New.\n'}, {'CONTEXT.md': 'New.\n'}, {'docs/notes.txt': 'New.\n'}]
        for extra in cases:
            with self.subTest(extra=next(iter(extra))):
                git(self.repo.root, 'checkout', '-q', '--detach', self.repo.base)
                self.assertEqual(self.repo.lane(self.repo.commit({'docs/CI.md': 'New.\n', **extra})), 'full')
        for change in ({'deleted': ['tools/run.py']}, {'links': {'docs/link.md': '../tools/run.py'}}):
            with self.subTest(change=change):
                git(self.repo.root, 'checkout', '-q', '--detach', self.repo.base)
                self.assertEqual(self.repo.lane(self.repo.commit({'README.md': 'New.\n'}, **change)), 'full')
        # A rename out of the lane leaves through its addition.
        git(self.repo.root, 'checkout', '-q', '--detach', self.repo.base)
        git(self.repo.root, 'mv', 'docs/INSTALL.md', 'tools/INSTALL.md')
        self.assertEqual(self.repo.lane(self.repo.commit()), 'full')

    def test_anything_unclear_takes_the_full_gate(self):
        head = self.repo.commit({'README.md': 'New.\n'})
        self.assertEqual(self.repo.lane(head), 'docs')
        for event in ('push', 'schedule', 'workflow_dispatch'):
            self.assertEqual(docs_lane.lane(self.repo.root, event, self.repo.base, head)[0], 'full')
        for base, tip in (('', head), (self.repo.base, ''), ('HEAD~1', head), ('--output=x', head), ('f' * 40, head)):
            self.assertEqual(docs_lane.lane(self.repo.root, 'pull_request', base, tip)[0], 'full', base)
        self.assertFalse(any(path.name.startswith('x...') for path in self.repo.root.iterdir()))  # no git option
        self.assertEqual(self.repo.lane(self.repo.commit()), 'docs')  # an empty commit on top changes nothing
        self.assertEqual(self.repo.lane(self.repo.base), 'full')  # a pull request that changes nothing


class Links(unittest.TestCase):
    def setUp(self):
        self.repo = Repository(MAIN)
        self.addCleanup(self.repo.scratch.cleanup)

    def broken(self, touched):
        return docs_lane.broken_links(self.repo.root, set(touched))

    def test_links_of_a_touched_doc_resolve(self):
        self.assertEqual(self.broken(['README.md', 'docs/CI.md']), [])
        self.repo.commit({'docs/CI.md': '[a](INSTALL.md#steps) [b](../README.md) ![c](photo.jpg?raw=1) '
                          '[d](https://example.com/x.md) [e](#tiers) [f](/tools/) <img src="photo.jpg">\n'
                          '`[g](missing.md)`\n```\n[h](missing.md)\n```\n[i]: INSTALL.md\n'})
        self.assertEqual(self.broken(['docs/CI.md']), [])
        self.repo.commit({'docs/CI.md': '[a](INSTALL.md) [b](gone.md#x) <img src="gone.png">\n[c]: ../gone/\n'})
        self.assertEqual(self.broken(['docs/CI.md']), ['docs/CI.md: gone.md', 'docs/CI.md: ../gone/',
                                                         'docs/CI.md: gone.png'])
        self.assertEqual(self.broken(['README.md']), [])  # an untouched doc's links are not this change's

    def test_a_removed_or_renamed_doc_breaks_the_links_to_it(self):
        git(self.repo.root, 'mv', 'docs/INSTALL.md', 'docs/SETUP.md')
        self.repo.commit()
        self.assertEqual(self.broken(['docs/INSTALL.md', 'docs/SETUP.md']), ['README.md: docs/INSTALL.md'])
        self.assertEqual(self.broken(['docs/SETUP.md']), [])  # control: only the touched path is checked


class DocsChecks(unittest.TestCase):
    def test_every_source_that_names_a_lane_path_is_a_docs_check_or_a_reviewed_non_reader(self):
        """A new test or tool that reads README.md or docs/ text fails here until
        DOCS_CHECKS runs it, or NOT_DOCS_CHECKS says why it is no docs check."""
        # Untracked sources count too, so a new test fails before it is committed.
        sources = git(ROOT, 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*.py').split('\0')
        readers = {path for path in sources if path and (ROOT / path).is_file()
                   and names_lane_path((ROOT / path).read_text())}
        checks = {script for script, *_ in docs_lane.DOCS_CHECKS}
        self.assertEqual(readers, checks | set(docs_lane.NOT_DOCS_CHECKS))
        self.assertFalse(checks & set(docs_lane.NOT_DOCS_CHECKS))

    def test_the_scan_sees_a_read_but_not_a_mention(self):
        self.assertTrue(names_lane_path("text = (ROOT / 'docs' / name).read_text()\n"))
        self.assertTrue(names_lane_path("text = Path(f'docs/{name}')\n"))
        self.assertTrue(names_lane_path("text = (ROOT / 'README.md').read_text()\n"))
        self.assertFalse(names_lane_path('"""See docs/CI.md."""\n# and README.md\n'))
        self.assertFalse(names_lane_path("lock = 'docs/toolchain-lock.json'\nnotes = 'docs/NOTICES.txt'\n"))


def names_lane_path(source):
    """Whether Python source has a string, other than a docstring, that names a
    lane path or the docs folder; comments and docstrings only mention them."""
    tree = ast.parse(source)
    docstrings = {id(node.body[0].value) for node in ast.walk(tree)
                  if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))
                  and node.body and isinstance(node.body[0], ast.Expr) and isinstance(node.body[0].value, ast.Constant)}
    return any(isinstance(node, ast.Constant) and isinstance(node.value, str) and id(node) not in docstrings
               and (node.value in ('docs', 'docs/') or docs_lane.in_lane(node.value)) for node in ast.walk(tree))


if __name__ == '__main__':
    unittest.main()
