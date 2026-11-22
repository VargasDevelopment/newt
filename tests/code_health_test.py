"""Regression fixtures for the read-only tool/test line ratchet."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import code_health as health
from resource_fixture import BUILD, ResourceFixture, recorded
import code_health_resources as resources


def baseline():
    return recorded({"version": 2, "policy": copy.deepcopy(health.POLICY), "large_files": {},
                     "resources": {BUILD: {"ram_bytes": 200000, "flash_bytes": 1300000,
                                           "stack_bytes": {"rust/firmware/native/entry.cpp::void loop()": 512}}}})


class RatchetTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="newt-health-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.write(".gitignore", ".build/\n.pio/\n.toolchain/\n")
        self.base = baseline()
        self.write(health.BASELINE, json.dumps(self.base))

    def write(self, path, text):
        destination = self.root / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text)
        return destination

    def check(self):
        return health.check_structure(self.root, health.read_baseline(self.root))

    def test_growth_and_new_untracked_oversized_file_fail_without_baseline_writes(self):
        self.base["large_files"] = {"tools/large.py": 510}
        self.write(health.BASELINE, json.dumps(self.base))
        original = (self.root / health.BASELINE).read_bytes()
        self.write("tools/large.py", "x=1\n" * 510)
        self.assertEqual(self.check()[1], [])
        self.write("tools/large.py", "x=1\n" * 511)
        self.assertIn("exceeds reviewed ceiling 510", self.check()[1][0])
        self.write("tools/large.py", "x=1\n" * 499)
        self.assertEqual(self.check()[1], [])
        self.write("tools/replacement.py", "x=1\n" * 501)
        self.assertIn("tools/replacement.py", self.check()[1][0])
        self.assertEqual((self.root / health.BASELINE).read_bytes(), original)

    def test_repeated_baseline_key_fails_instead_of_keeping_the_last_value(self):
        text = json.dumps(self.base)
        self.assertTrue(text.startswith('{"version": 2, '))
        self.write(health.BASELINE, text.replace('{"version": 2, ', '{"version": 2, "version": 2, ', 1))
        with self.assertRaisesRegex(ValueError, "Duplicate key"):
            health.read_baseline(self.root)

    def test_adoption_base_is_a_committed_baseline_never_an_option(self):
        subprocess.run(["git", "-C", str(self.root), "add", health.BASELINE], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=fixture", "-c", "user.email=fixture@localhost",
                        "commit", "-q", "-m", "base"], check=True)
        self.assertEqual(health.read_baseline(self.root, "HEAD"), self.base)
        with self.assertRaisesRegex(ValueError, "not an option"):
            health.read_baseline(self.root, "--output=/dev/null")

    def test_deleted_tracked_file_passes_and_new_offender_cannot_replace_it(self):
        path = self.write("tools/large.py", "x=1\n" * 501)
        self.base["large_files"] = {"tools/large.py": 501}
        self.write(health.BASELINE, json.dumps(self.base))
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        path.unlink()
        self.assertEqual(self.check()[1], [])
        self.write("tools/other.py", "x=1\n" * 501)
        self.assertTrue(self.check()[1])

    def test_vendor_and_build_exclusions_do_not_hide_new_authored_code(self):
        self.write("vendor/sdk/main.py", "x=1\n" * 1000)
        self.write(".build/main.py", "x=1\n" * 1000)
        self.assertEqual(self.check()[1], [])
        self.write("tools/generated/main.py", "x=1\n" * 501)
        self.assertIn("tools/generated/main.py", self.check()[1][0])

    def test_categories_use_distinct_ceilings(self):
        self.write("tests/test.cpp", "int x;\n" * 700)
        self.write("tools/probe.py", "x=1\n" * 500)
        self.assertEqual(self.check()[1], [])
        self.write("tests/test.cpp", "int x;\n" * 701)
        self.write("tools/probe.py", "x=1\n" * 501)
        self.assertEqual(len(self.check()[1]), 2)

    def test_symlink_cannot_hide_authored_code(self):
        self.write("tests/helper.py", "x=1\n")
        (self.root / "tools").mkdir()
        (self.root / "tools/helper.py").symlink_to("../tests/helper.py")
        with self.assertRaisesRegex(ValueError, "regular in-repository"):
            self.check()

    def test_baseline_schema_policy_and_ceiling_drift_fail_closed(self):
        mutations = [lambda b: b.update(version=99), lambda b: b.update(unexpected=True),
                     lambda b: b["policy"]["line_thresholds"].update(tool=600),
                     lambda b: b["large_files"].update({"tools/ui.py": True}),
                     lambda b: b["large_files"].update({"tools/ui.py": 499}),
                     lambda b: b["resources"].pop(BUILD),
                     # A retired variant has no live ceilings.
                     lambda b: b["resources"].update(regional=copy.deepcopy(b["resources"][BUILD])),
                     # A selected key under a path outside rust/ (#298).
                     lambda b: b["resources"][BUILD]["stack_bytes"].update({"firmware/main.cpp::void loop()": 1}),
                     lambda b: b["records"].pop("ram_bytes"),
                     lambda b: b["records"]["ram_bytes"].update(after=1),
                     lambda b: b["records"]["ram_bytes"].update(reviewer=" "),
                     lambda b: b["records"]["ram_bytes"].update(owner_approval=""),
                     lambda b: b["records"]["ram_bytes"].update(evidence={}),
                     lambda b: b["floors"].update(reading_heap_min_free_bytes=-1),
                     lambda b: b["floors"].pop("loop_stack_min_free_bytes")]
        for mutate in mutations:
            value = baseline()
            mutate(value)
            self.write(health.BASELINE, json.dumps(value))
            with self.assertRaises(ValueError):
                health.read_baseline(self.root)

    def test_deliberate_tightening_prevents_debt_reintroduction(self):
        self.base["large_files"] = {"tools/large.py": 520}
        self.write(health.BASELINE, json.dumps(self.base))
        self.write("tools/large.py", "x=1\n" * 510)
        self.assertEqual(self.check()[1], [])
        self.base["large_files"]["tools/large.py"] = 510
        self.write(health.BASELINE, json.dumps(self.base))
        self.write("tools/large.py", "x=1\n" * 511)
        self.assertTrue(self.check()[1])


class SharedDataInputTests(ResourceFixture):
    def test_rust_tree_and_shared_notes_are_owned_inputs(self):
        notes = self.root / resources.SHARED_DATA_INPUT
        notes.parent.mkdir(parents=True)
        notes.write_bytes(b"shared readonly notes")
        unrelated = notes.with_name("uncompiled.rs")
        unrelated.write_bytes(b"owned Rust input")
        inputs = resources.identity_inputs(self.root, BUILD)["inputs_sha256"]
        self.assertIn(notes, inputs)
        self.assertIn(unrelated, inputs)
        before = resources.tree_hash(self.root, inputs)
        unrelated.write_bytes(b"changed Rust input")
        self.assertNotEqual(before, resources.tree_hash(self.root, inputs))
        notes.unlink()
        notes.symlink_to(unrelated)
        with self.assertRaisesRegex(ValueError, "owned ordinary"):
            resources.identity_inputs(self.root, BUILD)

    def test_only_the_named_notes_generator_leaf_is_bound_as_an_output(self):
        depfile = (self.root / resources.ARCHIVE).with_suffix(".d")
        depfile.parent.mkdir(parents=True, exist_ok=True)
        prefix = ".build/rust-app/target/riscv32imc-unknown-none-elf/release/build/"
        for name, accepted in (("newt-device-ui-" + "a" * 16 + "/out/whats_new.rs", True),
                               ("newt-device-ui-" + "a" * 16 + "/out/other.rs", False),
                               ("unowned-" + "a" * 16 + "/out/whats_new.rs", False)):
            depfile.write_text(resources.ARCHIVE + ": rust/device-ui/build.rs " + prefix + name + "\n")
            if accepted:
                self.assertEqual(resources.generated_inputs(self.root), [self.root / prefix / name])
            else:
                with self.assertRaisesRegex(ValueError, "Unbound generated"):
                    resources.generated_inputs(self.root)


if __name__ == "__main__":
    unittest.main()
