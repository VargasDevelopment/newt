"""Restart/failure tests for the local delivery simulation; no device I/O."""
from dataclasses import asdict
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from newt_delivery import DeliveryError, InjectedFailure, fixture
from newt_package import encode_app


def package(version="1.0.0", note="Pause."):
    return encode_app({"id": "pocket-pause", "version": version,
                       "title": "Pocket pause", "subtitle": "", "note": note,
                       "cards": [{"title": "A quiet moment", "detail": "A small reminder.",
                                  "action": "show-note"}]})


class DeliveryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fixture = fixture(self.root)
        self.device = self.fixture.device("x3-dev")
        self.first = package()
        self.second = package("1.0.1", "Take a breath.")

    def queue(self, payload, alias="x3-dev"):
        digest = self.fixture.artifacts.add(payload)
        return self.fixture.control.queue(alias, digest)

    def deliver(self, **kwargs):
        return self.device.deliver(self.fixture.artifacts, self.fixture.control, **kwargs)

    def baseline(self):
        desired = self.queue(self.first)
        self.assertEqual(self.deliver().status, "installed")
        return desired, self.device.installed()

    def test_sharing_is_separate_from_recipient_intent(self):
        digest = self.fixture.artifacts.add(self.first)
        self.assertEqual(self.fixture.artifacts.get(digest), self.first)
        self.assertIsNone(self.fixture.control.desired("x3-dev"))
        self.assertIsNone(self.device.installed())
        self.assertEqual(asdict(self.deliver()), {"status": "idle", "revision": None,
                                                "digest": None})

    def test_queue_idempotence_and_recipient_isolation(self):
        desired = self.queue(self.first)
        self.assertEqual(self.queue(self.first), desired)
        self.queue(self.second, "x3-spare")
        self.assertIsNone(self.fixture.device("x3-spare").installed())
        self.assertEqual(self.deliver().status, "installed")
        self.assertEqual(self.device.installed().payload, self.first)
        self.assertTrue(self.fixture.control.acknowledged("x3-dev"))
        self.assertFalse(self.fixture.control.acknowledged("x3-spare"))
        self.assertEqual(self.deliver().status, "already_installed")
        self.assertEqual(self.queue(self.first), desired)
        self.assertTrue(self.fixture.control.acknowledged("x3-dev"))

    def test_failure_before_commit_preserves_old_state_and_restart_retries(self):
        for stage in ("download", "validation", "staging", "before_commit"):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as directory:
                simulation = fixture(Path(directory))
                device = simulation.device("x3-dev")
                simulation.control.queue("x3-dev", simulation.artifacts.add(self.first))
                device.deliver(simulation.artifacts, simulation.control)
                old = device.installed()
                simulation.control.queue("x3-dev", simulation.artifacts.add(self.second))
                with self.assertRaises(InjectedFailure):
                    device.deliver(simulation.artifacts, simulation.control, stage)
                restarted = fixture(Path(directory))
                self.assertEqual(restarted.device("x3-dev").installed(), old)
                self.assertFalse(restarted.control.acknowledged("x3-dev"))
                result = restarted.device("x3-dev").deliver(restarted.artifacts,
                                                            restarted.control)
                self.assertEqual(result.status, "installed")
                self.assertEqual(restarted.device("x3-dev").installed().payload, self.second)

    def test_committed_install_survives_lost_acknowledgement_without_duplicate(self):
        self.baseline()
        desired = self.queue(self.second)
        with self.assertRaises(InjectedFailure):
            self.deliver(failure_stage="after_commit")
        restarted = fixture(self.root)
        installed_path = self.root / "devices/x3-dev/installed.json"
        committed_stat = installed_path.stat()
        self.assertEqual(restarted.device("x3-dev").installed().payload, self.second)
        self.assertFalse(restarted.control.acknowledged("x3-dev"))
        result = restarted.device("x3-dev").deliver(restarted.artifacts, restarted.control)
        self.assertEqual(result.status, "already_installed")
        self.assertEqual(installed_path.stat().st_mtime_ns, committed_stat.st_mtime_ns)
        self.assertEqual(restarted.device("x3-dev").installed().revision, desired.revision)
        self.assertTrue(restarted.control.acknowledged("x3-dev"))

    def test_newer_desired_state_supersedes_download_before_commit(self):
        _, old = self.baseline()
        desired = self.queue(self.second)
        third = package("1.0.2", "A quiet minute.")
        latest = []

        def supersede(stage):
            if stage == "before_commit":
                latest.append(self.queue(third))

        self.assertEqual(self.deliver(hook=supersede).status, "superseded")
        self.assertEqual(self.device.installed(), old)
        self.assertGreater(latest[0].revision, desired.revision)
        self.assertFalse(self.fixture.control.acknowledged("x3-dev"))
        self.assertEqual(self.deliver().status, "installed")
        self.assertEqual(self.device.installed().payload, third)

    def test_first_install_interruption_leaves_no_partial_installation(self):
        self.queue(self.first)
        with self.assertRaises(InjectedFailure):
            self.deliver(failure_stage="staging")
        restarted = fixture(self.root)
        self.assertIsNone(restarted.device("x3-dev").installed())
        self.assertEqual(restarted.device("x3-dev").deliver(
            restarted.artifacts, restarted.control).status, "installed")

    def test_new_queue_after_commit_cannot_be_acknowledged_by_old_install(self):
        self.baseline()
        second_desired = self.queue(self.second)
        third = package("1.0.2", "A quiet minute.")

        def supersede(stage):
            if stage == "after_commit":
                self.queue(third)

        self.assertEqual(self.deliver(hook=supersede).status, "installed")
        self.assertEqual(self.device.installed().revision, second_desired.revision)
        self.assertEqual(self.device.installed().payload, self.second)
        self.assertFalse(self.fixture.control.acknowledged("x3-dev"))
        self.assertEqual(self.deliver().status, "installed")
        self.assertEqual(self.device.installed().payload, third)
        self.assertTrue(self.fixture.control.acknowledged("x3-dev"))

    def test_missing_or_corrupt_artifact_retains_previous_install(self):
        _, old = self.baseline()
        desired = self.queue(self.second)
        artifact = self.root / "artifacts" / (desired.digest + ".newt")
        artifact.unlink()
        with self.assertRaises(FileNotFoundError):
            self.deliver()
        self.assertEqual(self.device.installed(), old)
        artifact.write_bytes(self.first)  # Valid package, wrong content identity.
        with self.assertRaises(ValueError):
            self.deliver()
        self.assertEqual(self.device.installed(), old)
        artifact.write_bytes(self.second[:-1])
        with self.assertRaises(ValueError):
            self.deliver()
        self.assertEqual(self.device.installed(), old)

    def test_corrupt_staging_is_detected_before_commit(self):
        _, old = self.baseline()
        self.queue(self.second)

        def corrupt(stage):
            if stage == "staging":
                (self.root / "devices/x3-dev/installed.json.pending").write_bytes(b"{}")

        with self.assertRaises(ValueError):
            self.deliver(hook=corrupt)
        self.assertEqual(self.device.installed(), old)
        self.assertEqual(self.deliver().status, "installed")

    def test_restart_rejects_invalid_installed_record(self):
        self.baseline()
        path = self.root / "devices/x3-dev/installed.json"
        record = json.loads(path.read_bytes())
        record["payload"] = "?"
        path.write_text(json.dumps(record))
        with self.assertRaises(ValueError):
            fixture(self.root).device("x3-dev").installed()

    def test_rollback_or_same_revision_retarget_is_rejected(self):
        self.baseline()
        self.queue(self.second)
        self.deliver()
        path = self.root / "control/x3-dev.json"
        record = json.loads(path.read_bytes())
        for revision in (1, 2):
            record["revision"] = revision
            record["digest"] = self.fixture.artifacts.add(self.first)
            path.write_text(json.dumps(record))
            with self.assertRaises(DeliveryError):
                self.deliver()
            self.assertEqual(self.device.installed().payload, self.second)

    def test_inputs_are_bounded_and_cannot_select_paths(self):
        for alias in ("../escape", "/outside-project/escape", "X3", "", "a" * 33):
            with self.assertRaises(DeliveryError):
                self.fixture.device(alias)
            with self.assertRaises(DeliveryError):
                self.fixture.control.queue(alias, "0" * 64)
        for digest in ("../escape", "0" * 63, "A" * 64):
            with self.assertRaises(DeliveryError):
                self.fixture.artifacts.get(digest)
        with self.assertRaises(DeliveryError):
            self.fixture.artifacts.add(self.first + b"x")
        with self.assertRaises(DeliveryError):
            self.deliver(failure_stage="typo")

    def test_desired_corruption_and_revision_overflow_are_not_repaired_silently(self):
        self.queue(self.first)
        path = self.root / "control/x3-dev.json"
        record = json.loads(path.read_bytes())
        record["revision"] = True
        path.write_text(json.dumps(record))
        with self.assertRaises(DeliveryError):
            self.deliver()
        record["revision"] = (1 << 63) - 1
        path.write_text(json.dumps(record))
        with self.assertRaises(DeliveryError):
            self.queue(self.second)
        self.assertIsNone(self.device.installed())
        path.write_bytes(b'{"revision":1,"revision":2,"digest":"' + b"0" * 64 +
                         b'","acknowledged":false}')
        with self.assertRaises(DeliveryError):
            self.deliver()


if __name__ == "__main__":
    unittest.main()
