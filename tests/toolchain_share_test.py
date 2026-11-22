"""A shared toolchain store stays pinned and read-only; a checkout's view reads
the same paths and bytes as its private copy, and nothing is ever deleted."""
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from code_health_resources import tree_hash

spec = importlib.util.spec_from_file_location("toolchain_share", ROOT / "tools/toolchain-share.py")
share = importlib.util.module_from_spec(spec)
spec.loader.exec_module(share)

LIBS = "packages/framework-arduinoespressif32-libs/esp32c3"
SCRIPT = LIBS + "/pioarduino-build.py"
IDF = "packages/framework-arduinoespressif32/idf_component.yml"
PERMISSIONS = os.geteuid() != 0  # Root ignores permission bits.


def writable_again(top):
    """Let a test remove what the tool made read-only."""
    for directory, names, _ in os.walk(top):
        os.chmod(directory, 0o755)
        for name in names:
            path = Path(directory, name)
            if not path.is_symlink():
                os.chmod(path, 0o755)


class ShareTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.addCleanup(writable_again, temp.name)
        base = Path(temp.name).resolve()
        self.root, self.store = base / "checkout", base / "store"
        self.store.mkdir()
        self.platformio = self.root / ".toolchain/platformio"
        files = {SCRIPT: "env.Append(CPPPATH=[])\n", IDF: "dependencies: {}\n", IDF + ".orig": "dependencies: {}\n",
                 "packages/toolchain-riscv32-esp/bin/gcc": "#!/bin/sh\n",
                 "platforms/espressif32/platform.py": "PLATFORM = 1\n",
                 "penv/lib/site.py": "SITE = 1\n", "penv/pyvenv.cfg": "home = /bin\n",
                 ".cache/uv/CACHEDIR.TAG": "cache\n", "appstate.json": "{}\n", "penv/.lock": ""}
        for name, text in files.items():
            (self.platformio / name).parent.mkdir(parents=True, exist_ok=True)
            (self.platformio / name).write_text(text)
        (self.platformio / "packages/toolchain-riscv32-esp/bin/gcc").chmod(0o755)
        (self.platformio / "penv/bin").mkdir()
        (self.platformio / "penv/bin/python").symlink_to("python3")
        (self.platformio / "penv/bin/python3").symlink_to("/usr/bin/python3")
        self.llvm_target = base / "other-checkout-llvm"
        (self.llvm_target / "bin").mkdir(parents=True)
        (self.llvm_target / "bin/clang-22").write_text("clang")
        (self.llvm_target / "bin/clang").symlink_to("clang-22")
        (self.root / ".toolchain/llvm").symlink_to(self.llvm_target)
        (self.root / ".build").mkdir()
        rooted = patch.object(share, "ROOT", self.root)
        rooted.start()
        self.addCleanup(rooted.stop)

    def identity(self, component):
        """Each stored file's bytes by relative name and, for a project-local
        tree, the resource identity's own hash of root-relative names and bytes."""
        top = Path(os.path.realpath(self.root / ".toolchain" / component))
        names = [name for name in share.walk(top) if not share.private(component, name)]
        contents = {name: os.readlink(top / name) if (top / name).is_symlink() else share.sha256(top / name)
                    for name in names if not (top / name).is_dir() or (top / name).is_symlink()}
        files = [top / name for name in names if (top / name).is_file() and not (top / name).is_symlink()]
        return contents, tree_hash(self.root, files) if top.is_relative_to(self.root) else None

    def migrate(self):
        with redirect_stdout(io.StringIO()) as output, redirect_stderr(io.StringIO()):
            self.assertEqual(share.main(["migrate", "--store", str(self.store)]), 0)
        return output.getvalue()

    def test_store_is_pinned_read_only_and_excludes_private_state(self):
        digest = share.store(self.store, "platformio", self.platformio)
        entry = self.store / f"platformio-{digest}"
        record = json.loads((entry / "manifest.json").read_text())
        names = [item[0] for item in record["entries"]]
        self.assertIn(SCRIPT, names)
        self.assertFalse([name for name in names if name.startswith((".cache", "appstate.json", "penv/.lock"))])
        self.assertEqual(share.digest_of(record), digest)
        for path in (entry / "tree" / SCRIPT, entry / "tree/packages", entry / "manifest.json", entry):
            self.assertFalse(path.stat().st_mode & 0o222, path)
        if PERMISSIONS:
            with self.assertRaises(PermissionError):
                (entry / "tree" / IDF).write_text("rewritten")
        self.assertEqual(share.store(self.store, "platformio", self.platformio), digest)
        self.assertEqual(len(list(self.store.iterdir())), 1)

    def test_store_refuses_a_never_built_or_unreadable_tree(self):
        # A first build writes this backup into a directory a view keeps read-only.
        original = self.platformio / (IDF + ".orig")
        original.rename(self.root / "held")
        with self.assertRaisesRegex(RuntimeError, "never built"):
            share.store(self.store, "platformio", self.platformio)
        (self.root / "held").rename(original)
        if PERMISSIONS:
            hidden = self.platformio / "platforms/espressif32"
            hidden.chmod(0)
            try:
                with self.assertRaises(PermissionError):
                    share.store(self.store, "platformio", self.platformio)
            finally:
                hidden.chmod(0o755)
        self.assertEqual(list(self.store.iterdir()), [])

    def test_a_concurrent_identical_store_is_verified_and_both_copies_kept(self):
        rename = os.rename

        def racing(source, destination):
            if Path(destination).parent == self.store and not Path(destination).exists():
                shutil.copytree(source, destination, symlinks=True)  # Another checkout finished first.
            rename(source, destination)
        with patch.object(share.os, "rename", side_effect=racing), redirect_stderr(io.StringIO()) as errors:
            digest = share.store(self.store, "platformio", self.platformio)
        share.verify_store(self.store, "platformio", digest)
        self.assertIn(f"stored platformio-{digest} first", errors.getvalue())
        self.assertEqual(len(list(self.store.iterdir())), 2)

    def test_migration_keeps_identity_and_moves_every_previous_copy_aside(self):
        before = {component: self.identity(component) for component in share.COMPONENTS}
        self.assertIsNone(before["llvm"][1])  # Read through the previous symlink.
        original = {path: (self.platformio / path).read_bytes() for path in share.walk(self.platformio)
                    if (self.platformio / path).is_file()}
        output = self.migrate()
        after = {component: self.identity(component) for component in share.COMPONENTS}
        self.assertEqual(after["platformio"], before["platformio"])
        self.assertEqual(after["llvm"][0], before["llvm"][0])
        toolchain = self.root / ".toolchain"
        moved = {path.name.split(".before-share-")[0]: path for path in toolchain.iterdir() if ".before-share-" in path.name}
        self.assertEqual(sorted(moved), ["llvm", "platformio"])
        self.assertEqual({path: (moved["platformio"] / path).read_bytes() for path in original}, original)
        self.assertEqual(os.readlink(moved["llvm"]), str(self.llvm_target))
        self.assertTrue((self.llvm_target / "bin/clang-22").is_file())
        self.assertIn("remove it yourself", output)
        self.assertIn("Kept verbatim, leaving platformio: penv/bin/python3\n", output)
        record = json.loads((self.root / share.RECORD).read_text())
        self.assertEqual(sorted(record), ["llvm", "platformio"])
        stored = self.store / f"platformio-{record['platformio']['digest']}/tree"
        self.assertTrue((self.platformio / IDF).samefile(stored / IDF))
        self.assertFalse((self.platformio / SCRIPT).samefile(stored / SCRIPT))
        self.assertEqual((self.platformio / SCRIPT).stat().st_nlink, 1)
        self.assertEqual((self.platformio / "appstate.json").read_text(), "{}\n")
        self.assertEqual(os.readlink(self.platformio / "penv/bin/python3"), "/usr/bin/python3")
        self.assertFalse((self.platformio / IDF).stat().st_mode & 0o222)  # pio's rewrite fails harmlessly
        # Running it again verifies the views and changes nothing.
        self.assertIn("Already linked platformio", self.migrate())
        self.assertEqual(len([path for path in toolchain.iterdir() if ".before-share-" in path.name]), 2)
        # A changed private copy is refused, never stored as a new pin.
        (self.platformio / SCRIPT).write_text("changed\n")
        with self.assertRaisesRegex(RuntimeError, "differs from the store"):
            self.migrate()
        self.assertEqual(len(list(self.store.iterdir())), 2)
        self.assertEqual(json.loads((self.root / share.RECORD).read_text()), record)
        # Nor is a view stored into another store.
        other = self.store.parent / "other-store"
        other.mkdir()
        with self.assertRaisesRegex(SystemExit, "already a view of"), redirect_stdout(io.StringIO()):
            share.main(["store", "--store", str(other), "--component", "llvm"])
        self.assertEqual(list(other.iterdir()), [])
        # A store moved behind a compatibility link is still the same store.
        moved = self.store.parent / "moved-store"
        self.store.rename(moved)
        self.store.symlink_to(moved)
        with redirect_stdout(io.StringIO()) as output:
            self.assertEqual(share.main(["store", "--store", str(moved), "--component", "llvm"]), 0)
        self.assertIn("Already linked llvm", output.getvalue())

    @unittest.skipUnless(PERMISSIONS, "root ignores permission bits")
    def test_view_writes_stay_private(self):
        self.migrate()
        with self.assertRaises(PermissionError):
            (self.platformio / IDF).write_text("rewritten")
        with self.assertRaises(PermissionError):
            (self.platformio / "platforms/espressif32/added.py").write_text("")
        # The platform's own edits: a backup beside the script, then a restore.
        backup = self.platformio / (SCRIPT + ".esp32c3")
        backup.write_bytes((self.platformio / SCRIPT).read_bytes())
        (self.platformio / SCRIPT).write_bytes(backup.read_bytes())
        backup.unlink()
        (self.platformio / "appstate.json").write_text('{"updated": true}\n')
        (self.platformio / "penv/.lock").write_text("")
        (self.platformio / ".cache").chmod(0)  # Private state is never read.
        try:
            with redirect_stdout(io.StringIO()):
                self.assertEqual(share.main(["verify"]), 0)
        finally:
            (self.platformio / ".cache").chmod(0o755)

    def test_verify_rejects_tampering_and_store_refuses_a_running_build(self):
        self.migrate()
        pin = json.loads((self.root / share.RECORD).read_text())["platformio"]
        tree = self.store / f"platformio-{pin['digest']}/tree"

        def rejected(change, restore):
            change()
            try:
                with self.assertRaises(RuntimeError):
                    share.verify_view(self.root, "platformio", pin["store"], pin["digest"])
            finally:
                restore()
            share.verify_view(self.root, "platformio", pin["store"], pin["digest"])

        libs = self.platformio / LIBS
        backup = libs / "pioarduino-build.py.esp32c3"
        rejected(lambda: backup.write_text("leftover"), backup.unlink)
        script = self.platformio / SCRIPT
        rejected(lambda: script.write_text("changed"), lambda: script.write_text("env.Append(CPPPATH=[])\n"))
        stored = tree / "platforms/espressif32/platform.py"
        sealed = stored.stat().st_mtime_ns

        def write_store(text):  # Same size, mode and mtime: only its bytes differ.
            stored.chmod(0o644)
            stored.write_text(text)
            stored.chmod(0o444)
            os.utime(stored, ns=(sealed, sealed))
        rejected(lambda: write_store("PLATFORM = 2\n"), lambda: write_store("PLATFORM = 1\n"))
        directory = self.platformio / "platforms/espressif32"
        viewed = directory / "platform.py"

        def copy_instead_of_link():
            directory.chmod(0o755)
            viewed.unlink()
            viewed.write_text("PLATFORM = 1\n")

        def relink():
            viewed.unlink()
            os.link(stored, viewed)
            directory.chmod(0o555)
        rejected(copy_instead_of_link, relink)
        backup.write_text("in progress")
        with self.assertRaisesRegex(RuntimeError, "backup"):
            share.store(self.store, "platformio", self.platformio)

    def test_health_check_rejects_owner_writes_through_a_view(self):
        # The owner can still chmod a stored inode through its view, as PlatformIO's
        # rmtree error handler does, write it in place and restore its mode, as an
        # editor's forced save does, or replace an identity input in a writable directory.
        self.migrate()
        sealed = time.time_ns() - 3600 * 10**9  # Stored an hour ago.
        for directory, _, files in os.walk(self.store):
            for name in files:
                os.utime(Path(directory, name), ns=(sealed, sealed), follow_symlinks=False)
        share.check_views(self.root)
        viewed = self.platformio / IDF

        def rewrite_in_place(text):
            viewed.chmod(0o644)
            viewed.write_text(text)
            viewed.chmod(0o444)
        directory = self.platformio / "platforms/espressif32"
        config = self.platformio / "penv/pyvenv.cfg"
        text = config.read_text()

        def replace_config():
            config.unlink()
            config.write_text(text)

        def relink_config():
            config.unlink()
            os.link(self.store / f"platformio-{self.pin()}/tree/penv/pyvenv.cfg", config)
        for change, restore in ((lambda: viewed.chmod(0o644), lambda: viewed.chmod(0o444)),
                                (lambda: rewrite_in_place("dependencies: []\n"),
                                 lambda: (rewrite_in_place("dependencies: {}\n"),
                                          os.utime(viewed, ns=(sealed, sealed)))),
                                (lambda: directory.chmod(0o755), lambda: directory.chmod(0o555)),
                                (replace_config, relink_config)):
            change()
            try:
                with self.assertRaisesRegex(RuntimeError, "differs from the store"):
                    share.check_views(self.root)
            finally:
                restore()
            share.check_views(self.root)

    def test_every_check_enforces_each_recorded_executable_bit(self):
        # A mode change leaves bytes, inode and modification time alone: only the
        # manifest's executable bit, which fixes each stored mode, shows it.
        self.migrate()
        digest = self.pin()
        tree = self.store / f"platformio-{digest}/tree"
        compiler, platform = "packages/toolchain-riscv32-esp/bin/gcc", "platforms/espressif32/platform.py"
        checks = {"verify_store": lambda: share.verify_store(self.store, "platformio", digest),
                  "check_views": lambda: share.check_views(self.root),
                  "verify_view": lambda: share.verify_view(self.root, "platformio", self.store, digest)}
        cases = (("exec bit removed in the store", tree / compiler, 0o444, 0o555, set(checks)),
                 ("group exec bit removed through a view", self.platformio / compiler, 0o545, 0o555, set(checks)),
                 ("exec bit added in the store", tree / platform, 0o555, 0o444, set(checks)),
                 ("exec bit added to a private copy", self.platformio / SCRIPT, 0o755, 0o644,
                  {"check_views", "verify_view"}),
                 ("stored directory mode", tree / "platforms/espressif32", 0o550, 0o555,
                  {"verify_store", "verify_view"}))
        for label, path, changed, kept, failing in cases:
            self.assertEqual(path.stat().st_mode & 0o777, kept)
            path.chmod(changed)
            try:
                for name, check in checks.items():
                    with self.subTest(label, check=name):
                        if name in failing:
                            with self.assertRaisesRegex(RuntimeError, "differs"):
                                check()
                        else:
                            check()
            finally:
                path.chmod(kept)
            for check in checks.values():
                check()

    def pin(self):
        return json.loads((self.root / share.RECORD).read_text())["platformio"]["digest"]

    def test_relinking_within_one_second_keeps_every_previous_copy(self):
        digest = share.store(self.store, "platformio", self.platformio)
        with patch.object(share.time, "strftime", return_value="20260101-000000"):
            share.link(self.store, "platformio", digest)
            share.link(self.store, "platformio", digest)
        toolchain = self.root / ".toolchain"
        self.assertEqual(sorted(path.name for path in toolchain.iterdir() if ".before-share-" in path.name),
                         ["platformio.before-share-20260101-000000", "platformio.before-share-20260101-000000-2"])
        share.verify_view(self.root, "platformio", self.store, digest)

    def test_link_refuses_another_filesystem_and_changes_nothing(self):
        digest = share.store(self.store, "platformio", self.platformio)
        with patch.object(share, "same_filesystem", return_value=False), self.assertRaises(RuntimeError):
            share.link(self.store, "platformio", digest)
        self.assertEqual(sorted(path.name for path in (self.root / ".toolchain").iterdir()), ["llvm", "platformio"])
        self.assertFalse((self.root / share.RECORD).exists())
        self.assertFalse(any(path.stat().st_nlink > 1 for path in self.platformio.rglob("*") if path.is_file()))


if __name__ == "__main__":
    unittest.main()
