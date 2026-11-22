"""Interpreter and reused stdlib changes invalidate otherwise unchanged receipts."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import code_health_resources_test as resource_fixture
from resource_fixture import BUILD
import python_runtime
import history_fixtures

REAL_RUN = subprocess.run


class PythonRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.real_description = python_runtime.runtime_description
        self.fixture = resource_fixture.ResourceTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        self.resources = resource_fixture.resources

    def assert_stale(self):
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.fixture.check()
        with self.assertRaisesRegex(ValueError, "during compilation"):
            self.resources.build_report(self.fixture.root, BUILD, self.resources.configuration(self.fixture.root, BUILD),
                                        0, self.fixture.report["identity"])

    def test_gate_refuses_an_optimized_interpreter(self):
        # An invalid profile keeps a missing guard from starting a nested gate.
        gate = Path(__file__).resolve().parents[1] / "tools/check.py"
        for options, environment in ((["-O"], {}), ([], {"PYTHONOPTIMIZE": "1"})):
            with self.subTest(options=options, environment=environment):
                result = REAL_RUN([sys.executable, *options, "-B", str(gate), "--profile", "none"],
                                  capture_output=True, text=True, timeout=60, env={**os.environ, **environment})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("without -O or PYTHONOPTIMIZE", result.stderr)

    def test_gate_pins_sanitizer_options(self):
        # Runs the gate's module setup only: the invalid profile stops it in argparse.
        gate = Path(__file__).resolve().parents[1] / "tools/check.py"
        code = (f"import os, runpy, sys\nsys.argv = [{str(gate)!r}, '--profile', 'none']\n"
                f"sys.path.insert(0, {str(gate.parent)!r})\n"
                "try:\n    runpy.run_path(sys.argv[0], run_name='__main__')\nexcept SystemExit:\n    pass\n"
                "print(os.environ['ASAN_OPTIONS'], os.environ['UBSAN_OPTIONS'])\n")
        result = REAL_RUN([sys.executable, "-B", "-c", code], capture_output=True, text=True, timeout=60,
                          env={**os.environ, "ASAN_OPTIONS": "exitcode=0", "UBSAN_OPTIONS": "halt_on_error=0"})
        self.assertEqual(result.stdout.splitlines()[-1].split(), ["halt_on_error=1:abort_on_error=1", "halt_on_error=1"])

    def test_gate_hands_off_to_pinned_python_before_its_imports(self):
        # A copy of check.py with no sibling modules: anything past the handoff
        # would fail to import, so each case shows the handoff ran first.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            gate = root / "tools/check.py"
            gate.parent.mkdir()
            gate.write_text((Path(__file__).resolve().parents[1] / "tools/check.py").read_text())
            pinned = root / ".toolchain/platformio/penv/bin/python"

            def run(*, guard=None):
                env = {k: v for k, v in os.environ.items() if k != "NEWT_CHECK_PYTHON_HANDOFF"}
                if guard:
                    env["NEWT_CHECK_PYTHON_HANDOFF"] = guard
                return REAL_RUN([sys.executable, "-B", str(gate), "--profile", "linux", "-j", "2"],
                                capture_output=True, text=True, timeout=60, env=env)

            missing = run()
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn(f"pinned Python, {pinned}, which is missing", missing.stderr)

            pinned.parent.mkdir(parents=True)
            pinned.write_text("#!/bin/sh\nprintf '%s\\n' \"$NEWT_CHECK_PYTHON_HANDOFF\" \"$@\"\n")
            pinned.chmod(0o755)
            handed = run()
            self.assertEqual(handed.returncode, 0, handed.stderr)
            self.assertEqual(handed.stdout.splitlines(),
                             [str(pinned), "-B", str(gate), "--profile", "linux", "-j", "2"])

            looped = run(guard=str(pinned))
            self.assertNotEqual(looped.returncode, 0)
            self.assertIn("handed off to", looped.stderr)

    def test_receipts_spell_runtimes_under_home_without_the_personal_path(self):
        root = self.fixture.runtime_roots["procedure"].resolve()
        with patch.object(Path, "home", return_value=root.parent):
            identity = python_runtime.runtime_identity()
        prefix = "~/" + root.name
        self.assertEqual(identity["runtime_root"], prefix)
        self.assertEqual(identity["resolved_path"], prefix + "/bin/python")
        self.assertEqual((identity["stdlib"], identity["platstdlib"]), (prefix + "/lib/python",) * 2)
        self.assertNotIn(str(root.parent), json.dumps(identity))
        with patch.object(Path, "home", return_value=root.parent / "elsewhere"):
            self.assertEqual(python_runtime.runtime_identity()["runtime_root"], str(root))

    def test_each_procedure_and_driver_runtime_component_is_bound(self):
        names = ("bin/python", "lib/python/json/__init__.py", "lib/python/json/__pycache__/__init__.pyc",
                 "lib/python/lib-dynload/_json.so", "Python")
        for role, root in self.fixture.runtime_roots.items():
            for name in names:
                with self.subTest(role=role, component=name):
                    path = root / name
                    original = path.read_bytes()
                    path.write_bytes(original + b"changed runtime implementation")
                    identity = self.resources.build_identity(self.fixture.root, BUILD)
                    for key in ("inputs_sha256", "compiler_sha256", "toolchain_sha256"):
                        self.assertEqual(identity[key], self.fixture.report["identity"][key])
                    self.assert_stale()
                    path.write_bytes(original)
                    self.assertEqual(self.fixture.check(), [])

    def test_procedure_executable_bytes_and_same_byte_alternate_path_are_bound(self):
        root = self.fixture.runtime_roots["procedure"]
        original = root / "bin/python"
        description = self.fixture.runtime_description
        for same_bytes in (False, True):
            alternate = root / "bin/alternate-python"
            alternate.write_bytes(original.read_bytes() if same_bytes else b"alternate executable")
            # Record both binary files first so path selection is the sole change.
            self.fixture.report = self.fixture.build()
            self.fixture.save()
            def switched(executable=None):
                result = description(executable)
                if executable is None:
                    result["executable"] = str(alternate)
                return result
            with patch.object(python_runtime, "runtime_description", side_effect=switched):
                self.assert_stale()
            self.assertEqual(self.fixture.check(), [])

    def test_runtime_entry_addition_removal_and_alias_target_are_bound(self):
        root = self.fixture.runtime_roots["procedure"]
        cache = root / "lib/python/new.pyc"
        cache.write_bytes(b"new executable runtime cache")
        self.assert_stale()
        cache.unlink()
        source = root / "lib/python/json/__init__.py"
        original = source.read_bytes()
        source.unlink()
        self.assert_stale()
        source.write_bytes(original)
        alias = root / "shared-library"
        alias.symlink_to("Python")
        self.fixture.report = self.fixture.build()
        self.fixture.save()
        alias.unlink()
        alias.symlink_to("bin/python")
        self.assert_stale()

    def test_diagnostic_inventory_keeps_the_same_identity_and_tracks_entry_changes(self):
        root = self.fixture.runtime_roots["procedure"].resolve()
        alias = root / "shared-library"
        alias.symlink_to("Python")
        files = {}
        self.assertEqual(python_runtime.runtime_identity(inventory=files), python_runtime.runtime_identity())
        self.assertEqual(set(files), {p.relative_to(root).as_posix() for p in root.rglob("*")})
        for name in ("Python", "lib/python/json/__pycache__/__init__.pyc", "shared-library"):
            with self.subTest(path=name):
                path = root / name
                original = path.read_bytes()
                if path.is_symlink():
                    path.unlink()
                    path.symlink_to("bin/python")
                else:
                    path.write_bytes(original + b"changed")
                changed = {}
                python_runtime.runtime_identity(inventory=changed)
                self.assertNotEqual(files[name], changed[name])
                if path.is_symlink():
                    path.unlink()
                    path.symlink_to("Python")
                else:
                    path.write_bytes(original)

    def test_discovery_schema_paths_and_optimization_are_exact(self):
        description = self.fixture.runtime_description
        valid = description()
        for changed in ({**valid, "extra": "unknown"}, {k: v for k, v in valid.items() if k != "stdlib"},
                        {**valid, "optimize": True}, {**valid, "stdlib": str(self.fixture.root)},
                        {**valid, "base_prefix": str(self.fixture.root / "absent")}):
            with self.subTest(description=changed), patch.object(python_runtime, "runtime_description", return_value=changed):
                with self.assertRaises((ValueError, OSError)):
                    self.fixture.check()
        def optimized(executable=None):
            result = description(executable)
            if executable is None:
                result["optimize"] = 1
            return result
        with patch.object(python_runtime, "runtime_description", side_effect=optimized):
            self.assert_stale()

    def test_unrelated_global_packages_are_excluded_and_runtime_aliases_fail_closed(self):
        root = self.fixture.runtime_roots["procedure"]
        packages = root / "lib/python/site-packages"
        packages.symlink_to(self.fixture.root / "apps", target_is_directory=True)
        self.assertEqual(self.fixture.check(), [])
        alias = root / "lib/python/escaped-runtime.py"
        alias.symlink_to(self.fixture.root / "rust/firmware/native/main.cpp")
        with self.assertRaisesRegex(ValueError, "runtime alias"):
            self.fixture.check()
        alias.unlink()
        alias.symlink_to(root / "missing.py")
        with self.assertRaises(OSError):
            self.fixture.check()

    def test_excluded_package_aliases_and_discovery_paths_fail_closed_for_both_roles(self):
        original = self.fixture.runtime_description
        for role, root in self.fixture.runtime_roots.items():
            excluded = root / "lib/python/site-packages/alias-json"
            excluded.mkdir(parents=True)
            (excluded / "__init__.py").write_bytes(b"executed alias source")
            alias = root / "lib/python/imported-json"
            alias.symlink_to(excluded, target_is_directory=True)
            with self.subTest(role=role, alias=True), self.assertRaisesRegex(ValueError, "excluded site-packages"):
                self.fixture.check()
            alias.unlink()
            for field in ("stdlib", "platstdlib"):
                def excluded_library(executable=None):
                    result = original(executable)
                    if (executable is not None) == (role == "driver"):
                        result[field] = str(excluded)
                    return result
                with self.subTest(role=role, field=field), \
                        patch.object(python_runtime, "runtime_description", side_effect=excluded_library), \
                        self.assertRaisesRegex(ValueError, "excluded site-packages"):
                    self.fixture.check()
            self.assertEqual(self.fixture.check(), [])

    def test_owned_sibling_directory_aliases_bind_source_and_bytecode_for_both_roles(self):
        for role, root in self.fixture.runtime_roots.items():
            alias = root / "lib/python/imported-json"
            alias.symlink_to("json", target_is_directory=True)
            self.fixture.report = self.fixture.build()
            self.fixture.save()
            self.assertEqual(self.fixture.check(), [])
            for name in ("__init__.py", "__pycache__/__init__.pyc"):
                with self.subTest(role=role, component=name):
                    target = alias / name
                    original = target.read_bytes()
                    target.write_bytes(original + b"changed executable alias content")
                    self.assert_stale()
                    target.write_bytes(original)
                    self.assertEqual(self.fixture.check(), [])

    def test_prior_helper_control_omits_excluded_alias_contents(self):
        source = (Path(__file__).resolve().parents[1] / "tools/python_runtime.py").read_text()
        guard = ('                if "site-packages" in target.relative_to(root).parts:\n'
                 '                    raise ValueError("Python runtime alias enters excluded site-packages")\n')
        self.assertEqual(source.count(guard), 1)
        old = {"__name__": "prior_runtime_alias_control"}
        exec(compile(source.replace(guard, ""), "prior_runtime_alias_control.py", "exec"), old)
        for role, root in self.fixture.runtime_roots.items():
            root = root.resolve()
            excluded = root / "lib/python/site-packages/alias-json"
            excluded.mkdir(parents=True)
            alias = root / "lib/python/imported-json"
            alias.symlink_to(excluded, target_is_directory=True)
            for name in ("__init__.py", "__init__.pyc", "_json.so"):
                with self.subTest(role=role, component=name):
                    target = excluded / name
                    target.write_bytes(b"executable alias content")
                    before = old["runtime_hash"](root)
                    target.write_bytes(b"changed executable alias content")
                    self.assertEqual(old["runtime_hash"](root), before)
                    with self.assertRaisesRegex(ValueError, "excluded site-packages"):
                        python_runtime.runtime_hash(root)

    def test_old_driver_executable_only_identity_accepts_runtime_mutations(self):
        # Calibrated control uses the actual prior collector, not a test clone.
        source = history_fixtures.text("5030065", "tools/code_health_resources.py")
        old = {"__name__": "old_resource_control"}
        exec(compile(source, "old_resource_control.py", "exec"), old)
        # That collector read a regional build's SDK copy from this older path.
        legacy_sdk = self.fixture.root / ".build/freeink-sdk/pinned.h"
        legacy_sdk.parent.mkdir(parents=True)
        legacy_sdk.write_text("// regional SDK copy where the old collector reads it")
        # It also hashed the C++ source roots and generators NEWT-90 removed.
        for directory in ("src", "include", "firmware"):
            (self.fixture.root / directory).mkdir()
        for name in ("make-reader-font.py", "make-frosted-home.py", "make-sleep-brand.py"):
            (self.fixture.root / "tools" / name).write_text("# authored build procedure")
        with patch.object(self.resources, "build_identity", side_effect=old["build_identity"]):
            self.fixture.report = self.fixture.build()
            self.fixture.save()
            for role, names in (("procedure", ("bin/python", "lib/python/json/__init__.py",
                                                "lib/python/json/__pycache__/__init__.pyc", "Python")),
                                ("driver", ("lib/python/json/__init__.py", "lib/python/lib-dynload/_json.so"))):
                for name in names:
                    with self.subTest(role=role, component=name):
                        path = self.fixture.runtime_roots[role] / name
                        original = path.read_bytes()
                        path.write_bytes(original + b"mutated runtime")
                        self.assertEqual(self.fixture.check(), [])
                        path.write_bytes(original)

    def test_available_alternate_interpreter_stales_actual_runtime_receipt(self):
        alternate = Path(__file__).resolve().parents[1] / ".toolchain/platformio/penv/bin/python"
        self.assertTrue(alternate.is_file(), "Approved existing alternate interpreter is required")
        # Probe only approved existing runtimes, with no bytecode writes or installs.
        original = self.fixture.runtime_description
        def alternate_description(flags=()):
            return json.loads(REAL_RUN([str(alternate), "-I", "-B", *flags, "-c",
                "import sys;sys.path.insert(0,sys.argv[1]);from python_runtime import runtime_description;"
                "import json;print(json.dumps(runtime_description()))",
                str(alternate.parents[4] / "tools")], check=True, capture_output=True, text=True).stdout)
        def selected_procedure(description):
            return lambda executable=None: original(executable) if executable is not None else dict(description)
        def actual_procedure(executable=None):
            return original(executable) if executable is not None else self.real_description()
        with patch.object(python_runtime, "runtime_description", side_effect=actual_procedure):
            self.fixture.report = self.fixture.build()
            self.fixture.save()
            self.assertEqual(self.fixture.check(), [])
            current = self.fixture.report["identity"]["procedure_python"]
            # A venv may share the gate's exact resolved interpreter and runtime.
            # Resource measurement mocks stay local; both process probes are real.
            with patch.object(python_runtime, "runtime_description",
                              side_effect=selected_procedure(alternate_description())):
                alternate_identity = self.resources.build_identity(self.fixture.root, BUILD)["procedure_python"]
                if alternate_identity == current:
                    self.assertEqual(self.fixture.check(), [])
                else:
                    self.assert_stale()
            # Always retain a real distinct-runtime control, even with one Python
            # installation. The collector must bind the process optimization mode.
            flag = "-O" if sys.flags.optimize != 1 else "-OO"
            optimized = alternate_description((flag,))
            self.assertNotEqual(optimized["optimize"], sys.flags.optimize)
            with patch.object(python_runtime, "runtime_description", side_effect=selected_procedure(optimized)):
                actual = self.resources.build_identity(self.fixture.root, BUILD)["procedure_python"]
                self.assertNotEqual(actual, alternate_identity)
                self.assertNotEqual(actual, current)
                self.assert_stale()


if __name__ == "__main__":
    unittest.main()
