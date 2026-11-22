"""Fault controls for the linked hook that defers image confirmation to Newt's
first finished screen (docs/UPDATES.md, "Card install")."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("native_guard", ROOT / "tools/verify-nvs-guard.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)

SYMBOL = "42000100 g     F .flash.text 00000004 verifyRollbackLater"
CODE = """42000100 <verifyRollbackLater>:
42000100: 4505 li a0,1
42000102: 8082 ret
"""


class RollbackHook(unittest.TestCase):
    def reject(self, code=CODE, symbol=SYMBOL, message=""):
        with self.assertRaisesRegex(RuntimeError, message):
            guard.validate_rollback_hook(guard.parse_functions(code), symbol)

    def test_strong_true_hook_passes(self):
        guard.validate_rollback_hook(guard.parse_functions(CODE), SYMBOL)

    def test_arduino_weak_default_is_rejected(self):
        self.reject(symbol=SYMBOL.replace(" g ", " w "), message="strong")

    def test_duplicate_definition_is_rejected(self):
        self.reject(symbol=SYMBOL + "\n" + SYMBOL, message="one strong")

    def test_false_return_is_rejected(self):
        self.reject(CODE.replace("li a0,1", "li a0,0"), message="first finished screen")

    def test_extra_work_is_rejected(self):
        self.reject(CODE.replace("42000102:", "42000102: 000000ef jal 42000300 <decoy>\n42000106:"), message="first finished screen")


if __name__ == "__main__":
    unittest.main()
