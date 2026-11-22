"""Reject fixed host scratch roots and mandatory TMPDIR lookups in gate sources."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SOURCES = {'.py', '.cpp', '.h', '.hpp', '.c', '.cc', '.cxx', '.m', '.mm', '.sh'}
FIXED = re.compile(r'(?:(?<![\w.-])|(?<=-I)|(?<=-L))/(?:private/)?(?:var/)?tmp(?=/|[^A-Za-z0-9_-]|$)')
LOOKUP = re.compile(r'''os\.environ\s*\[\s*['"]TMPDIR['"]\s*\]''')
# Each exception permits one source line, identified by its narrow prefix.
# These are the canonical allocator, persistent coordination, or inert text;
# none grants a file-wide exclusion or permits a fixed fixture directory.
EXCEPTIONS = (
    ('tools/gate_scratch.py', 'BASE = ', 'The single private per-run gate allocator.'),
    ('tools/host_jobs.py', 'return Path(os.environ.get(GLOBAL_ROOT,', 'Persistent per-uid flock coordination, not scratch.'),
    ('tests/host_wide_jobs_test.py', 'self.assertEqual(host_jobs.global_root(),', 'Checks the persistent pool path without opening it.'),
    ('tests/gate_scratch_test.py', 'BAD_SCRATCH = ', 'Inert rejection fixtures; never opened.'),
    ('tests/gate_scratch_test.py', 'BAD_LOOKUP = ', 'Inert rejection fixture for an unset lookup.'),
)


def violations(root=ROOT):
    found, used = [], set()
    for folder in ('tests', 'tools'):
        for path in sorted((root / folder).rglob('*')):
            if path.suffix not in SOURCES or not path.is_file():
                continue
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                found.append(f'{relative}: source symlink cannot be audited')
                continue
            text = path.read_text()
            lookups = {text.count('\n', 0, match.start()) + 1 for match in LOOKUP.finditer(text)}
            for number, line in enumerate(text.splitlines(), 1):
                if not (FIXED.search(line) or number in lookups):
                    continue
                allowed = next((index for index, (name, prefix, _) in enumerate(EXCEPTIONS)
                                if name == relative and line.strip().startswith(prefix) and index not in used), None)
                if allowed is None:
                    found.append(f'{relative}:{number}: use default temporary-directory APIs, not fixed roots or required TMPDIR')
                else:
                    used.add(allowed)
    found.extend(f'{name}: unused scratch exception: {prefix}'
                 for index, (name, prefix, _) in enumerate(EXCEPTIONS) if index not in used)
    return found


def check(root=ROOT):
    errors = violations(root)
    if errors:
        raise RuntimeError('Scratch policy failed:\n' + '\n'.join(errors))
    print('PASS scratch policy: fixtures inherit the gate-owned temporary directory', flush=True)


if __name__ == '__main__':
    check()
