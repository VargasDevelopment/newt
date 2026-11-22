"""Fault controls for the actual C3 loop-task creation instruction contract."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("native_guard", ROOT / "tools/verify-nvs-guard.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
from rust_open_stack import OPEN_PROFILE

SYMBOL = "42000100 g     F .flash.text 00000004 getArduinoLoopTaskStackSize()"
CODE = """42000100 <getArduinoLoopTaskStackSize()>:
42000100: 6511 lui a0,0x4
42000102: 8082 ret
420001f0 <app_main>:
420001f0: 000000ef jal 42000300 <setCpuFrequencyMhz>
420001f4: 000000ef jal 42000400 <initArduino>
42000200: 000000ef jal 42000100 <getArduinoLoopTaskStackSize()>
42000204: 862a mv a2,a0
42000206: 4203b537 lui a0,0x4203b
4200020a: f4450513 addi a0,a0,-188 # 4203af44 <loopTask(void*)>
4200020e: 808fc06f j 42036fe8 <xTaskCreateUniversal>
"""


class ReaderStackGuard(unittest.TestCase):
    def reject(self, code=CODE, symbol=SYMBOL, message=""):
        with self.assertRaisesRegex(RuntimeError, message):
            guard.validate_reader_stack(guard.parse_functions(code), symbol)

    def test_owned_capacity_reaches_real_loop_task_argument(self):
        result = guard.validate_reader_stack(guard.parse_functions(CODE), SYMBOL)
        self.assertEqual(result["stack_bytes"], 16384)
        self.assertEqual(result["additional_dynamic_bytes"], 8192)

    def test_default_weak_definition_is_rejected(self):
        self.reject(symbol=SYMBOL.replace(" g ", " w "), message="strong")

    def test_duplicate_definition_is_rejected(self):
        self.reject(symbol=SYMBOL + "\n" + SYMBOL, message="one strong")

    def test_old_eight_kib_return_is_rejected(self):
        self.reject(CODE.replace("a0,0x4", "a0,0x2"), message="16384")

    def test_unknown_return_body_is_rejected(self):
        self.reject(CODE.replace("42000102: 8082 ret", "42000102: 000000ef jal 42000300 <decoy>\n42000106: 8082 ret"), message="16384")

    def test_default_literal_does_not_replace_owned_stack(self):
        self.reject(CODE.replace("mv a2,a0", "li a2,8192"), message="not passed")

    def test_later_size_argument_clobber_is_rejected(self):
        self.reject(CODE.replace("4200020e:", "4200020c: 00000613 li a2,1\n4200020e:"), message="not passed")

    def test_missing_stack_call_is_rejected(self):
        self.reject(CODE.replace("jal 42000100 <getArduinoLoopTaskStackSize()>", "jal 42000300 <decoy>"), message="occur once")

    def test_missing_task_creation_is_rejected(self):
        self.reject(CODE.replace("<xTaskCreateUniversal>", "<decoy>"), message="occur once")

    def test_wrong_task_entry_is_rejected(self):
        self.reject(CODE.replace("<loopTask(void*)>", "<anotherTask(void*)>"), message="not passed")

    def test_task_entry_clobber_is_rejected(self):
        self.reject(CODE.replace("4200020e:", "4200020c: 00000513 li a0,0\n4200020e:"), message="not passed")

    def test_branch_bypass_is_rejected(self):
        self.reject(CODE.replace("42000204:", "42000202: 00000063 beq a0,zero,4200020e\n42000204:"), message="control transfer")

    def test_pre_provider_branch_cannot_create_default_task(self):
        bypass = "420001f0: 00000613 li a2,8192\n420001f4: 00000513 addi a0,a0,0 # 4203af44 <loopTask(void*)>\n420001f8: 00000063 beq a1,zero,4200020e\n"
        self.reject(CODE.replace("42000200: 000000ef", bypass + "42000200: 000000ef"), message="bypass")

    def test_pre_provider_jump_cannot_create_default_task(self):
        self.reject(CODE.replace("42000200: 000000ef", "420001f8: 0000006f j 4200020e <app_main+0xe>\n42000200: 000000ef"), message="bypass")

    def test_tail_jump_to_provider_cannot_replace_a_call(self):
        self.reject(CODE.replace("jal 42000100", "j 42000100"), message="bypass")

    def test_pre_provider_call_cannot_enter_creator_inside_app_main(self):
        bypass = "420001f8: 00000637 lui a2,0x2\n420001fc: 000000ef jal 4200020e <app_main+0x1e>\n"
        self.reject(CODE.replace("42000200: 000000ef", bypass + "42000200: 000000ef"), message="entrypoints")

    def test_provider_offset_cannot_skip_constant(self):
        self.reject(CODE.replace("jal 42000100 <getArduinoLoopTaskStackSize()>", "jal 42000102 <getArduinoLoopTaskStackSize()+0x2>"), message="entrypoints")

    def test_creator_offset_cannot_skip_task_setup(self):
        self.reject(CODE.replace("<xTaskCreateUniversal>", "<xTaskCreateUniversal+0x2>"), message="entrypoints")

    def test_startup_offset_cannot_skip_preservation_guard(self):
        self.reject(CODE.replace("<initArduino>", "<initArduino+0x2>"), message="entrypoints")

    def test_intervening_call_cannot_clobber_stack_argument(self):
        self.reject(CODE.replace("42000206:", "42000205: 000000ef jal 42000300 <decoy>\n42000206:"), message="control transfer")


class RustOpenFrames(unittest.TestCase):
    def frames(self, sizes):
        return {OPEN_PROFILE + suffix: [
            f"42000100: 00010113 addi sp,sp,-{size}",
            f"42000104: 00010113 addi sp,sp,{size}",
            "42000108: 8082 ret",
        ] for suffix, size in sizes.items()}

    def test_original_profile_fits(self):
        self.assertEqual(guard.validate_open_frames(self.frames({"": 672}))["reserved_bytes"], 672)

    def test_split_profile_includes_retained_closure(self):
        result = guard.validate_open_frames(self.frames({"": 112, "::{{closure}}": 560}))
        self.assertEqual(result["reserved_bytes"], 672)

    def test_small_wrapper_cannot_hide_large_feature_frame(self):
        with self.assertRaisesRegex(RuntimeError, "2144.*672"):
            guard.validate_open_frames(self.frames({"": 112, "::{{closure}}": 2032}))

    def test_nested_profile_helper_cannot_escape_ceiling(self):
        with self.assertRaisesRegex(RuntimeError, "688.*672"):
            guard.validate_open_frames(self.frames({"": 112, "::{{closure}}": 560,
                                                     "::{{closure}}::{{closure}}": 16}))

    def test_unknown_stack_write_is_rejected(self):
        functions = self.frames({"": 112})
        next(iter(functions.values())).insert(1, "42000102: 40c10133 sub sp,sp,a2")
        with self.assertRaisesRegex(RuntimeError, "Unresolved.*SP adjustment"):
            guard.validate_open_frames(functions)

    def test_missing_profile_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "Missing selected"):
            guard.validate_open_frames({})

    def test_bit_instruction_cannot_masquerade_as_a_branch(self):
        functions = self.frames({"": 112})
        next(iter(functions.values())).insert(1, "42000102: 00010113 bset sp,sp,a2")
        with self.assertRaisesRegex(RuntimeError, "Unresolved.*SP adjustment"):
            guard.validate_open_frames(functions)


if __name__ == "__main__":
    unittest.main()
