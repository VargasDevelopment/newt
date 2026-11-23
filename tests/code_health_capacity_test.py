"""Flash capacity comes from the complete, digest-verified generated X3 table.

A ceiling or floor moves only with a fresh record that runs from its value at
the base commit, and a floor also names the owner's approval."""
import contextlib
import copy
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from resource_fixture import (BUILD, SIZE_OUTPUT, STACK, ResourceFixture, X3_PARTITIONS, partition_table, record,
                              recorded)
import code_health as health
import code_health_resources as resources
import code_health_flash_policy as flash_rules


LOOP = "rust/firmware/native/main.cpp::void loop()"


class CapacityTests(ResourceFixture):
    def test_capacity_comes_from_generated_app_slots(self):
        table = (self.directory / "partitions.bin").read_bytes()
        self.assertEqual(resources.partition_capacity(table), 0x770000)
        self.assertEqual(self.report["flash_capacity_bytes"], resources.partition_capacity(table))
        self.assertNotIn("reference_flash_capacity_bytes", self.report)
        # The absolute profile budget guard is not measurement evidence.
        with patch.object(resources, "APP_PARTITION_CAPACITY", 0x780000):
            measured = resources.measure_outputs(self.root, BUILD, resources.artifact_paths(self.root, BUILD))
            self.assertEqual(measured["flash_capacity_bytes"], 0x770000)
        self.grow(size=7016448)
        self.assertEqual(self.check(), [])

    def test_smaller_sdk_table_cannot_use_forged_carrier_capacity(self):
        # A valid ESP digest does not authorize the SDK's smaller factory layout.
        stock = partition_table(((0, 0, 0x10000, 1310720, b"factory", 0),))
        path = self.directory / "partitions.bin"
        path.write_bytes(stock)
        with self.assertRaises(ValueError):
            resources.build_report(self.root, BUILD, resources.configuration(self.root, BUILD), 0,
                                   resources.build_identity(self.root, BUILD))
        # A receipt editor who rebinds the changed artifact still cannot claim
        # the real X3 carrier size over these different generated partitions.
        self.report["artifacts"][path.relative_to(self.root).as_posix()] = resources.sha256(path)
        self.report["flash_capacity_bytes"] = 0x770000
        self.save()
        with self.assertRaises(ValueError):
            self.check()

    def test_same_capacity_wrong_layout_is_rejected_with_valid_md5(self):
        mutations = (
            ("offset", 3, 2, 0x790000),
            ("type", 0, 0, 0),
            ("subtype", 3, 1, 16),
            ("label", 3, 4, b"factory"),
            ("flags", 3, 5, 1),
            ("smaller app slot", 3, 3, 0x760000),
        )
        for label, index, field, value in mutations:
            with self.subTest(mutation=label):
                entries = list(X3_PARTITIONS)
                entry = list(entries[index])
                entry[field] = value
                entries[index] = tuple(entry)
                with self.assertRaises(ValueError):
                    resources.partition_capacity(partition_table(entries))
        for entries in (X3_PARTITIONS[:-1], X3_PARTITIONS + (X3_PARTITIONS[-1],),
                        (X3_PARTITIONS[1], X3_PARTITIONS[0], *X3_PARTITIONS[2:])):
            with self.subTest(entries=entries):
                with self.assertRaises(ValueError):
                    resources.partition_capacity(partition_table(entries))

    def test_md5_and_padding_are_verified_not_just_app_capacities(self):
        original = partition_table()
        marker = 32 * len(X3_PARTITIONS)
        corruptions = {
            "missing MD5": original[:marker] + b"\xff" * (len(original) - marker),
            "MD5 digest": original[:marker + 16] + bytes([original[marker + 16] ^ 1]) + original[marker + 17:],
            "MD5 prefix": original[:marker + 2] + b"\x00" + original[marker + 3:],
            "undigested entry": original[:12] + bytes([original[12] ^ 1]) + original[13:],
            "padding": original[:-1] + b"\x00",
        }
        for label, table in corruptions.items():
            with self.subTest(mutation=label):
                with self.assertRaises(ValueError):
                    resources.partition_capacity(table)

    def test_forged_capacity_receipt_and_obsolete_reference_field_fail(self):
        original = copy.deepcopy(self.report)
        self.report["flash_capacity_bytes"] += 0x1000
        self.save()
        with self.assertRaisesRegex(ValueError, "actual build outputs"):
            self.check()
        self.report = original
        self.report["reference_flash_capacity_bytes"] = 1310720
        self.save()
        with self.assertRaisesRegex(ValueError, "report schema"):
            self.check()

    def grow(self, size=112):
        """Build a bigger image and set the flash ceiling to its measurement."""
        self.report = self.build(BUILD, size)
        self.save()
        self.budget["resources"][BUILD]["flash_bytes"] = size
        recorded(self.budget)

    def test_actual_image_reserve_boundary_and_oversized_reviewed_budget(self):
        self.assertEqual(resources.usable_flash(7798784), 7016448)
        self.grow(size=7016448)
        self.assertEqual(self.check(), [])
        self.report = self.build(flash_bytes=7016449)
        self.save()
        failures = self.check()
        self.assertTrue(any("generated partition usable limit 7016448" in failure for failure in failures))
        self.budget["resources"][BUILD]["flash_bytes"] = 7798784
        with self.assertRaisesRegex(ValueError, "Invalid absolute budget"):
            self.check()

    def test_ceiling_edit_without_its_record_fails_and_writes_nothing(self):
        path = self.root / health.BASELINE
        path.write_text(json.dumps(self.budget))
        original = path.read_bytes()
        receipt = (self.root / f"artifacts/code-health-{BUILD}.json").read_bytes()
        self.budget["resources"][BUILD]["flash_bytes"] += 16
        with self.assertRaisesRegex(ValueError, "flash_bytes: its record ends at 96, not the current 112"):
            self.check()
        self.assertEqual(path.read_bytes(), original)
        self.assertEqual((self.root / f"artifacts/code-health-{BUILD}.json").read_bytes(), receipt)

    def commit_base(self):
        """Commit the current baseline as the adoption BASE and return it."""
        (self.root / health.BASELINE).write_text(json.dumps(self.budget))
        git = ["git", "-C", str(self.root), "-c", "user.name=fixture", "-c", "user.email=fixture@localhost"]
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(git + ["add", health.BASELINE], check=True)
        subprocess.run(git + ["commit", "-q", "-m", "base"], check=True)
        return copy.deepcopy(self.budget)

    def adopt_cli(self):
        """Run the real entry point with --adopt against the committed BASE."""
        committed = health.read_baseline  # BASE from git; the fixture candidate has no structural schema.
        with (patch.object(health, "ROOT", self.root), patch.object(health, "verify_existing_inputs"),
              patch.object(health, "length_warnings", return_value=[]),
              patch.object(health, "base_failures", return_value=[]),
              patch.object(health, "read_baseline", lambda root, revision=None:
                            committed(root, revision) if revision else self.budget),
              patch.object(sys, "argv", ["code_health.py", "--variant", BUILD, "--adopt", "HEAD"]),
              contextlib.redirect_stdout(io.StringIO())):
            health.main()

    def test_adoption_binds_raised_and_added_ceilings_to_the_fresh_measurement(self):
        base = self.commit_base()
        self.command.return_value.stdout = SIZE_OUTPUT.replace(".dram0.bss 80", ".dram0.bss 96")
        added = "rust/firmware/native/nvs_guard.cpp::void guard()"
        with patch("resource_fixture.STACK", STACK.replace("\t32\t", "\t48\t")):
            self.report = self.build(flash_bytes=112)
        self.save()
        self.budget["resources"][BUILD].update(flash_bytes=112, ram_bytes=176)
        self.budget["resources"][BUILD]["stack_bytes"].update({LOOP: 48, added: 16})
        recorded(self.budget)
        adopt = lambda: resources.check_resources(self.root, self.budget, BUILD, base)
        self.assertEqual(adopt(), [])
        self.adopt_cli()
        exact = copy.deepcopy(self.budget)
        for field in ("flash_bytes", "ram_bytes", LOOP, added):
            self.budget = copy.deepcopy(exact)
            budgets = self.budget["resources"][BUILD]
            target = budgets["stack_bytes"] if "::" in field else budgets
            target[field] += 16
            recorded(self.budget)
            # Ordinary checks cannot know the measurement a record was made from.
            self.assertEqual(self.check(), [])
            with self.subTest(field=field):
                self.assertTrue(any(f"{BUILD} {field}: raised ceiling" in failure for failure in adopt()))
        with self.assertRaisesRegex(ValueError, "raised ceiling 32 is not the adopted measurement 16"):
            self.adopt_cli()


def baseline():
    return recorded({"version": 2, "resources": {BUILD: {"flash_bytes": 100, "ram_bytes": 200, "wifi_lent_bytes": 40,
                                                         "stack_bytes": {LOOP: 32}}}})


class BaseCommitTests(unittest.TestCase):
    """Each moved limit against the base commit's baseline."""

    def changes(self, head, base=None):
        resources.validate_budgets(head)
        return flash_rules.changes(base or baseline(), head)

    def moved(self, name, value, **fields):
        head = baseline()
        if name in head["floors"]:
            head["floors"][name] = value
        elif "::" in name:
            head["resources"][BUILD]["stack_bytes"][name] = value
        else:
            head["resources"][BUILD][name] = value
        head["records"][name] = record(baseline()["records"].get(name, {}).get("after"), value, **fields)
        return head

    def test_a_raise_or_a_lowering_with_its_fresh_record_passes(self):
        for name, value in (("flash_bytes", 112), ("ram_bytes", 216), ("wifi_lent_bytes", 48), (LOOP, 48),
                            ("flash_bytes", 96), (LOOP, 16)):
            with self.subTest(name=name, value=value):
                self.assertEqual(self.changes(self.moved(name, value)), [])

    def test_a_raise_without_its_record_fails(self):
        head = baseline()
        head["resources"][BUILD]["flash_bytes"] = 112
        with self.assertRaisesRegex(ValueError, "flash_bytes: its record ends at 100, not the current 112"):
            self.changes(head)

    def test_a_record_that_does_not_start_at_the_base_value_fails(self):
        for before in (96, 112, None):
            head = self.moved("flash_bytes", 112)
            head["records"]["flash_bytes"]["before"] = before
            with self.subTest(before=before):
                self.assertEqual(self.changes(head),
                                 ["flash_bytes: changed from 100 to 112, so its record must run from 100"])

    def test_a_floor_moves_only_with_the_owners_approval(self):
        for name, value in (("reading_heap_min_free_bytes", 60000), ("loop_stack_min_free_bytes", 4096)):
            with self.subTest(name=name):
                self.assertEqual(self.changes(self.moved(name, value)),
                                 [f"{name}: a floor moves only with a record naming the owner's approval"])
                approved = self.moved(name, value, owner_approval="2026-11-21: the owner approved this floor")
                self.assertEqual(self.changes(approved), [])

    def test_a_rewritten_record_fails(self):
        for field, value in (("reason", "Rewritten review"), ("reviewer", "Someone else"), ("before", 7),
                             ("source_revision", "c" * 40), ("owner_approval", "Added later")):
            head = baseline()
            head["records"][LOOP][field] = value
            with self.subTest(field=field):
                self.assertEqual(self.changes(head), [f"{LOOP}: its record changed but its value did not"])

    def test_an_added_ceiling_needs_a_record_from_nothing_and_a_removed_one_fails(self):
        added = "rust/firmware/native/entry.cpp::void setup()"
        self.assertEqual(self.changes(self.moved(added, 16)), [])
        head = self.moved(added, 16)
        head["records"][added]["before"] = 16
        self.assertEqual(self.changes(head), [f"{added}: changed from None to 16, so its record must run from None"])
        head = baseline()
        head["resources"][BUILD].pop("wifi_lent_bytes")
        head["records"].pop("wifi_lent_bytes")
        self.assertEqual(self.changes(head), ["wifi_lent_bytes: a limit cannot be removed"])

    def test_a_pre_record_base_accepts_only_the_unchanged_migration(self):
        setup, resize = "void setup()", "void* resize()"
        head = baseline()
        head["resources"][BUILD]["stack_bytes"].update({f"rust/firmware/native/entry.cpp::{setup}": 16,
                                                        f"rust/target-expat/native/xml_budget.cpp::{resize}": 48})
        recorded(head)
        # The pre-NEWT-69 shape: old stack names and floors behind an anchor.
        base = {"version": 1, "flash_policy": {"floors": {"anchor": copy.deepcopy(head["floors"]), "updates": []}},
                "resources": copy.deepcopy(head["resources"])}
        base["resources"][BUILD]["stack_bytes"] = {LOOP: 32, f"firmware/main.cpp::{setup}": 16,
                                                   f"src/xml_budget.cpp::{resize}": 48}
        self.assertEqual(self.changes(head, base), [])
        # Codex's case on #385: a branch built on the old format cherry-picks the
        # migration and moves a floor with a self-consistent record.
        moved = copy.deepcopy(head)
        moved["floors"]["reading_heap_min_free_bytes"] = 1
        moved["records"]["reading_heap_min_free_bytes"].update(before=1, after=1)
        raised = copy.deepcopy(head)
        raised["resources"][BUILD]["flash_bytes"] = 112
        raised["records"]["flash_bytes"] = record(100, 112)
        for label, changed in (("floor", moved), ("ceiling", raised)):
            with self.subTest(label):
                self.assertEqual(self.changes(changed, base), [
                    "Limits differ from a pre-record base; update your branch onto main before changing limits"])


class BaseCommitResolutionTests(unittest.TestCase):
    """The base is origin/main's merge base, or HEAD's first parent on main itself."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="newt-health-base-")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.git("init", "-q")
        self.base = baseline()
        self.commit(self.base)

    def git(self, *arguments):
        return subprocess.run(["git", "-C", str(self.root), "-c", "user.name=fixture",
                               "-c", "user.email=fixture@localhost", *arguments],
                              check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, value):
        (self.root / health.BASELINE).write_text(json.dumps(value))
        self.git("add", health.BASELINE)
        self.git("commit", "-q", "-m", "change")
        return self.git("rev-parse", "HEAD")

    def test_branch_and_main_push_compare_with_the_commit_they_build_on(self):
        main = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", main)
        head = copy.deepcopy(self.base)
        head["resources"][BUILD]["ram_bytes"] = 216
        head["records"]["ram_bytes"] = record(196, 216)
        expected = ["ram_bytes: changed from 200 to 216, so its record must run from 200"]
        candidate = self.commit(head)
        self.assertEqual(health.base_failures(self.root, head), expected)
        # On main itself the merge base is HEAD; the first parent is the base.
        self.git("update-ref", "refs/remotes/origin/main", candidate)
        self.assertEqual(health.base_failures(self.root, head), expected)
        head["records"]["ram_bytes"]["before"] = 200
        self.assertEqual(health.base_failures(self.root, head), [])

    def test_a_missing_base_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "cannot find it.*git fetch origin"):
            health.base_failures(self.root, self.base)


if __name__ == "__main__":
    unittest.main()
