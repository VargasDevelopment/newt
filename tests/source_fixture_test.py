"""A missing or duplicated control anchor cannot silently omit coverage."""
import unittest
from source_fixture import replace_once


class SourceFixtureTest(unittest.TestCase):
    def test_unique_anchor(self):
        self.assertEqual(replace_once("start MARK end", "MARK", "actual code"), "start actual code end")

    def test_each_drift_fails_closed(self):
        for source in ("missing", "MARK\nMARK"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                replace_once(source, "MARK", "replacement")


if __name__ == "__main__":
    unittest.main()
