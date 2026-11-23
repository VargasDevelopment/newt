"""Release preparation (tools/release_tools.py --prepare), the gate before
`cz bump`, on tests/release_tools_test.py's fixture: bump and the incremental
changelog must both start at the configured version's own tag.
--negative-controls removes each guard in a copy of its tool and requires its test to fail."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_tools_test import ReleaseFixture, args, gate_tier, negative_controls, release


class ReleasePrepareTests(ReleaseFixture):
    def changelog(self, *tags):
        (self.root / "CHANGELOG.md").write_text("# Changelog\n" + "".join(f"\n## {tag}\n\n- a change\n" for tag in tags))

    def test_preparation_needs_the_changelog_to_end_at_the_release(self):
        """The incremental changelog starts after its latest heading, bump after the version's tag."""
        self.git("checkout", "-qB", "main")
        self.baseline("0.1.0")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.branch("release", "main")
        self.assertFalse((self.root / "CHANGELOG.md").exists())
        with self.assertRaisesRegex(ValueError, "CHANGELOG.md's latest release heading is missing, not v0.1.0"):
            release.gate(self.root, prepare=True)
        for tags in ((), ("v0.1.0-alpha.0",), ("v0.1.1", "v0.1.0")):
            with self.subTest(tags=tags):
                self.changelog(*tags)
                with self.assertRaisesRegex(ValueError, "latest release heading is .*, not v0.1.0"):
                    release.gate(self.root, prepare=True)
        self.changelog("v0.1.0", "v0.1.0-alpha.0")
        self.assertEqual(release.gate(self.root, prepare=True), ["v0.1.0"])

    def test_preparation_starts_at_its_own_tag_never_its_parents(self):
        """Just after a release merges, main's version is untagged until CI tags it. CI's run on
        that merge checks from its parent's tag; preparing the next release must not, since cz
        bump would read the whole history as an initial release."""
        history = self.release_history()
        self.changelog("v0.1.0-alpha.1", "v0.1.0-alpha.0")
        self.assertEqual(release.gate(self.root), ["v0.1.0-alpha.0"])
        with self.assertRaisesRegex(ValueError, "Release 0.1.0-alpha.1 has no tag v0.1.0-alpha.1"):
            release.gate(self.root, prepare=True)
        self.git("tag", "-a", "v0.1.0-alpha.1", "-m", "Newt firmware 0.1.0-alpha.1", history.release)
        self.assertEqual(release.gate(self.root, prepare=True), ["v0.1.0-alpha.1"])
        # The release branch's own commit has that version but not the tagged merge.
        self.git("checkout", "-q", "--detach", history.record)
        with self.assertRaisesRegex(ValueError, "does not contain v0.1.0-alpha.1"):
            release.gate(self.root, prepare=True)


# (tool, old, new, test): removing the guard must make its test fail.
CONTROLS = (
    ("release_tools.py", '    if rules["changelog_tag"] != tag:', "    if False:",
     "test_preparation_needs_the_changelog_to_end_at_the_release"),
    ("release_tools.py", "        built_on = [release_start(root, head)]", "        built_on = built_on[:1]",
     "test_preparation_starts_at_its_own_tag_never_its_parents"),
    ("release_tools.py", "    if not is_ancestor(root, tagged, head):", "    if False:",
     "test_preparation_starts_at_its_own_tag_never_its_parents"),
)


if __name__ == "__main__":
    loader = unittest.defaultTestLoader
    suite = (loader.loadTestsFromName(args.single, ReleasePrepareTests) if args.single
             else loader.loadTestsFromTestCase(ReleasePrepareTests))
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        sys.exit(1)
    if args.negative_controls or (not args.single and gate_tier.negative_controls(__file__)):
        negative_controls(CONTROLS, __file__)
