"""Resource gates fail on real evidence changes, not just editable report totals."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import call, patch

from resource_fixture import ACTUAL_SIZE_OUTPUT, BUILD, MEMORY_MAP, SIZE_OUTPUT, STACK, ResourceFixture, recorded
import code_health_resources as resources
import code_health_sram as sram
import freeink_sdk_provenance as sdk_provenance
from python_runtime import home_relative

PROJECT = Path(__file__).resolve().parents[1]
sdk_spec = importlib.util.spec_from_file_location("resource_freeink_patch", PROJECT / "tools/apply-freeink-patch.py")
sdk_patch = importlib.util.module_from_spec(sdk_spec)
sdk_spec.loader.exec_module(sdk_patch)


class ResourceTests(ResourceFixture):
    def dependency_routes(self):
        return resources.require_dependency_ownership(
            self.root, BUILD, resources.artifact_paths(self.root, BUILD))

    def test_exact_ceilings_pass_with_iram_and_no_alias_double_count(self):
        self.assertEqual(self.report["ram_bytes"], 160)
        self.assertEqual(self.check(), [])

    def test_build_identity_crosses_the_canonical_sdk_boundary(self):
        self.sdk_validation.reset_mock()
        resources.build_identity(self.root, BUILD)
        self.assertEqual(self.sdk_validation.call_args_list, [call(self.root, BUILD)])

    def test_joint_sdk_tree_receipt_and_manifest_forgery_fails_all_resource_boundaries(self):
        for stem in ("freeink-producer-policy", "freeink-regional-gray"):
            for suffix in (".patch", ".json"):
                shutil.copy2(PROJECT / "patches" / (stem + suffix),
                             self.root / "patches" / (stem + suffix))
        self.sdk_validation_patch.stop()
        canonical = patch.object(resources, "validate_reader_copy",
                                 side_effect=lambda root, variant: sdk_provenance.validate_reader_copy(
                                     root, variant, PROJECT / "vendor/freeink-sdk"))
        canonical.start()
        self.addCleanup(canonical.stop)
        def compose():
            with patch.multiple(sdk_patch, ROOT=self.root, SOURCE=PROJECT / "vendor/freeink-sdk",
                                BUILD=self.root / ".build"):
                sdk_patch.main(["--reader-lane", BUILD])
        compose()
        self.report = self.build()
        self.save()

        manifest_path = self.root / "patches/freeink-producer-policy.json"
        original_manifest = manifest_path.read_bytes()
        manifest = json.loads(manifest_path.read_text())
        changed_name = next(iter(manifest["files"]))
        changed = self.root / resources.reader_sdk(BUILD) / changed_name
        changed.write_bytes(changed.read_bytes() + b"forged patched SDK bytes")
        manifest["files"][changed_name]["patched_sha256"] = sdk_patch.digest(changed)
        manifest_path.write_text(json.dumps(manifest))
        provenance = self.root / resources.reader_sdk_provenance(BUILD)
        receipt = json.loads(provenance.read_text())
        forged_tree = sdk_patch.tree_digest(self.root / resources.reader_sdk(BUILD))
        receipt["patches"][-1]["manifest_sha256"] = sdk_patch.digest(manifest_path)
        receipt["patches"][-1]["files"][changed_name]["result_sha256"] = sdk_patch.digest(changed)
        receipt["patches"][-1]["result_tree_sha256"] = forged_tree
        receipt["patched_tree_sha256"] = forged_tree
        provenance.write_text(json.dumps(receipt))

        self.assertEqual(manifest["patch_sha256"], sdk_patch.digest(self.root / "patches/freeink-producer-policy.patch"))
        with self.assertRaisesRegex(RuntimeError, "applied patch"):
            sdk_provenance.validate_reader_copy(self.root, BUILD, PROJECT / "vendor/freeink-sdk")
        with self.assertRaisesRegex(ValueError, "applied patch"):
            resources.build_identity(self.root, BUILD)
        with patch.object(resources, "validate_reader_copy"):
            self.report["identity"] = resources.build_identity(self.root, BUILD)
            self.save()
            self.assertEqual(resources.check_resources(self.root, self.budget, BUILD), [])
        with self.assertRaisesRegex(ValueError, "applied patch"):
            resources.check_resources(self.root, self.budget, BUILD)

        manifest_path.write_bytes(original_manifest)
        compose()
        self.report = self.build()
        self.save()
        target = self.root / resources.reader_sdk(BUILD)
        extra = target / "forged-empty-directory"
        try:
            extra.mkdir()
            with self.assertRaisesRegex(ValueError, "structure"):
                resources.build_identity(self.root, BUILD)
            with self.assertRaisesRegex(ValueError, "structure"):
                resources.check_resources(self.root, self.budget, BUILD)
        finally:
            extra.rmdir()

        executable = target / "libs/book/FreeInkBook/test/host/run.sh"
        original_mode = executable.stat().st_mode & 0o777
        try:
            for mode, accepted in ((0o645, False), (0o744, True)):
                with self.subTest(mode=oct(mode)):
                    executable.chmod(mode)
                    if accepted:
                        self.assertEqual(resources.build_identity(self.root, BUILD), self.report["identity"])
                        self.assertEqual(resources.check_resources(self.root, self.budget, BUILD), [])
                    else:
                        with self.assertRaisesRegex(ValueError, "structure"):
                            resources.build_identity(self.root, BUILD)
                        with self.assertRaisesRegex(ValueError, "structure"):
                            resources.check_resources(self.root, self.budget, BUILD)
        finally:
            executable.chmod(original_mode)

    def test_actual_c3_linker_sections_include_padding_and_separate_rtc(self):
        memory = resources.ram_measurements(ACTUAL_SIZE_OUTPUT, MEMORY_MAP)
        self.assertEqual(memory["ram_bytes"], 132400)
        self.assertEqual(memory["ram_alignment_bytes"], 12)
        self.assertEqual(memory["ram_capacity_bytes"], 321296)
        self.assertEqual(memory["rtc_bytes"], 172)
        self.assertNotIn(".dram0.dummy", memory["ram_sections"])
        self.assertEqual(memory["ram_sections"][".iram0.text_end"], 252)

    def test_missing_changed_capacity_and_unknown_sram_accounting_fail(self):
        for memory_map in ("", MEMORY_MAP.replace("0x0004e710", "0x00050000"), MEMORY_MAP.replace("0x40380000", "0x40390000")):
            with self.assertRaisesRegex(ValueError, "capacity"):
                resources.ram_measurements(SIZE_OUTPUT, memory_map)
        for size_output in (SIZE_OUTPUT + ".secret 4 1070071900\n", SIZE_OUTPUT.replace(".dram0.data 20 1070071868", ".dram0.data 20 1070071808")):
            with self.assertRaises(ValueError):
                resources.ram_measurements(size_output, MEMORY_MAP)

    def test_forged_alignment_or_rtc_accounting_fails(self):
        original = copy.deepcopy(self.report)
        self.report["ram_alignment_bytes"] = 1
        self.save()
        with self.assertRaisesRegex(ValueError, "SRAM accounting"):
            self.check()
        self.report = original
        self.report["rtc_bytes"] = 1
        self.save()
        with self.assertRaisesRegex(ValueError, "actual build outputs"):
            self.check()

    def test_exceeded_ram_flash_and_stack_are_each_reported(self):
        self.budget["resources"][BUILD].update(ram_bytes=159,
                                                    stack_bytes={"rust/firmware/native/main.cpp::void loop()": 31})
        recorded(self.budget)
        self.report = self.build(flash_bytes=97)
        self.save()
        failures = self.check()
        self.assertEqual(len(failures), 3)
        self.assertIn("ram_bytes", failures[0])
        self.assertIn("flash_bytes", failures[1])
        self.assertIn("static frame", failures[2])

    def test_missing_output_or_receipt_fails(self):
        for path in (self.directory / "firmware.bin", self.root / f"artifacts/code-health-{BUILD}.json"):
            content = path.read_bytes()
            path.unlink()
            with self.assertRaises(OSError):
                self.check()
            path.write_bytes(content)

    def test_source_generated_vendor_config_and_compiler_changes_stale_receipt(self):
        for name in ("rust/firmware/native/main.cpp", "rust/firmware/x3_existing_partitions.csv", "rust/firmware/native/generated.h", "rust/firmware/native/reader.ipp", "rust/firmware/native/firmware_version.h", "vendor/freeink-sdk/pinned.h", "platformio-rust-app.ini",
                     f".build/{BUILD}/platformio.ini",
                     ".toolchain/platformio/packages/toolchain-riscv32-esp/bin/riscv32-esp-elf-g++",
                     ".toolchain/platformio/packages/toolchain-riscv32-esp/libexec/gcc/riscv32-esp-elf/14.2.0/cc1plus"):
            path = self.root / name
            content = path.read_bytes()
            path.write_bytes(content + b"changed")
            with self.assertRaisesRegex(ValueError, "Stale"):
                self.check()
            path.write_bytes(content)
        self.assertEqual(self.check(), [])

    def test_docs_do_not_invalidate_compiled_identity(self):
        (self.root / "docs").mkdir()
        (self.root / "docs/new.md").write_text("documentation")
        self.assertEqual(self.check(), [])

    def test_authored_build_and_verification_changes_stale_receipt(self):
        for name in self.procedure_names:
            with self.subTest(procedure=name):
                path = self.root / "tools" / name
                original = path.read_bytes()
                path.write_bytes(original + b"changed procedure, unchanged compiled inputs")
                with self.assertRaisesRegex(ValueError, "Stale"):
                    self.check()
                with self.assertRaisesRegex(ValueError, "during compilation"):
                    resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0, self.report["identity"])
                path.write_bytes(original)
        self.assertEqual(self.check(), [])

    def test_sdk_patch_procedure_changes_build_identity(self):
        config = resources.configuration(self.root, BUILD)
        before = resources.build_identity(self.root, BUILD)
        path = self.root / "tools/apply-freeink-patch.py"
        path.write_text("# changed patch procedure")
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.check()
        with self.assertRaisesRegex(ValueError, "during compilation"):
            resources.build_report(self.root, BUILD, config, 0, before)

    def test_unrestricted_inline_and_extensionless_include_inputs_stale_receipt(self):
        for name in ("rust/firmware/native/inline_impl.inl", "rust/firmware/native/extensionless"):
            path = self.root / name
            original = path.read_bytes()
            path.write_bytes(original + b"changed")
            with self.assertRaisesRegex(ValueError, "Stale"):
                self.check()
            path.write_bytes(original)
        self.assertEqual(self.check(), [])

    def test_standard_header_and_runtime_archive_changes_stale_receipt(self):
        for name in ("riscv32-esp-elf/include/c++/14.2.0/vector", "riscv32-esp-elf/lib/libstdc++.a"):
            path = self.root / ".toolchain/platformio/packages/toolchain-riscv32-esp" / name
            original = path.read_bytes()
            path.write_bytes(original + b"changed, manifest unchanged")
            with self.assertRaisesRegex(ValueError, "Stale"):
                self.check()
            path.write_bytes(original)
        self.assertEqual(self.check(), [])

    def test_actual_framework_sources_headers_archives_and_recipes_stale_receipt(self):
        for name in ("packages/framework-arduinoespressif32/cores/esp32/esp32-hal-gpio.c",
                     "packages/framework-arduinoespressif32-libs/esp32c3/include/sdk.h",
                     "packages/framework-arduinoespressif32-libs/esp32c3/lib/libheap.a",
                     "packages/framework-arduinoespressif32-libs/esp32c3/ld/sections.ld",
                     "platforms/espressif32/builder/main.py"):
            path = self.root / ".toolchain/platformio" / name
            original = path.read_bytes()
            path.write_bytes(original + b"changed locally, manifest unchanged")
            with self.assertRaisesRegex(ValueError, "Stale"):
                self.check()
            path.write_bytes(original)
        self.assertEqual(self.check(), [])

    def test_build_driver_code_dependencies_caches_helpers_and_entrypoints_stale_receipt(self):
        package_manifests = {path: path.read_bytes() for path in
                             (self.root / ".toolchain/platformio/packages").glob("*/package.json")}
        for name in self.build_driver_files:
            with self.subTest(input=name):
                path = self.root / ".toolchain/platformio" / name
                original = path.read_bytes()
                path.write_bytes(original + b"changed implementation, unchanged manifests")
                with self.assertRaisesRegex(ValueError, "Stale"):
                    self.check()
                path.write_bytes(original)
        for path, original in package_manifests.items():
            self.assertEqual(path.read_bytes(), original)
        self.assertEqual(self.check(), [])

    def test_added_build_driver_cache_stales_receipt_without_changing_source_cache_policy(self):
        source_cache = self.root / "rust/firmware/native/__pycache__/ignored.cpython-312.pyc"
        source_cache.parent.mkdir()
        source_cache.write_bytes(b"noncompiled source cache")
        self.assertEqual(self.check(), [])
        driver_cache = self.root / ".toolchain/platformio/penv/lib/python3.12/site-packages/click/__pycache__/core.cpython-312.pyc"
        driver_cache.parent.mkdir()
        driver_cache.write_bytes(b"newly installed executable cached dependency")
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.check()
        driver_cache.unlink()
        self.assertEqual(self.check(), [])

    def test_python_interpreter_content_and_resolved_path_stale_receipt(self):
        original = self.interpreter.read_bytes()
        self.interpreter.write_bytes(original + b"changed Python executable")
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.check()
        self.interpreter.write_bytes(original)
        replacement = self.interpreter.with_name("python3.12-replacement")
        replacement.write_bytes(original)
        launcher = self.root / ".toolchain/platformio/penv/bin/python"
        launcher.unlink()
        launcher.symlink_to(replacement)
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.check()
        launcher.unlink()
        launcher.symlink_to(self.interpreter)
        self.assertEqual(self.check(), [])

    def test_explicit_external_interpreter_is_hashed_without_granting_dependency_ownership(self):
        with tempfile.TemporaryDirectory() as external_directory:
            external = Path(external_directory) / "python3.12"
            external.write_bytes(self.interpreter.read_bytes())
            launcher = self.root / ".toolchain/platformio/penv/bin/python"
            launcher.unlink()
            launcher.symlink_to(external)
            try:
                identity = resources.build_identity(self.root, BUILD)
                self.assertEqual(identity["python_interpreter"]["resolved_path"], home_relative(external.resolve()))
                self.assertEqual(identity["python_interpreter"]["sha256"], resources.sha256(external))
                self.report = resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0, identity)
                self.save()
                self.assertEqual(self.check(), [])
                external.write_bytes(b"changed external interpreter")
                with self.assertRaisesRegex(ValueError, "Stale"):
                    self.check()
                external.write_bytes(self.interpreter.read_bytes())
                dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
                original = dependency.read_text()
                dependency.write_text(original.rstrip() + " " + str(external) + "\n")
                self.report["artifacts"][dependency.relative_to(self.root).as_posix()] = resources.sha256(dependency)
                self.save()
                with self.assertRaisesRegex(ValueError, "Unbound or missing compiler dependency"):
                    self.check()
            finally:
                launcher.unlink()
                launcher.symlink_to(self.interpreter)

    def test_missing_build_driver_roots_entrypoints_and_interpreter_fail_closed(self):
        for name in ("platforms/espressif32/platform.json", "platforms/espressif32/platform.py",
                     "platforms/espressif32/boards/esp32-c3-devkitm-1.json",
                     "penv/lib", "packages/tool-scons", "packages/tool-esptoolpy",
                     "packages/tool-esp_install", "penv/bin/pio", "penv/bin/platformio",
                     "penv/pyvenv.cfg", "penv/bin/python", "tools/firmware-build.py"):
            with self.subTest(input=name):
                path = self.root / name if name.startswith("tools/") else self.root / ".toolchain/platformio" / name
                absent = path.with_name(path.name + "-missing")
                path.rename(absent)
                try:
                    with self.assertRaises((ValueError, OSError)):
                        self.check()
                finally:
                    absent.rename(path)
        self.assertEqual(self.check(), [])

    def test_build_driver_symlinks_cannot_escape_their_owned_roots(self):
        outside = self.root / "tools/unowned-driver.py"
        outside.parent.mkdir(exist_ok=True)
        outside.write_bytes(b"unowned executable implementation")
        for name in ("penv/lib/python3.12/site-packages/platformio/builder/main.py",
                     "packages/tool-scons/scons.py", "packages/tool-esptoolpy/esptool.py",
                     "packages/tool-esp_install/esp_install.py", "penv/bin/pio", "tools/firmware-build.py"):
            with self.subTest(input=name):
                path = self.root / name if name.startswith("tools/") else self.root / ".toolchain/platformio" / name
                original = path.read_bytes()
                path.unlink()
                path.symlink_to(outside)
                try:
                    with self.assertRaisesRegex(ValueError, "symlink|owned|escapes"):
                        self.check()
                finally:
                    path.unlink()
                    path.write_bytes(original)
        outside_directory = outside.parent / "unowned-module"
        outside_directory.mkdir()
        (outside_directory / "__init__.py").write_bytes(b"unowned installed dependency")
        for name in ("platforms/espressif32", "penv/lib", "packages/tool-scons",
                     "packages/tool-esptoolpy", "packages/tool-esp_install"):
            with self.subTest(directory=name):
                linked_directory = self.root / ".toolchain/platformio" / name / "external-module"
                linked_directory.symlink_to(outside_directory, target_is_directory=True)
                try:
                    with self.assertRaisesRegex(ValueError, "symlink|owned|escapes"):
                        resources.identity_inputs(self.root, BUILD)
                    linked_directory.unlink()
                    linked_directory.symlink_to(outside_directory / "missing", target_is_directory=True)
                    with self.assertRaisesRegex(ValueError, "symlink|owned|escapes"):
                        resources.identity_inputs(self.root, BUILD)
                finally:
                    linked_directory.unlink()
        self.assertEqual(self.check(), [])

    def test_driver_and_source_aliases_cannot_reenter_excluded_subtrees(self):
        for directory, excluded in ((".toolchain/platformio/penv/lib", ".git"),
                                    ("rust", "__pycache__")):
            with self.subTest(directory=directory):
                owned = self.root / directory
                before = resources.build_identity(self.root, BUILD)
                hidden = owned / excluded / "hidden-driver"
                hidden.mkdir(parents=True)
                hidden_file = hidden / "implementation.py"
                hidden_file.write_bytes(b"excluded executable implementation")
                self.assertEqual(resources.build_identity(self.root, BUILD), before)
                alias = owned / "visible-driver"
                for target in (hidden, hidden_file):
                    with self.subTest(target=target.name):
                        alias.symlink_to(target, target_is_directory=target.is_dir())
                        try:
                            with self.assertRaisesRegex(ValueError, "symlink|owned|escapes"):
                                resources.identity_inputs(self.root, BUILD)
                        finally:
                            alias.unlink()
                self.assertEqual(resources.build_identity(self.root, BUILD), before)

    def test_owned_directory_alias_retargeting_fails_for_drivers_and_includes(self):
        for directory in (".toolchain/platformio/penv/lib", "rust"):
            with self.subTest(directory=directory):
                owned = self.root / directory
                targets = [owned / name for name in ("implementation-a", "implementation-b")]
                for target in targets:
                    target.mkdir()
                    (target / "implementation").write_text(target.name)
                self.report = self.build()
                self.save()
                alias = owned / "active-implementation"
                for target in targets:
                    alias.symlink_to(target, target_is_directory=True)
                    try:
                        with self.assertRaisesRegex(ValueError, "directory symlink"):
                            self.check()
                    finally:
                        alias.unlink()
                self.assertEqual(self.check(), [])

    def test_input_root_directory_symlinks_are_rejected(self):
        for directory in (".toolchain/platformio/penv/lib", "rust"):
            with self.subTest(directory=directory):
                owned = self.root / directory
                target = owned.with_name(owned.name + "-real")
                owned.rename(target)
                owned.symlink_to(target, target_is_directory=True)
                try:
                    with self.assertRaisesRegex(ValueError, "directory symlink"):
                        self.check()
                finally:
                    owned.unlink()
                    target.rename(owned)
                self.assertEqual(self.check(), [])

    def test_input_root_parent_alias_outside_project_is_rejected(self):
        # Containment is checked once per canonical input root, so an alias
        # above that root must still fail when it resolves outside the project.
        for parent in ("vendor", ".toolchain"):
            with self.subTest(parent=parent), tempfile.TemporaryDirectory() as outside:
                owned = self.root / parent
                moved = Path(outside) / owned.name
                shutil.move(owned, moved)
                owned.symlink_to(moved, target_is_directory=True)
                try:
                    with self.assertRaisesRegex(ValueError, "escapes the isolated project"):
                        resources.identity_inputs(self.root, BUILD)
                finally:
                    owned.unlink()
                    shutil.move(moved, owned)
        self.assertEqual(self.check(), [])

    def test_owned_file_symlinks_remain_bound(self):
        for name in ("rust/firmware/native/generated.h", ".toolchain/platformio/penv/lib/python3.12/site-packages/click/core.py"):
            with self.subTest(input=name):
                target = self.root / name
                alias = target.with_name("file-alias")
                alias.symlink_to(target)
                original = target.read_bytes()
                try:
                    self.report = self.build()
                    self.save()
                    self.assertEqual(self.check(), [])
                    target.write_bytes(original + b"changed linked implementation")
                    with self.assertRaisesRegex(ValueError, "Stale"):
                        self.check()
                finally:
                    target.write_bytes(original)
                    alias.unlink()
                self.report = self.build()
                self.save()
                self.assertEqual(self.check(), [])

    def test_changed_authored_config_stales_report_with_unchanged_generated_config(self):
        generated = resources.configuration(self.root, BUILD)
        self.assertEqual(self.check(), [])
        (self.root / "platformio-rust-app.ini").write_text("[env:newt_x3]\n; intended config changed\n")
        self.assertEqual(resources.sha256(generated), self.report["identity"]["config_sha256"])
        with self.assertRaisesRegex(ValueError, "Stale"):
            self.check()

    def test_wrong_variant_or_artifact_path_fails(self):
        original = copy.deepcopy(self.report)
        self.report["identity"]["variant"] = "regional"
        self.save()
        with self.assertRaisesRegex(ValueError, "wrong-variant"):
            self.check()
        self.report = original
        self.report["artifacts"][".pio/regional/build/newt_x3/firmware.elf"] = self.report["artifacts"].pop(
            f".pio/{BUILD}/build/newt_x3/firmware.elf")
        self.save()
        with self.assertRaisesRegex(ValueError, "artifact identity"):
            self.check()

    def test_compiler_dependencies_cannot_escape_owned_input_roots(self):
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        original = dependency.read_text()
        external = Path(self.temp.name).parent / (Path(self.temp.name).name + "-external.h")
        external.write_text("external")
        self.addCleanup(external.unlink)
        for name in ("tools/firmware-build.py", "tools/native_helper.h", "root_helper.h", ".pio/scratch_helper.h", str(external)):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists():
                path.write_text("unbound included helper")
            dependency.write_text(original.rstrip() + " " + name + "\n")
            self.report["artifacts"][dependency.relative_to(self.root).as_posix()] = resources.sha256(dependency)
            self.save()
            with self.assertRaisesRegex(ValueError, "Unbound or missing compiler dependency"):
                self.check()
            with self.assertRaisesRegex(ValueError, "Unbound or missing compiler dependency"):
                resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0,
                                       resources.build_identity(self.root, BUILD))
        dependency.write_text(original)

    def test_mixed_vendor_dependency_cannot_bypass_composed_reader_sdk(self):
        alias = self.root / ".pio/libdeps/sdk"
        alias.parent.mkdir(parents=True, exist_ok=True)
        alias.symlink_to(self.root / "vendor/freeink-sdk", target_is_directory=True)
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        dependency.write_text(dependency.read_text().rstrip() + " .pio/libdeps/sdk/pinned.h\n")
        self.report["artifacts"][dependency.relative_to(self.root).as_posix()] = resources.sha256(dependency)
        self.save()
        with self.assertRaisesRegex(ValueError, "Pinned SDK dependency bypasses"):
            self.check()

    def test_dependency_proof_requires_all_eight_sdk_library_routes(self):
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        route = (Path(resources.reader_sdk(BUILD)) / resources.SDK_LIBRARIES[-1] / "route.h").as_posix()
        dependency.write_text(dependency.read_text().replace(" " + route, ""))
        with self.assertRaisesRegex(ValueError,
                                    r"Missing actual compiler dependency routes.*libs/book/FreeInkBook"):
            self.dependency_routes()

    def test_dependency_proof_rejects_another_lane_with_specific_oracle(self):
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        library = resources.SDK_LIBRARIES[0]
        expected = (Path(resources.reader_sdk(BUILD)) / library / "route.h").as_posix()
        # A retired variant's stale SDK copy can survive in an old checkout.
        other = Path(".build/freeink-reader-sdk-default") / library / "route.h"
        (self.root / other).parent.mkdir(parents=True)
        (self.root / other).write_text("// retired lane\n")
        dependency.write_text(dependency.read_text().replace(expected, other.as_posix()))
        with self.assertRaisesRegex(ValueError, "Compiler dependency uses another reader SDK lane"):
            self.dependency_routes()

    def test_dependency_proof_rejects_mixed_pinned_route_with_specific_oracle(self):
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        dependency.write_text(dependency.read_text().rstrip() + " vendor/freeink-sdk/pinned.h\n")
        with self.assertRaisesRegex(ValueError, "Pinned SDK dependency bypasses composed reader copy"):
            self.dependency_routes()

    def test_dependency_proof_rejects_undeclared_sdk_subtree_with_specific_oracle(self):
        undeclared = self.root / resources.reader_sdk(BUILD) / "libs/network/Undeclared/route.h"
        undeclared.parent.mkdir(parents=True)
        undeclared.write_text("// outside the eight compiled SDK routes\n")
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        dependency.write_text(dependency.read_text().rstrip() + " " +
                              undeclared.relative_to(self.root).as_posix() + "\n")
        with self.assertRaisesRegex(ValueError, "Compiler dependency uses an undeclared SDK library route"):
            self.dependency_routes()

    def test_input_symlink_escape_is_rejected(self):
        helper = self.root / "tools/helper.h"
        helper.parent.mkdir(exist_ok=True)
        helper.write_text("unowned")
        (self.root / "rust/firmware/native/link.h").symlink_to(helper)
        with self.assertRaisesRegex(ValueError, "symlink escapes"):
            self.check()

    def test_dependency_make_escaping_continuations_and_missing_proof(self):
        header = self.root / "rust/firmware/native/space #cash$ colon:.h"
        header.write_text("owned")
        escaped = str(header.relative_to(self.root)).replace(" ", "\\ ").replace("#", "\\#").replace("$", "$$").replace(":", "\\:")
        dependency = self.directory / "src/rust/firmware/native/main.cpp.d"
        target = dependency.with_suffix(".o").relative_to(self.root).as_posix()
        sdk_routes = {(self.root / resources.reader_sdk(BUILD) / library / "route.h").resolve()
                      for library in resources.SDK_LIBRARIES}
        route_text = " ".join(path.relative_to(self.root).as_posix() for path in sorted(sdk_routes))
        dependency.write_text(f"{target}: rust/firmware/native/main.cpp {route_text} \\\n {escaped}\n{escaped}:\n")
        self.assertEqual(resources.dependency_files(self.root, dependency),
                         {header.resolve(), (self.root / "rust/firmware/native/main.cpp").resolve(), *sdk_routes})
        self.report["identity"] = resources.build_identity(self.root, BUILD)
        self.report["artifacts"][dependency.relative_to(self.root).as_posix()] = resources.sha256(dependency)
        self.save()
        self.assertEqual(self.check(), [])
        dependency.unlink()
        with self.assertRaises(OSError):
            self.check()

    def test_unbound_linker_input_is_rejected(self):
        archive = self.root / "tools/external.a"
        archive.parent.mkdir(exist_ok=True)
        archive.write_bytes(b"unowned archive")
        memory_map = self.directory / "firmware.map"
        memory_map.write_text(memory_map.read_text() + "LOAD tools/external.a\n")
        self.report["artifacts"][memory_map.relative_to(self.root).as_posix()] = resources.sha256(memory_map)
        self.save()
        with self.assertRaisesRegex(ValueError, "Unbound or missing linker input"):
            self.check()

    def test_replaced_output_and_forged_measurements_fail(self):
        path = self.directory / "firmware.elf"
        path.write_bytes(b"different")
        with self.assertRaisesRegex(ValueError, "Changed build artifact"):
            self.check()
        path.write_bytes(b"elf")
        self.report["flash_bytes"] = 1
        self.save()
        with self.assertRaisesRegex(ValueError, "actual build outputs"):
            self.check()

    def test_stale_outputs_cannot_issue_receipt(self):
        identity = resources.build_identity(self.root, BUILD)
        path = self.directory / "src/rust/firmware/native/main.cpp.su"
        os.utime(path, ns=(1, 1))
        with self.assertRaisesRegex(ValueError, "stale build output"):
            resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 2, identity)

    def test_changed_inputs_during_build_fail(self):
        identity = resources.build_identity(self.root, BUILD)
        (self.root / "rust/firmware/native/main.cpp").write_text("changed")
        with self.assertRaisesRegex(ValueError, "during compilation"):
            resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0, identity)

    def test_missing_selected_stack_fails(self):
        self.budget["resources"][BUILD]["stack_bytes"]["rust/firmware/native/main.cpp::void missing()"] = 32
        recorded(self.budget)
        with self.assertRaisesRegex(ValueError, "Missing selected stack"):
            self.check()

    def test_unbounded_selected_frame_fails_but_dynamic_bounded_passes(self):
        path = self.directory / "src/rust/firmware/native/main.cpp.su"
        for qualifier, count in (("dynamic", 1), ("dynamic,bounded", 0)):
            path.write_text(STACK.replace("static", qualifier))
            self.report["artifacts"][path.relative_to(self.root).as_posix()] = resources.sha256(path)
            self.report["stack_bytes"]["rust/firmware/native/main.cpp::void loop()"]["kind"] = qualifier
            self.save()
            self.assertEqual(len(self.check()), count)

    def test_invalid_measurements_and_absolute_budgets_fail(self):
        original = copy.deepcopy(self.report)
        for value in (-1, True, "96", 1.5):
            self.report["flash_bytes"] = value
            self.save()
            with self.assertRaisesRegex(ValueError, "Invalid resource measurement"):
                self.check()
        self.report = original
        self.save()
        for field, value in (("ram_bytes", resources.RAM_CAPACITY + 1), ("flash_bytes", resources.APP_PARTITION_CAPACITY + 1), ("ram_bytes", True)):
            budget = self.budget["resources"][BUILD]
            old = budget[field]
            budget[field] = value
            with self.assertRaisesRegex(ValueError, "Invalid absolute budget"):
                self.check()
            budget[field] = old

    def test_malformed_stack_size_sections_and_partition_fail(self):
        path = self.directory / "src/rust/firmware/native/main.cpp.su"
        for text in (STACK.replace("32", "-1"), STACK.replace("static", "unknown"), "broken"):
            path.write_text(text)
            with self.assertRaises(ValueError):
                resources.stack_frames(self.root, [path])
        for text in (SIZE_OUTPUT.replace(".dram0.data 20", ".dram0.data -20"), SIZE_OUTPUT.replace(".dram0.bss", ".dram0.secret"), ".dram0.data 20 0\n"):
            with self.assertRaises(ValueError):
                sram.ram_sections(text)
        for table in (b"", b"\x00" * 32, b"\xff" * 32):
            with self.assertRaises(ValueError):
                resources.partition_capacity(table)

    def test_only_the_one_build_has_paths(self):
        self.assertEqual(resources.build_directory(self.root, BUILD), self.root / f".pio/{BUILD}/build/newt_x3")
        for retired in ("default", "dev-usb", "regional", "other"):
            with self.assertRaisesRegex(ValueError, "Unknown"):
                resources.build_directory(self.root, retired)

    def test_budgets_validate_without_a_build(self):
        resources.validate_budgets(self.budget)
        original = copy.deepcopy(self.budget)
        self.budget["resources"][BUILD]["ram_bytes"] = -1
        with self.assertRaises(ValueError):
            resources.validate_budgets(self.budget)
        self.budget = original
        for key in ("/outside-project/file.cpp::void loop()", "firmware/../file.cpp::void loop()", "rust/firmware/native/main.cpp::", "main.cpp::void loop()", "::void loop()"):
            self.budget["resources"][BUILD]["stack_bytes"] = {key: 32}
            with self.assertRaises(ValueError):
                resources.validate_budgets(self.budget)
        self.budget["resources"]["regional"] = copy.deepcopy(self.budget["resources"][BUILD])
        with self.assertRaisesRegex(ValueError, "exactly the one firmware build"):
            resources.validate_budgets(self.budget)
        del self.budget["resources"]["regional"], self.budget["resources"][BUILD]
        with self.assertRaisesRegex(ValueError, "exactly the one firmware build"):
            resources.validate_budgets(self.budget)



if __name__ == "__main__":
    unittest.main()
