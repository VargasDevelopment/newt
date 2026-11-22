"""Authored standalone provenance failures; no private books or hardware."""
from contextlib import ExitStack
import copy
import hashlib
import io
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tarfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import trusted_host as trust
import freeink_source_pin
import host_common


class SourcePinTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.sdk = Path(self.temp.name) / 'sdk'
        self.sdk.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'Authored fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        (self.sdk / 'input.h').write_text('original authored source')
        (self.sdk / '.gitignore').write_text('ignored.h\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'Authored pin')
        self.git('remote', 'add', 'origin', trust.SDK_ORIGIN)
        self.pin = self.git('rev-parse', 'HEAD')
        self.pins = [patch.object(module, 'SDK_PIN', self.pin) for module in (trust, freeink_source_pin)]
        for pinned in self.pins:
            pinned.start()
            self.addCleanup(pinned.stop)

    def git(self, *args):
        return subprocess.run(['git', '-C', str(self.sdk), *args], check=True,
                              capture_output=True, text=True).stdout.strip()

    def test_exact_clean_pin_accepted(self):
        raw = freeink_source_pin.source_git(
            self.sdk, 'config', '--local', '--no-includes', '--get-all', 'remote.origin.url')
        effective = freeink_source_pin.source_git(
            self.sdk, 'remote', 'get-url', '--all', 'origin')
        self.assertEqual(raw, trust.SDK_ORIGIN.encode() + b'\n')
        self.assertEqual(effective, trust.SDK_ORIGIN.encode() + b'\n')
        self.assertEqual(trust.verify_source_pin(self.sdk), self.pin)

    def test_origin_control_characters_are_not_normalized(self):
        for suffix in ('\r', '\n', '\v', '\f', '\x85', '\u2028', '\u2029', ' '):
            with self.subTest(suffix=repr(suffix)):
                changed = trust.SDK_ORIGIN + suffix
                self.git('remote', 'set-url', 'origin', changed)
                raw = freeink_source_pin.source_git(
                    self.sdk, 'config', '--local', '--no-includes', '--get-all', 'remote.origin.url')
                self.assertEqual(raw, changed.encode() + b'\n')
                with self.assertRaisesRegex(RuntimeError, 'raw local origin'):
                    trust.verify_source_pin(self.sdk)
                if suffix == '\r':
                    # Negative control: text decoding and splitlines accepted this URL.
                    for arguments in (('config', '--local', '--no-includes', '--get-all',
                                       'remote.origin.url'), ('remote', 'get-url', '--all', 'origin')):
                        former = freeink_source_pin.source_git(self.sdk, *arguments, text=True)
                        self.assertEqual(former.splitlines(), [trust.SDK_ORIGIN])
                self.git('remote', 'set-url', 'origin', trust.SDK_ORIGIN)
                if suffix == '\n':
                    continue  # Git rejects a newline in a rewrite subsection name.
                rewrite = 'url.' + changed + '.insteadOf'
                self.git('config', rewrite, trust.SDK_ORIGIN)
                effective = freeink_source_pin.source_git(
                    self.sdk, 'remote', 'get-url', '--all', 'origin')
                self.assertEqual(effective, changed.encode() + b'\n')
                with self.assertRaisesRegex(RuntimeError, 'effective origin'):
                    trust.verify_source_pin(self.sdk)
                self.git('config', '--unset-all', rewrite)

    def test_alias_rewritten_to_official_is_not_a_canonical_raw_origin(self):
        alias = 'freeink-alias:'
        self.git('remote', 'set-url', 'origin', alias)
        self.git('config', 'url.' + trust.SDK_ORIGIN + '.insteadOf', alias)
        self.assertEqual(self.git('remote', 'get-url', '--all', 'origin'), trust.SDK_ORIGIN)
        with self.assertRaisesRegex(RuntimeError, 'raw local origin'):
            trust.verify_source_pin(self.sdk)

    def test_duplicate_raw_origins_are_rejected(self):
        self.git('config', '--add', 'remote.origin.url', trust.SDK_ORIGIN)
        raw = self.git('config', '--local', '--no-includes', '--get-all', 'remote.origin.url')
        self.assertEqual(raw.splitlines(), [trust.SDK_ORIGIN, trust.SDK_ORIGIN])
        with self.assertRaisesRegex(RuntimeError, 'raw local origin'):
            trust.verify_source_pin(self.sdk)

    def test_canonical_raw_origin_rewritten_to_nonofficial_transport_is_rejected(self):
        attacker = 'https://example.invalid/replaced-sdk'
        self.git('config', 'url.' + attacker + '.insteadOf', trust.SDK_ORIGIN)
        raw = self.git('config', '--local', '--no-includes', '--get-all', 'remote.origin.url')
        self.assertEqual(raw, trust.SDK_ORIGIN)
        self.assertEqual(self.git('remote', 'get-url', '--all', 'origin'), attacker)
        with self.assertRaisesRegex(RuntimeError, 'effective origin'):
            trust.verify_source_pin(self.sdk)

    def test_other_commit_or_origin_rejected(self):
        self.git('remote', 'set-url', 'origin', 'https://example.invalid/replaced-sdk')
        with self.assertRaises(RuntimeError): trust.verify_source_pin(self.sdk)
        self.git('remote', 'set-url', 'origin', trust.SDK_ORIGIN)
        self.git('commit', '--allow-empty', '-qm', 'Wrong pin')
        with self.assertRaises(RuntimeError): trust.verify_source_pin(self.sdk)

    def test_modified_untracked_and_ignored_source_rejected(self):
        for name in ('input.h', 'injected.h', 'ignored.h'):
            path = self.sdk / name
            before = path.read_bytes() if path.exists() else None
            path.write_text('unexpected authored replacement')
            with self.assertRaises(RuntimeError): trust.verify_source_pin(self.sdk)
            if before is None: path.unlink()
            else: path.write_bytes(before)
        self.assertEqual(trust.verify_source_pin(self.sdk), self.pin)

    def test_index_flags_cannot_hide_modified_pinned_bytes(self):
        for flag in ('assume-unchanged', 'skip-worktree'):
            self.git('update-index', '--' + flag, 'input.h')
            (self.sdk / 'input.h').write_text('hidden replacement source')
            self.assertEqual(self.git('status', '--porcelain'), '')
            with self.assertRaises(RuntimeError): trust.verify_source_pin(self.sdk)
            (self.sdk / 'input.h').write_text('original authored source')
            self.git('update-index', '--no-' + flag, 'input.h')
        self.assertEqual(trust.verify_source_pin(self.sdk), self.pin)

    def test_filemode_disabled_cannot_hide_executable_mode_drift(self):
        path = self.sdk / 'input.h'
        self.git('config', 'core.fileMode', 'false')
        for mask in (0o000, 0o001, 0o010, 0o011, 0o100, 0o101, 0o110, 0o111):
            with self.subTest(mask=oct(mask)):
                path.chmod(0o644 | mask)
                self.assertEqual(self.git('status', '--porcelain'), '')
                if mask & 0o100:
                    with self.assertRaisesRegex(RuntimeError, 'executable mode'):
                        trust.verify_source_pin(self.sdk)
                else:
                    self.assertEqual(trust.verify_source_pin(self.sdk), self.pin)
        path.chmod(0o644)
        self.assertEqual(trust.verify_source_pin(self.sdk), self.pin)

    def test_replace_ref_cannot_redirect_pinned_tree_or_archive(self):
        pinned, path = self.pin, self.sdk / 'input.h'
        path.write_text('attacker replacement source')
        self.git('add', 'input.h')
        self.git('commit', '-qm', 'Attacker replacement')
        attacker = self.git('rev-parse', 'HEAD')
        self.git('replace', pinned, attacker)
        self.git('reset', '--hard', '-q', pinned)
        self.assertEqual(self.git('rev-parse', 'HEAD'), pinned)
        self.assertEqual(path.read_text(), 'attacker replacement source')
        self.assertEqual(self.git('status', '--porcelain'), '')

        unsafe = {key: value for key, value in freeink_source_pin.git_environment().items()
                  if key != 'GIT_NO_REPLACE_OBJECTS'}
        archive = subprocess.run(['git', '-C', str(self.sdk), 'archive', '--format=tar', 'HEAD'],
                                 check=True, capture_output=True, env=unsafe).stdout
        with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
            self.assertEqual(contents.extractfile('input.h').read(), b'attacker replacement source')
        safe_archive = freeink_source_pin.source_git(self.sdk, 'archive', '--format=tar', 'HEAD')
        with tarfile.open(fileobj=io.BytesIO(safe_archive)) as contents:
            self.assertEqual(contents.extractfile('input.h').read(), b'original authored source')

        with patch.object(freeink_source_pin, 'git_environment', return_value=unsafe), \
                patch.object(freeink_source_pin, 'reject_replace_refs'):
            self.assertEqual(freeink_source_pin.verify_source_pin(self.sdk), pinned)
        with self.assertRaisesRegex(RuntimeError, 'replacement object refs'):
            trust.verify_source_pin(self.sdk)

    def test_symlink_checkout_rejected(self):
        link = self.sdk.parent / 'alias'
        link.symlink_to(self.sdk, target_is_directory=True)
        with self.assertRaises(RuntimeError): trust.verify_source_pin(link)


if __name__ == '__main__':
    unittest.main()
