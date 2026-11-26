"""Deterministic checks for the synthetic S/L/I page-turn fixture generator (#168)."""
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import page_turn_fixtures as fixtures


class PageTurnFixturesTest(unittest.TestCase):
    def test_books_are_deterministic_and_valid_epubs(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            first = fixtures.generate(Path(a), ["s", "l", "i"])
            second = fixtures.generate(Path(b), ["s", "l", "i"])
            self.assertEqual(first, second)
            manifest = json.loads((Path(a) / "manifest.json").read_text())
            self.assertEqual(manifest, first)

            for key in ("s", "l", "i"):
                epub = Path(a) / fixtures.SPECS[key].filename
                self.assertEqual(
                    hashlib.sha256(epub.read_bytes()).hexdigest(),
                    first["books"][key]["sha256"],
                )
                with zipfile.ZipFile(epub, "r") as zf:
                    self.assertIsNone(zf.testzip())
                    infos = zf.infolist()
                    self.assertEqual(infos[0].filename, "mimetype")
                    self.assertEqual(infos[0].compress_type, zipfile.ZIP_STORED)
                    self.assertEqual(zf.read("mimetype"), b"application/epub+zip")
                    self.assertIn("META-INF/container.xml", zf.namelist())
                    self.assertIn("OEBPS/content.opf", zf.namelist())
                    self.assertIn("OEBPS/nav.xhtml", zf.namelist())

    def test_book_s_has_short_chapters_and_declared_css(self):
        data = fixtures.build_book("s")
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "book-s.epub"
            path.write_bytes(data)
            with zipfile.ZipFile(path, "r") as zf:
                names = set(zf.namelist())
                self.assertIn("OEBPS/style.css", names)
                for idx in range(len(fixtures.SHORT_CHAPTER_PARAGRAPHS)):
                    self.assertIn(f"OEBPS/c{idx:02d}.xhtml", names)
                self.assertNotIn(b"style.css", zf.read("OEBPS/c00.xhtml"))
                self.assertIn(b'href="style.css"', zf.read("OEBPS/c01.xhtml"))

    def test_book_l_has_long_primary_and_short_second_chapter(self):
        data = fixtures.build_book("l")
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "book-l.epub"
            path.write_bytes(data)
            with zipfile.ZipFile(path, "r") as zf:
                ch1 = zf.read("OEBPS/c00.xhtml")
                ch2 = zf.read("OEBPS/c01.xhtml")
                nav = zf.read("OEBPS/nav.xhtml")
                self.assertGreater(len(ch1), 400_000)
                self.assertLess(len(ch2), 16_000)
                self.assertIn(b"c00.xhtml#s00_00400", nav)
                self.assertIn(b"c00.xhtml#s00_01400", nav)
                self.assertIn(b'id="s00_00400"', ch1)
                self.assertIn(b'id="s00_01400"', ch1)

    def test_book_i_has_valid_synthetic_pngs_at_segment_edges(self):
        data = fixtures.build_book("i")
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "book-i.epub"
            path.write_bytes(data)
            with zipfile.ZipFile(path, "r") as zf:
                ch1 = zf.read("OEBPS/c00.xhtml")
                nav = zf.read("OEBPS/nav.xhtml")
                for anchor in fixtures.IMAGE_EDGE_ANCHORS.values():
                    self.assertIn(f'c00.xhtml#{anchor}'.encode(), nav)
                    self.assertIn(f'id="{anchor}"'.encode(), ch1)
                for href, _ in fixtures.IMAGE_EDGE_PARAGRAPHS.values():
                    self.assertIn(f'src="{href}"'.encode(), ch1)
                    png = zf.read(f"OEBPS/{href}")
                    self.assertTrue(png.startswith(b"\x89PNG\r\n\x1a\n"))
                    length, kind, width, height = struct.unpack(">I4sII", png[8:24])
                    self.assertEqual((length, kind, width, height), (13, b"IHDR", 240, 160))
                    self.assertTrue(png.endswith(b"IEND\xaeB`\x82"))

    def test_no_generated_fixtures_committed_in_repo(self):
        for spec in fixtures.SPECS.values():
            matches = list(ROOT.glob(f"**/{spec.filename}"))
            self.assertEqual(matches, [], f"generated fixture must not be committed: {matches}")


if __name__ == "__main__":
    unittest.main()
