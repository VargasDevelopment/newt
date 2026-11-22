"""Actual native-tool write paths preserve rejected aliases and use exclusive staging."""
import hashlib
from contextlib import ExitStack, contextmanager, redirect_stdout
import importlib.util
import io
import json
import marshal
import os
from pathlib import Path
import py_compile
import re
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import code_health
import code_health_resources as resources
import daily_firmware
from freeink_sdk_routes import FIRMWARE_BUILD as BUILD
import native_output
import native_toolchain
import history_fixtures


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "tools" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


build = load("firmware-build")
sdk = load("apply-freeink-patch")
guard = load("verify-nvs-guard")
SENTINEL = b"existing target must survive rejected output\n"
KINDS = ("inside", "outside", "broken", "directory", "hardlink-inside", "hardlink-outside",
         "parent-inside", "parent-outside", "parent-broken", "parent-file", "root-alias", "root-parent",
         "new", "existing")


class NativeOutputTests(unittest.TestCase):
    def fixture(self, relative, kind):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        sandbox = Path(temporary.name).resolve()
        root = sandbox / "project"
        root.mkdir()
        output = root / relative
        output.parent.mkdir(parents=True)
        protected = None
        target = (sandbox if "outside" in kind else root) / "alias-target"
        if kind in ("inside", "outside", "broken", "hardlink-inside", "hardlink-outside"):
            if kind != "broken":
                target.write_bytes(SENTINEL)
                protected = target
            if kind.startswith("hardlink"):
                output.hardlink_to(target)
            else:
                output.symlink_to(target)
        elif kind == "directory":
            output.mkdir()
            protected = output / "preserve"
            protected.write_bytes(SENTINEL)
        elif kind.startswith("parent-"):
            output.parent.rmdir()
            if kind == "parent-file":
                output.parent.write_bytes(SENTINEL)
                protected = output.parent
            elif kind != "parent-broken":
                target.mkdir()
                protected = target / output.name
                protected.write_bytes(SENTINEL)
            if kind != "parent-file":
                output.parent.symlink_to(target, target_is_directory=True)
        elif kind == "root-alias":
            alias = sandbox / "root-alias"
            alias.symlink_to(root, target_is_directory=True)
            root = alias
        elif kind == "root-parent":
            alias = sandbox / "parent-alias"
            alias.symlink_to(sandbox, target_is_directory=True)
            root = alias / "project"
        elif kind == "existing":
            output.write_bytes(SENTINEL)
        return root, root / relative, protected

    def check_protected(self, protected):
        if protected is not None:
            self.assertEqual(protected.read_bytes(), SENTINEL)

    def run_health(self, root, variant, driver=build, *, clean=True, calls=None, native_write=None,
                   config_source=None, locks=None, variant_flags=()):
        # Only historical builders select their variant with flags.
        arguments = [*(["--clean", "--health-report"] if clean else []), *variant_flags]
        if config_source is not None:  # The historical builder's own C++ configuration.
            (root / "platformio.ini").write_text(config_source)
        if driver is build:
            (root / daily_firmware.SOURCE_CONFIGURATION).write_bytes(
                (ROOT / daily_firmware.SOURCE_CONFIGURATION).read_bytes())
        calls = [] if calls is None else calls
        locks = [] if locks is None else locks

        def observed(step, result):
            locks.append((step, native_toolchain.held(root)))
            return result

        def run(command, **kwargs):
            calls.append(command)
            if str(root / 'tools/crt_bundle.py') in command and driver is build:
                bundle = root / '.build/regional-dev-usb/crt/x509_crt_bundle'
                bundle.parent.mkdir(parents=True, exist_ok=True)
                bundle.write_bytes(b'native certificate fixture')
            if command[0] == "mock-pio":
                observed("clean" if command[-2:] == ["-t", "clean"] else "build", None)
            elif command[0] == sys.executable and Path(command[2]).name == "verify-nvs-guard.py":
                observed("verify", None)
            if command[0] == "mock-pio" and command[-2:] != ["-t", "clean"] and native_write is not None:
                native_write.write_bytes(b"fixture native output replaced protected target\n")
            output = {"rev-parse": "0123456789ab\n", "show": "2026-10-27\n"}.get(
                command[1] if command[:1] == ["git"] else "", "")
            return subprocess.CompletedProcess(command, 0, stdout=output)

        with ExitStack() as context:
            context.enter_context(patch.object(daily_firmware, 'CRT_BUNDLE_SHA256',
                                              hashlib.sha256(b'native certificate fixture').hexdigest()))
            share = getattr(driver, "toolchain_share", None)
            if share is not None:
                check = share.check_views
                context.enter_context(patch.object(share, "check_views",
                                                   side_effect=lambda checked: observed("views", check(checked))))
            context.enter_context(patch.object(driver, "ROOT", root))
            context.enter_context(patch.object(driver, "TOOLCHAIN", root / ".toolchain"))
            context.enter_context(patch.object(driver, "require_toolchain", return_value=root / "mock-pio"))
            context.enter_context(patch.object(driver, "build_command", return_value=["mock-pio"]))
            context.enter_context(patch.object(driver, "source_identity", create=True, return_value={
                "firmware_version": "0.1.0-alpha.0", "git_revision": "0123456789ab" + "0" * 28, "dirty": False}))
            context.enter_context(patch.object(driver.subprocess, "run", side_effect=run))
            context.enter_context(patch.object(code_health, "read_baseline", return_value={}))
            context.enter_context(patch.object(code_health, "base_failures", return_value=[]))
            context.enter_context(patch.object(resources, "build_identity",
                                               side_effect=lambda *_: observed("identity", {})))
            image = (resources.build_directory(root, variant) / "firmware.bin").relative_to(root).as_posix()
            context.enter_context(patch.object(resources, "build_report", side_effect=lambda *_: observed("report", {
                "result": "fixture", "identity": {}, "artifacts": {image: "a" * 64}})))
            context.enter_context(patch.object(resources, "check_resources",
                                               side_effect=lambda *_: observed("check", [])))
            context.enter_context(patch.object(sys, "argv", ["firmware-build.py", *arguments]))
            context.enter_context(redirect_stdout(io.StringIO()))
            self.assertIs(driver.main.__globals__, vars(driver), "Fixture patches must bind the executed function globals")
            self.assertEqual(driver.main.__globals__["ROOT"], root)
            driver.main()
        return calls

    def run_sdk(self, root, driver=sdk):
        source = root / "vendor/freeink-sdk"
        source.mkdir(parents=True, exist_ok=True)
        vendor_marker = source / "pinned-source"
        vendor_marker.write_bytes(SENTINEL)
        # The archive fixture must match the tracked-file inventory, as the
        # real patcher verifies both unchanged bytes and unexpected members.
        (source / "LICENSE").write_text("fixture license")
        (source / "new-sdk").write_text("verified fixture SDK")
        patch_file = root / "patches/regional.patch"
        patch_file.parent.mkdir(exist_ok=True)
        patch_file.write_text("fixture patch")
        manifest = patch_file.with_suffix(".json")
        manifest.write_text(json.dumps({"source_commit": driver.SDK_PIN, "patch_sha256": driver.digest(patch_file), "files": {}}))
        destination = root / ".build/freeink-sdk"
        if destination.parent.is_dir():
            destination.mkdir(exist_ok=True)
            (destination / "previous-sdk").write_bytes(SENTINEL)

        def git(*arguments):
            return {"rev-parse": "pin", "status": "", "remote": "https://github.com/Free-Ink/freeink-sdk.git"}[arguments[0]]

        def run(command, **kwargs):
            if command[0] == "git" and "ls-files" in command:
                return subprocess.CompletedProcess(command, 0, stdout=b"LICENSE\0new-sdk\0")
            if command[0] == "tar":
                staging = Path(command[command.index("-C") + 1])
                (staging / "LICENSE").write_text("fixture license")
                (staging / "new-sdk").write_text("verified fixture SDK")
            return subprocess.CompletedProcess(command, 0, stdout=b"")

        with ExitStack() as context:
            context.enter_context(patch.multiple(driver, ROOT=root, SOURCE=source, PATCH=patch_file,
                                                MANIFEST=manifest, BUILD=root / ".build", DESTINATION=destination))
            context.enter_context(patch.object(driver, "git", side_effect=git))
            context.enter_context(patch.object(driver, "verify_source_pin", return_value=driver.SDK_PIN))
            context.enter_context(patch.object(driver.subprocess, "run", side_effect=run))
            context.enter_context(redirect_stdout(io.StringIO()))
            driver.main()
        self.assertEqual(vendor_marker.read_bytes(), SENTINEL)

    def run_guard(self, root, output, driver=guard):
        elf = root / "fixture.elf"
        elf.write_bytes(b"fixture ELF")
        elf.with_name("firmware.map").write_text("fixture map")
        functions = {"initArduino": ["__wrap_nvs_flash_init", "__wrap_esp_partition_erase_range", "verifyRollbackLater"],
                     "__wrap_nvs_flash_init": ["nvs_flash_init"],
                     "nvs::NVSPartition::write()": ["__wrap_esp_partition_write"],
                     "nvs::NVSPartition::write_raw()": ["__wrap_esp_partition_write_raw"],
                     "nvs::NVSPartition::erase_range()": ["__wrap_esp_partition_erase_range"]}
        with ExitStack() as context:
            context.enter_context(patch.object(driver, "ROOT", root))
            context.enter_context(patch.object(driver.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, stdout="fixture")))
            context.enter_context(patch.object(driver, "parse_functions", return_value=functions))
            context.enter_context(patch.object(driver, "ordered_calls", side_effect=lambda lines: lines))
            context.enter_context(patch.object(driver, "validate_sleep_boundary", return_value={"result": "fixture"}))
            context.enter_context(patch.object(driver, "validate_reader_stack", return_value={"result": "fixture"}))
            context.enter_context(patch.object(driver, "validate_rollback_hook", return_value=None))
            context.enter_context(patch.object(driver, "validate_no_timer_task", return_value=None))
            context.enter_context(patch.object(driver, "validate_flash_writers", return_value={"result": "fixture"}))
            context.enter_context(patch.object(driver, "validate_crash_output", return_value={"result": "fixture"}))
            context.enter_context(patch.object(sys, "argv", ["verify-nvs-guard.py", "--elf", str(elf), "--output", str(output)]))
            context.enter_context(redirect_stdout(io.StringIO()))
            driver.main()

    def test_all_health_variants_reject_aliases_and_accept_real_destinations(self):
        for variant in resources.VARIANTS:
            for kind in KINDS:
                with self.subTest(variant=variant, destination=kind):
                    root, output, protected = self.fixture(f"artifacts/code-health-{variant}.json", kind)
                    if kind in ("new", "existing"):
                        calls = self.run_health(root, variant)
                        self.assertEqual(json.loads(output.read_text())["result"], "fixture")
                        self.assertTrue(any(command[0] == "mock-pio" for command in calls))
                    else:
                        with self.assertRaises((RuntimeError, OSError)):
                            self.run_health(root, variant)
                        self.check_protected(protected)

    def test_health_build_refuses_a_leftover_build_script_backup_before_capture(self):
        root, _, _ = self.fixture(f"artifacts/code-health-{BUILD}.json", "new")
        libs = root / ".toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3"
        libs.mkdir(parents=True)
        (libs / "pioarduino-build.py.esp32c3").write_bytes(SENTINEL)
        calls = []
        with self.assertRaisesRegex(RuntimeError, "unfinished PlatformIO run"):
            self.run_health(root, BUILD, calls=calls)
        self.assertEqual([command for command in calls if command[0] == "mock-pio"],
                         [["mock-pio", "--project-conf", str(root / f".build/{BUILD}/platformio.ini"), "-t", "clean"]])
        self.assertFalse((root / f"artifacts/code-health-{BUILD}.json").exists())

    @contextmanager
    def returning_within(self, seconds):
        def hung(*_):
            raise TimeoutError("the health build waits on its own toolchain lock")
        previous = signal.signal(signal.SIGALRM, hung)
        signal.alarm(seconds)
        try:
            yield
        finally:
            signal.alarm(0)
            signal.signal(signal.SIGALRM, previous)

    def test_health_build_checks_its_shared_toolchain_view_under_the_lock(self):
        root, receipt, _ = self.fixture(f"artifacts/code-health-{BUILD}.json", "new")
        self.addCleanup(lambda: [os.chmod(directory, 0o755) for directory, _, _ in os.walk(root.parent)])
        framework = root / ".toolchain/platformio/packages/framework-arduinoespressif32"
        framework.mkdir(parents=True)
        for name in ("idf_component.yml", "idf_component.yml.orig"):
            (framework / name).write_text("dependencies: {}\n")
        store = root.parent / "store"
        store.mkdir()
        share = build.toolchain_share
        with patch.object(share, "ROOT", root), redirect_stdout(io.StringIO()):
            share.link(store, "platformio", share.store(store, "platformio", root / ".toolchain/platformio"))
        locks = []
        with self.returning_within(30):
            self.run_health(root, BUILD, locks=locks)
        self.assertEqual(locks[:3], [("clean", "shared"), ("views", "exclusive"), ("identity", "exclusive")])
        receipt.unlink()
        # A write through the view reaches every checkout's store inode.
        (framework / "idf_component.yml").chmod(0o644)
        calls = []
        with self.assertRaisesRegex(RuntimeError, "differs from the store"):
            self.run_health(root, BUILD, calls=calls)
        self.assertEqual([command for command in calls if command[0] == "mock-pio"],
                         [["mock-pio", "--project-conf", str(root / f".build/{BUILD}/platformio.ini"), "-t", "clean"]])
        self.assertFalse(receipt.exists())

    def test_health_build_holds_the_toolchain_lock_through_each_platformio_run(self):
        # Lanes share one framework that every PlatformIO run edits (tools/native_toolchain.py).
        for variant in resources.VARIANTS:
            with self.subTest(variant=variant):
                root, _, _ = self.fixture(f"artifacts/code-health-{variant}.json", "new")
                locks = []
                self.run_health(root, variant, locks=locks)
                self.assertEqual(locks, [("clean", "shared"), ("views", "exclusive"), ("identity", "exclusive"),
                                         ("build", "exclusive"),
                                         ("verify", "exclusive"), ("report", "exclusive"),
                                         ("check", "exclusive")])
                self.assertEqual(native_toolchain.held(root), "free")

    def test_compiler_output_descendants_reject_aliases_before_any_child_or_write(self):
        cases = [(name, kind) for name in ("newt_x3/src/firmware/main.cpp.o", "newt_x3/src/firmware/main.cpp.d",
                                          "project.checksum")
                 for kind in ("inside", "outside", "broken", "hardlink-inside", "hardlink-outside", "fifo")]
        cases += [("newt_x3/src/main.cpp.o", kind) for kind in ("parent-inside", "parent-outside", "parent-broken")]
        for variant in resources.VARIANTS:
            prefix = f".pio/{variant}/build"
            for clean in (False, True):
                for name, kind in cases:
                    with self.subTest(variant=variant, clean=clean, path=name, alias=kind):
                        root, output, protected = self.fixture(f"{prefix}/{name}", kind)
                        if kind == "fifo":
                            os.mkfifo(output)
                        calls = []
                        with self.assertRaises((RuntimeError, OSError)):
                            self.run_health(root, variant, clean=clean, calls=calls, native_write=output)
                        self.assertEqual(calls, [], "No generator or native child may run before rejection")
                        self.check_protected(protected)
                        if "broken" in kind:
                            self.assertFalse(output.exists())

    def test_real_compiler_output_trees_preserve_dependency_links_and_allow_dispatch(self):
        for variant in resources.VARIANTS:
            prefix = f".pio/{variant}/build"
            for clean in (False, True):
                with self.subTest(variant=variant, clean=clean):
                    root, output, _ = self.fixture(f"{prefix}/newt_x3/src/firmware/main.cpp.o", "existing")
                    output.with_suffix(".d").write_bytes(SENTINEL)
                    checksum = root / prefix / "project.checksum"
                    checksum.write_bytes(SENTINEL)
                    dependency = root / prefix.replace("build", "libdeps") / "newt_x3/BoardConfig"
                    dependency.parent.mkdir(parents=True)
                    vendor = root / "vendor/freeink-sdk/BoardConfig"
                    vendor.mkdir(parents=True)
                    dependency.symlink_to(vendor, target_is_directory=True)
                    compiler = root / ".toolchain/platformio/packages/compiler/bin/compiler"
                    compiler.parent.mkdir(parents=True)
                    compiler.symlink_to(checksum)
                    calls = self.run_health(root, variant, clean=clean)
                    self.assertEqual(sum(command[0] == "mock-pio" for command in calls), 2 if clean else 1)
                    self.assertEqual(output.read_bytes(), SENTINEL)
                    self.assertEqual(checksum.read_bytes(), SENTINEL)
                    self.assertTrue(dependency.is_symlink() and compiler.is_symlink())

    def test_old_builder_control_dispatches_and_writes_through_descendant_aliases(self):
        source = history_fixtures.text("fe4c4fa", "tools/firmware-build.py")
        # The historical control must use its own reviewed input scope. The
        # current SD candidate adds sources and an XML wrapper linker flag.
        config = history_fixtures.text("fe4c4fa", "platformio.ini")
        from types import ModuleType
        old = ModuleType("old_build_descendant_control")
        old.__file__ = str(ROOT / "tools/firmware-build.py")
        exec(compile(source, old.__file__, "exec"), old.__dict__)
        for variant in resources.VARIANTS:
            prefix = f".pio/{variant}/build"
            for clean in (False, True):
                for kind in ("outside", "hardlink-outside", "parent-outside"):
                    with self.subTest(variant=variant, clean=clean, alias=kind):
                        root, output, protected = self.fixture(f"{prefix}/newt_x3/src/main.cpp.o", kind)
                        calls = self.run_health(root, variant, driver=old, clean=clean, native_write=output,
                                                config_source=config, variant_flags=["--regional-home", "--dev-usb"])
                        self.assertTrue(any(command[0] == "mock-pio" for command in calls))
                        self.assertEqual(protected.read_bytes(), b"fixture native output replaced protected target\n")

    def test_sdk_provenance_rejection_preserves_existing_sdk_before_replacement(self):
        for kind in KINDS:
            with self.subTest(destination=kind):
                root, output, protected = self.fixture(".build/freeink-sdk.provenance.json", kind)
                if kind in ("new", "existing"):
                    self.run_sdk(root)
                    self.assertIn("patched_tree_sha256", json.loads(output.read_text()))
                    self.assertTrue((root / ".build/freeink-sdk/new-sdk").is_file())
                    self.assertFalse((root / ".build/freeink-sdk/previous-sdk").exists())
                else:
                    with self.assertRaises((RuntimeError, OSError)):
                        self.run_sdk(root)
                    self.check_protected(protected)
                    previous = root / ".build/freeink-sdk/previous-sdk"
                    if previous.parent.is_dir():
                        self.assertEqual(previous.read_bytes(), SENTINEL)
                self.assertEqual((root / "vendor/freeink-sdk/pinned-source").read_bytes(), SENTINEL)

    def test_guard_main_rejects_aliases_and_accepts_real_destinations(self):
        for kind in KINDS:
            with self.subTest(destination=kind):
                root, output, protected = self.fixture("artifacts/nvs-guard.json", kind)
                if kind in ("new", "existing"):
                    self.run_guard(root, output)
                    self.assertEqual(json.loads(output.read_text())["result"], "PASS")
                else:
                    with self.assertRaises((RuntimeError, OSError)):
                        self.run_guard(root, output)
                    self.check_protected(protected)

    def test_guard_rejects_escapes_and_authored_sources_without_touching_targets(self):
        root, _, _ = self.fixture("artifacts/nvs-guard.json", "new")
        target = root.parent / "outside.json"
        target.write_bytes(SENTINEL)
        authored = root / "firmware/main.cpp"
        authored.parent.mkdir()
        authored.write_bytes(SENTINEL)
        for argument in (target, Path("../outside.json"), authored, Path("firmware/main.cpp"),
                         Path("artifacts/../firmware/main.cpp")):
            with self.subTest(argument=argument):
                with self.assertRaises(RuntimeError):
                    self.run_guard(root, argument)
                self.assertEqual(target.read_bytes(), SENTINEL)
                self.assertEqual(authored.read_bytes(), SENTINEL)

    def test_guard_creates_new_artifact_subdirectories(self):
        root, _, _ = self.fixture("artifacts/nvs-guard.json", "new")
        output = root / "artifacts/nested/caller-report.json"
        self.run_guard(root, output)
        self.assertEqual(json.loads(output.read_text())["result"], "PASS")

    def test_standalone_entrypoints_ignore_external_helper_cache_namespace(self):
        root, _, _ = self.fixture("artifacts/nvs-guard.json", "new")
        tools = root / "tools"
        tools.mkdir()
        helper = tools / "native_output.py"
        definitions = "owned_directory = owned_file = write_text = None\n"
        helper.write_text("MARKER = 'source'\n" + definitions)
        (tools / "freeink_sdk_routes.py").write_text((ROOT / "tools/freeink_sdk_routes.py").read_text())
        for name in ("freeink_sdk_provenance.py", "freeink_source_pin.py", "crash_output_guard.py", "crash_console_guard.py", "linked_code.py", "rust_open_stack.py"):
            (tools / name).write_text((ROOT / "tools" / name).read_text())
        prefix = root / "external-cache"
        with patch.object(sys, "pycache_prefix", str(prefix)):
            cache = Path(importlib.util.cache_from_source(str(helper)))
        py_compile.compile(str(helper), cfile=str(cache), doraise=True)
        original = cache.read_bytes()
        altered = original[:16] + marshal.dumps(compile("MARKER = 'cache'\n" + definitions, str(helper), "exec"))
        cache.write_bytes(altered)
        probe = ("import sys; from pathlib import Path\n"
                 "sys.path.insert(0, sys.argv[2])\n"
                 "namespace = {'__name__': 'probe', '__file__': sys.argv[1]}\n"
                 "exec(compile(Path(sys.argv[1]).read_text(), sys.argv[1], 'exec'), namespace)\n"
                 "import native_output; print(native_output.MARKER)\n")
        for name in ("apply-freeink-patch", "verify-nvs-guard"):
            current = tools / (name + ".py")
            current.write_text((ROOT / "tools" / current.name).read_text())
            old = tools / (name + "-old.py")
            old.write_text(current.read_text().replace("sys.pycache_prefix = None", ""))
            for launcher, expected in ((current, "source"), (old, "cache")):
                with self.subTest(entrypoint=name, launcher=launcher.name):
                    process = subprocess.Popen([sys.executable, "-B", "-c", probe, str(launcher), str(tools)],
                                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                               env=dict(os.environ, PYTHONPYCACHEPREFIX=str(prefix)))
                    output, error = process.communicate(timeout=10)
                    self.assertEqual((process.returncode, output, error), (0, expected + "\n", ""))
                    self.assertFalse((tools / "__pycache__").exists())
                    self.assertEqual(cache.read_bytes(), altered)

    def test_health_legacy_temporary_alias_is_unused_and_preserved(self):
        for variant in resources.VARIANTS:
            for kind in ("inside", "outside", "broken", "directory", "hardlink-inside", "hardlink-outside"):
                with self.subTest(variant=variant, legacy=kind):
                    root, legacy, protected = self.fixture(f"artifacts/code-health-{variant}.json.tmp", kind)
                    original = legacy.lstat()
                    self.run_health(root, variant)
                    self.assertEqual(legacy.lstat().st_ino, original.st_ino)
                    self.check_protected(protected)
                    self.assertEqual(json.loads(legacy.with_suffix("").read_text())["result"], "fixture")

    def test_staging_is_exclusive_and_failed_replacement_preserves_existing_file(self):
        root, output, _ = self.fixture("artifacts/report.json", "existing")
        mkstemp = native_output.tempfile.mkstemp
        staged = []

        def create(**arguments):
            descriptor, name = mkstemp(**arguments)
            path = Path(name)
            self.assertEqual(path.parent, output.parent)
            self.assertNotEqual(path, output.with_suffix(".json.tmp"))
            with self.assertRaises(FileExistsError):
                path.open("x")
            staged.append(path)
            return descriptor, name

        with patch.object(native_output.tempfile, "mkstemp", side_effect=create):
            with patch.object(native_output.os, "replace", side_effect=OSError("fixture replacement failure")):
                with self.assertRaisesRegex(OSError, "replacement failure"):
                    native_output.write_text(root, output, "new receipt")
            self.assertEqual(output.read_bytes(), SENTINEL)
            native_output.write_text(root, output, "new receipt")
        self.assertEqual(output.read_text(), "new receipt")
        self.assertEqual(len(staged), 2)
        self.assertNotEqual(staged[0], staged[1])
        self.assertFalse(any(path.exists() for path in staged))

    def test_a_directory_created_concurrently_is_checked_like_an_existing_one(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        sandbox = Path(temporary.name).resolve()
        root, outside = sandbox / "project", sandbox / "outside"
        root.mkdir()
        outside.mkdir()
        mkdir = Path.mkdir

        def racing(alias):
            # Another writer creates the entry first: a directory, or an alias to outside.
            def create(path, *arguments, **options):
                path.symlink_to(outside, target_is_directory=True) if alias else mkdir(path)
                raise FileExistsError(path)
            return create

        with patch.object(Path, "mkdir", racing(False)):
            self.assertEqual(native_output.owned_directory(root, root / "build/tmp", create=True), root / "build/tmp")
        self.assertTrue((root / "build/tmp").is_dir())
        with patch.object(Path, "mkdir", racing(True)):
            with self.assertRaisesRegex(RuntimeError, "real project-local directories"):
                native_output.owned_directory(root, root / "aliased/tmp", create=True)
        self.assertEqual(list(outside.iterdir()), [])

    def test_an_optimized_interpreter_writes_no_evidence(self):
        root, output, _ = self.fixture("artifacts/report.json", "new")
        tools = Path(native_output.__file__).resolve().parent
        code = (f"import sys\nsys.path.insert(0, {str(tools)!r})\nimport native_output\n"
                f"native_output.write_text({str(root)!r}, {str(output)!r}, 'receipt')\n")
        for options, environment in ((["-O"], {}), ([], {"PYTHONOPTIMIZE": "1"})):
            with self.subTest(options=options, environment=environment):
                result = subprocess.run([sys.executable, *options, "-B", "-c", code], capture_output=True, text=True,
                                        timeout=60, env={**os.environ, **environment})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("without -O or PYTHONOPTIMIZE", result.stderr)
                self.assertFalse(output.exists())
                self.assertEqual(list(output.parent.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
