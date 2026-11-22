"""Fault controls for the linked image keeping FreeRTOS's timer task and
Arduino's RGB LED path out (docs/CODE_HEALTH.md, record 72)."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("native_guard", ROOT / "tools/verify-nvs-guard.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)

# tasks.c's weak no-op, as linked with the RGB switch.
SYMBOL = "40387868  w    F .iram0.text\t00000004 xTimerCreateTimerTask"
CODE = """40387868 <xTimerCreateTimerTask>:
40387868:\t4505                \tli\ta0,1
4038786a:\t8082                \tret
"""
# 842f364b's link: timers.c's strong version, which starts the timer task.
TIMERS_SYMBOL = "40387714 g     F .iram0.text\t00000076 xTimerCreateTimerTask"
TIMERS_CODE = """40387714 <xTimerCreateTimerTask>:
40387714:\t1101                \taddi\tsp,sp,-32
40387716:\tce06                \tsw\tra,28(sp)
40387718:\tdd9ff0ef          \tjal\t403874f0 <prvCheckForValidListAndQueue>
"""
RGB_CODE = """420700fc <rgbLedWrite>:
420700fc:\t4505                \tli\ta0,1
"""
RMT_CODE = """42070200 <rmtInit>:
42070200:\t4505                \tli\ta0,1
"""


class TimerTaskGuard(unittest.TestCase):
    def reject(self, code=CODE, symbol=SYMBOL, message=""):
        with self.assertRaisesRegex(RuntimeError, message):
            guard.validate_no_timer_task(guard.parse_functions(code), symbol)

    def test_weak_no_op_passes(self):
        guard.validate_no_timer_task(guard.parse_functions(CODE), SYMBOL)

    def test_linked_timers_are_rejected(self):
        self.reject(TIMERS_CODE, TIMERS_SYMBOL, "timer task")

    def test_weak_hook_with_work_is_rejected(self):
        self.reject(TIMERS_CODE, SYMBOL, "timer task")

    def test_missing_or_duplicate_hook_is_rejected(self):
        self.reject(symbol="", message="timer task")
        self.reject(symbol=SYMBOL + "\n" + SYMBOL, message="timer task")

    def test_rgb_led_path_is_rejected(self):
        self.reject(CODE + RGB_CODE, message="RGB LED path")
        self.reject(CODE + RMT_CODE, message="RGB LED path")


if __name__ == "__main__":
    unittest.main()
