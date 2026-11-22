"""Boundary and canonical-wire tests for the one supported app shape."""

import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from newt_package import (MAX_SOURCE_BYTES, PACKAGE_BYTES, decode_package,
                          encode_app, package_digest, validate_source)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.source = (ROOT / "apps/pocket-pause.json").read_bytes()
        self.app = validate_source(self.source)
        self.package = encode_app(self.app)

    def reject_app(self, app):
        with self.assertRaises(ValueError):
            encode_app(app)
        with self.assertRaises(ValueError):
            validate_source(json.dumps(app).encode())

    def test_fixture_is_reproducible(self):
        self.assertEqual(len(self.package), PACKAGE_BYTES)
        self.assertEqual(self.package[:8], b"NEWTAPP1")
        self.assertEqual(decode_package(self.package), self.app)
        reordered = json.dumps(self.app, sort_keys=True).encode()
        self.assertEqual(encode_app(validate_source(reordered)), self.package)
        self.assertEqual(package_digest(self.package), hashlib.sha256(self.package).hexdigest())
        self.assertEqual(encode_app(decode_package(self.package)), self.package)
        self.assertEqual([card["action"] for card in self.app["cards"]],
                         ["show-note", "show-note", "show-note"])
        self.assertEqual(len({card["detail"] for card in self.app["cards"]}), 3)
        self.assertEqual(self.app["note"], "")

    def test_string_and_card_bounds(self):
        app = copy.deepcopy(self.app)
        app.update(id="a" * 32, version="123456789012.0.0", title="t" * 32,
                   subtitle="s" * 64, note="n" * 160)
        app["cards"] = [{"title": "c" * 32, "detail": "d" * 64,
                         "action": "show-note"}]
        encoded = encode_app(app)
        self.assertEqual(decode_package(encoded), app)
        self.assertEqual(encoded[417:], bytes(198))
        for key, limit in (("id", 32), ("version", 16), ("title", 32),
                           ("subtitle", 64), ("note", 160)):
            invalid = copy.deepcopy(app)
            invalid[key] = "a" * (limit + 1)
            self.reject_app(invalid)
        app["subtitle"] = app["note"] = ""
        self.assertEqual(decode_package(encode_app(app)), app)
        for key in ("id", "version", "title"):
            invalid = copy.deepcopy(app)
            invalid[key] = ""
            self.reject_app(invalid)
        for key, limit in (("title", 32), ("detail", 64)):
            for value in ("", "a" * (limit + 1)):
                invalid = copy.deepcopy(app)
                invalid["cards"][0][key] = value
                self.reject_app(invalid)
        for cards in ([], app["cards"] * 4, True, 1, {}, None):
            invalid = copy.deepcopy(app)
            invalid["cards"] = cards
            self.reject_app(invalid)

    def test_exact_schema_and_no_delivery_state(self):
        for key in self.app:
            invalid = copy.deepcopy(self.app)
            del invalid[key]
            self.reject_app(invalid)
        for key in ("device", "recipient", "account", "path", "queue", "permissions"):
            invalid = copy.deepcopy(self.app)
            invalid[key] = "test"
            self.reject_app(invalid)
        for key in self.app["cards"][0]:
            invalid = copy.deepcopy(self.app)
            del invalid["cards"][0][key]
            self.reject_app(invalid)
        invalid = copy.deepcopy(self.app)
        invalid["cards"][0]["script"] = "anything"
        self.reject_app(invalid)
        for action in ("unknown", "sleep", "", True, 1, None):
            invalid = copy.deepcopy(self.app)
            invalid["cards"][0]["action"] = action
            self.reject_app(invalid)
        for invalid in (None, [], True, 1, "app"):
            self.reject_app(invalid)

    def test_slug_versions_and_printable_ascii(self):
        for identifier in ("A", "1app", "../app", "a_b", "a b", "a.b"):
            invalid = copy.deepcopy(self.app)
            invalid["id"] = identifier
            self.reject_app(invalid)
        for version in ("1", "1.2", "1.2.3.4", "v1.2.3", "01.2.3", "1.02.3",
                        "1.2.03", "1.-2.3", "1.2.3-beta", "1.2.3\n"):
            invalid = copy.deepcopy(self.app)
            invalid["version"] = version
            self.reject_app(invalid)
        for key in ("title", "subtitle", "note"):
            for text in ("line\n", "tab\t", "null\0", "del\x7f", "caf\u00e9", "\x1f"):
                invalid = copy.deepcopy(self.app)
                invalid[key] = text
                self.reject_app(invalid)
        for key in ("title", "detail"):
            invalid = copy.deepcopy(self.app)
            invalid["cards"][0][key] = "\u2603"
            self.reject_app(invalid)

    def test_source_limits_duplicates_and_bad_json(self):
        compact = json.dumps(self.app).encode()
        self.assertEqual(validate_source(compact + b" " * (MAX_SOURCE_BYTES - len(compact))), self.app)
        with self.assertRaises(ValueError):
            validate_source(compact + b" " * (MAX_SOURCE_BYTES + 1 - len(compact)))
        for source in (b"", b"{", b"{} trailing", b"\xff", b"\xef\xbb\xbf{}",
                       b"[" * 3000 + b"]" * 3000):
            with self.assertRaises(ValueError):
                validate_source(source)
        for source in (compact.replace(b'"id":', b'"id": "other", "id":', 1),
                       compact.replace(b'"detail":', b'"detail": "other", "detail":', 1)):
            with self.assertRaises(ValueError):
                validate_source(source)
        for api in (validate_source, decode_package, package_digest):
            with self.assertRaises(ValueError):
                api("not bytes")

    def test_wire_size_magic_counts_and_actions(self):
        for size in range(PACKAGE_BYTES):
            with self.assertRaises(ValueError):
                decode_package(self.package[:size])
        for package in (self.package + b"\0", b"BADMAGIC" + self.package[8:]):
            with self.assertRaises(ValueError):
                decode_package(package)
        for count in (0, 4, 255):
            invalid = bytearray(self.package)
            invalid[317] = count
            with self.assertRaises(ValueError):
                decode_package(bytes(invalid))
        for offset in (416, 515, 614):
            for action in (0, 4, 255):
                invalid = bytearray(self.package)
                invalid[offset] = action
                with self.assertRaises(ValueError):
                    decode_package(bytes(invalid))

    def test_wire_strings_padding_and_unused_records(self):
        fields = ((8, 33), (41, 17), (58, 33), (91, 65), (156, 161),
                  (318, 33), (351, 65), (417, 33), (450, 65), (516, 33), (549, 65))
        for offset, width in fields:
            invalid = bytearray(self.package)
            invalid[offset:offset + width] = b"a" * width
            with self.assertRaises(ValueError):
                decode_package(bytes(invalid))
            raw = self.package[offset:offset + width]
            terminator = raw.find(b"\0")
            invalid = bytearray(self.package)
            # A maximum field has only its terminator left, no padding byte.
            invalid[offset + min(terminator + 1, width - 1)] = 1
            with self.assertRaises(ValueError):
                decode_package(bytes(invalid))
            for char in (1, 31, 127, 128, 255):
                invalid = bytearray(self.package)
                invalid[offset] = char
                with self.assertRaises(ValueError):
                    decode_package(bytes(invalid))
        app = copy.deepcopy(self.app)
        app["cards"] = app["cards"][:1]
        package = encode_app(app)
        for offset in range(417, PACKAGE_BYTES):
            invalid = bytearray(package)
            invalid[offset] = 1
            with self.assertRaises(ValueError):
                decode_package(bytes(invalid))


if __name__ == "__main__":
    unittest.main()
