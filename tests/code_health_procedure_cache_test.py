"""Executable procedure bytecode must be bound without owning unrelated caches."""
import importlib.util
import marshal
import os
from pathlib import Path
import py_compile
import subprocess
import sys
import unittest
from unittest.mock import patch

import code_health_resources_test as resource_fixture
from resource_fixture import BUILD


class ProcedureCacheTests(unittest.TestCase):
    def setUp(self):
        # Reuse the resource fixture, not its test methods or test discovery.
        self.fixture = resource_fixture.ResourceTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        self.root = self.fixture.root

    def remember(self):
        self.fixture.report = self.fixture.build()
        self.fixture.save()
        self.assertEqual(self.fixture.check(), [])

    def test_timestamp_valid_bytecode_change_stales_receipt_with_source_unchanged(self):
        for name in ("tools/newt_package.py", "assets/frosted-home/source/png_tools.py"):
            with self.subTest(helper=name):
                source = self.root / name
                source.write_text("MARKER = 'source'\n")
                cache = Path(importlib.util.cache_from_source(str(source)))
                py_compile.compile(str(source), cfile=str(cache), doraise=True)
                original = cache.read_bytes()
                source_bytes = source.read_bytes()
                self.remember()
                altered = compile("MARKER = 'cache'\n", str(source), "exec")
                cache.write_bytes(original[:16] + marshal.dumps(altered))
                self.assertEqual(source.read_bytes(), source_bytes)
                identity = resource_fixture.resources.build_identity(self.root, BUILD)
                self.assertEqual(identity["inputs_sha256"], self.fixture.report["identity"]["inputs_sha256"])
                # Python -B executes a valid existing cache despite disabling writes.
                code = f"import sys; sys.path.insert(0, {str(source.parent)!r}); import {source.stem}; print({source.stem}.MARKER)"
                process = subprocess.Popen([sys.executable, "-B", "-c", code], stdout=subprocess.PIPE,
                                           stderr=subprocess.PIPE, text=True, cwd=self.root)
                output, error = process.communicate(timeout=10)
                self.assertEqual((process.returncode, output, error), (0, "cache\n", ""))
                with self.assertRaisesRegex(ValueError, "Stale"):
                    self.fixture.check()
                cache.write_bytes(original)
                self.assertEqual(self.fixture.check(), [])

    def test_each_named_procedure_tagged_optimized_and_legacy_cache_is_bound(self):
        sources = [self.root / "tools" / name for name in self.fixture.procedure_names]
        sources.append(self.root / "assets/frosted-home/source/png_tools.py")
        for source in sources:
            for cache in (source.with_suffix(".pyc"),
                          source.parent / "__pycache__" / (source.stem + ".cpython-314.pyc"),
                          source.parent / "__pycache__" / (source.stem + ".cpython-314.opt-1.pyc")):
                with self.subTest(cache=cache.relative_to(self.root)):
                    cache.parent.mkdir(exist_ok=True)
                    cache.write_bytes(b"existing executable bytecode")
                    self.remember()
                    cache.write_bytes(b"changed bytecode; source and compiler outputs unchanged")
                    with self.assertRaisesRegex(ValueError, "Stale"):
                        self.fixture.check()
                    cache.unlink()
        self.remember()

    def test_added_relevant_cache_stales_but_unrelated_caches_do_not(self):
        for parent in (self.root / "tools", self.root / "assets/frosted-home/source"):
            directory = parent / "__pycache__"
            directory.mkdir()
            (directory / "unrelated.cpython-314.pyc").write_bytes(b"outside execution closure")
            self.assertEqual(self.fixture.check(), [])
            stem = "newt_package" if parent.name == "tools" else "png_tools"
            relevant = directory / (stem + ".cpython-314.pyc")
            relevant.write_bytes(b"new executable helper cache")
            with self.assertRaisesRegex(ValueError, "Stale"):
                self.fixture.check()
            relevant.unlink()
        self.assertEqual(self.fixture.check(), [])

    def test_sdk_helper_cache_is_bound_in_the_build_identity(self):
        resources = resource_fixture.resources
        config = resources.configuration(self.root, BUILD)
        cache = self.root / "tools/__pycache__/apply-freeink-patch.cpython-314.pyc"
        cache.parent.mkdir()
        cache.write_bytes(b"original SDK helper cache")
        self.remember()
        before = resources.build_identity(self.root, BUILD)
        cache.write_bytes(b"changed cache; helper source unchanged")
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.fixture.check()
        with self.assertRaisesRegex(ValueError, "during compilation"):
            resources.build_report(self.root, BUILD, config, 0, before)

    def test_nondefault_cache_prefix_cannot_issue_or_validate_identity(self):
        resources = resource_fixture.resources
        with patch.object(resources.sys, "pycache_prefix", str(self.root / "alternate-cache")):
            with self.assertRaisesRegex(ValueError, "cache prefix"):
                self.fixture.check()
            with self.assertRaisesRegex(ValueError, "cache prefix"):
                resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0,
                                       self.fixture.report["identity"])
        self.assertEqual(self.fixture.check(), [])

    def test_generator_children_ignore_ambient_cache_prefix(self):
        # Python resolves a script's directory before importing local helpers.
        source = (self.root / "tools/newt_package.py").resolve()
        source.write_text("MARKER = 'source'\n")
        prefix = self.root / "external-cache"
        with patch.object(sys, "pycache_prefix", str(prefix)):
            cache = Path(importlib.util.cache_from_source(str(source)))
        py_compile.compile(str(source), cfile=str(cache), doraise=True)
        original = cache.read_bytes()
        altered = original[:16] + marshal.dumps(compile("MARKER = 'cache'\n", str(source), "exec"))
        cache.write_bytes(altered)
        children = ("firmware-build.py", "make-builtin-app.py", "make-reader-fixture.py",
                    "make-stage-backdrop.py", "pack-reader-atlas.py")
        for name in children:
            (self.root / "tools" / name).write_text("import newt_package\nprint(newt_package.MARKER)\n")
        current = Path(__file__).resolve().parents[1] / "tools/code_health.py"
        old = self.root / "old-code-health.py"
        old.write_text(current.read_text().replace('"-E", ', ""))
        # Both versions reset the parent's prefix. The old control removes only
        # child environment isolation from the actual launcher function.
        probe = ("import sys; from pathlib import Path\n"
                 "namespace = {'__name__': 'probe', '__file__': sys.argv[1]}\n"
                 "exec(compile(Path(sys.argv[1]).read_text(), sys.argv[1], 'exec'), namespace)\n"
                 "namespace['verify_existing_inputs'](Path(sys.argv[2]))\n")
        environment = dict(os.environ, PYTHONPYCACHEPREFIX=str(prefix))
        for launcher, marker in ((current, "source"), (old, "cache")):
            with self.subTest(launcher=launcher.name):
                process = subprocess.Popen([sys.executable, "-B", "-c", probe, str(launcher), str(self.root)],
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                           cwd=self.root, env=environment)
                output, error = process.communicate(timeout=10)
                self.assertEqual((process.returncode, output.splitlines(), error),
                                 (0, [marker] * len(children), ""))
                self.assertFalse((source.parent / "__pycache__").exists())
                self.assertEqual(source.read_text(), "MARKER = 'source'\n")
                self.assertEqual([path for path in prefix.rglob("*") if path.is_file()], [cache])
                self.assertEqual(cache.read_bytes(), altered)

    def test_relevant_cache_symlinks_and_directory_aliases_fail_closed(self):
        for source in (self.root / "tools/newt_package.py", self.root / "assets/frosted-home/source/png_tools.py"):
            directory = source.parent / "__pycache__"
            target = source.parent / "owned-cache-target"
            target.mkdir()
            (target / "implementation.pyc").write_bytes(b"owned target")
            self.remember()
            directory.symlink_to(target, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "cache directory"):
                self.fixture.check()
            directory.unlink()
            directory.mkdir()
            for cache in (source.with_suffix(".pyc"), directory / (source.stem + ".cpython-314.pyc")):
                for destination in (target, target / "implementation.pyc", target / "missing.pyc", self.root.parent):
                    with self.subTest(cache=cache, target=destination):
                        cache.symlink_to(destination, target_is_directory=destination.is_dir())
                        with self.assertRaisesRegex(ValueError, "regular owned file|symlink"):
                            self.fixture.check()
                        cache.unlink()
                cache.mkdir()
                with self.assertRaisesRegex(ValueError, "regular owned file"):
                    self.fixture.check()
                cache.rmdir()
        self.assertEqual(self.fixture.check(), [])


if __name__ == "__main__":
    unittest.main()
