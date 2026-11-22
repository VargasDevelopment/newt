"""Negative linked-image fixtures for the returning Newt sleep boundary."""
import importlib.util
import sys
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("native_guard", ROOT / "tools/verify-nvs-guard.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


def function(name, targets):
    lines = [f"42000100 <{name}>:"]
    for index, target in enumerate(targets):
        lines.append(f"{0x42000100 + index * 4:08x}: 000000ef jal 42000400 <{target}>")
    return "\n".join(lines)


ENTRY_CALLS = ["esp_sleep_config_gpio_isolate", "gpio_deep_sleep_hold_en",
               "esp_deep_sleep_try_to_start", guard.STAMP_SLEEP,
               "Print::printf(char const*, ...)", "HWCDC::flush()", "esp_restart"]
STAMP = f"""42000400 <{guard.STAMP_SLEEP}>:
42000400: 1141 addi sp,sp,-16
42000402: 842a mv s0,a0
42000404: 4785 li a5,1
42000406: cf1c sw a5,28(a0)
42000408: 000000ef jal 42000500 <newt::firmware::sleepChecksum(newt::firmware::SleepRecord const&)>
4200040c: 04a42023 sw a0,64(s0)
42000410: 8082 ret
"""
SYMBOLS = """50000014 l     O .rtc_noinit\t0000008c (anonymous namespace)::retainedSd
500000a0 l     O .rtc_noinit\t00000044 (anonymous namespace)::retained
"""
DEV_SYMBOLS = SYMBOLS + "3fca08e8  w    O .dram0.bss\t00000001 newt::firmware::readerStackObservation\n"
SECTIONS = """Sections:
Idx Name          Size      VMA       LMA       File off  Algn
  0 .rtc.text     00000014  50000000  50000000  001a0000  2**1
                  CONTENTS, ALLOC, LOAD, READONLY, CODE
  1 .rtc.force_fast 00000000 50000014  50000014  001a0104  2**0
                  CONTENTS
  2 .rtc_noinit   000000d0  50000014  50000014  001a0014  2**2
                  ALLOC
  3 .rtc.force_slow 00000020 500000e4  500000e4  001a00e4  2**2
                  CONTENTS, ALLOC, LOAD, DATA
  4 .rtc_reserved 00000028  50001fd8  50001fd8  001a0fd8  2**3
                  ALLOC
"""
DEV_SECTIONS = SECTIONS


def valid_functions(calls=None, stamp=STAMP):
    return guard.parse_functions(function(guard.ENTER_SLEEP, calls or ENTRY_CALLS) + "\n" + stamp)


class NativeSleepGuardTest(unittest.TestCase):
    def check_failure(self, functions=None, symbols=SYMBOLS, sections=SECTIONS, match=None):
        with self.assertRaisesRegex(RuntimeError, match or ".+"):
            guard.validate_sleep_boundary(functions or valid_functions(), symbols, sections)

    def test_valid_boundary_and_rtc(self):
        result = guard.validate_sleep_boundary(valid_functions(), SYMBOLS, SECTIONS)
        self.assertEqual(result["retained"]["bytes"], 68)
        self.assertEqual(result["rejection_stamp_word_offsets"], [28, 64])
        self.assertEqual(result["rtc_retention"]["bytes"], 300)
        self.assertFalse(result["rtc_retention"]["development"])

    def test_valid_development_rtc_exception(self):
        result = guard.validate_retained(DEV_SYMBOLS, DEV_SECTIONS)
        self.assertTrue(result["development"])
        self.assertEqual((result["bytes"], result["sections"][".rtc_noinit"]), (300, 208))
        self.assertNotIn(guard.RETAINED_HOME, result["records"])
        self.assertEqual(result["dev_token_store_proof"], "not-linked")

    def test_jalr_and_tail_jump_keep_order_and_nvs_targets(self):
        parsed = guard.parse_functions("""42000100 <initArduino>:
42000100: 00000097 auipc ra,0x0
42000104: 000080e7 jalr 0(ra) # 42000200 <__wrap_nvs_flash_init>
42000108: 000000ef jal 42000300 <__wrap_esp_partition_erase_range>
4200010c: 0000006f j 42000400 <helper+0x8>
42000200 <__wrap_nvs_flash_init>:
42000200: 0000006f j 42000500 <nvs_flash_init>
""")
        self.assertEqual(guard.ordered_calls(parsed["initArduino"]),
                         ["__wrap_nvs_flash_init", "__wrap_esp_partition_erase_range", "helper"])
        self.assertEqual(guard.ordered_calls(parsed["__wrap_nvs_flash_init"]), ["nvs_flash_init"])

    def test_long_try_call_and_tail_restart(self):
        source = function(guard.ENTER_SLEEP, ENTRY_CALLS)
        source = source.replace("jal 42000400 <esp_deep_sleep_try_to_start>",
                                "jalr 530(ra) # 4038172a <esp_deep_sleep_try_to_start>")
        source = source.replace("jal 42000400 <esp_restart>", "j 4038172a <esp_restart>")
        guard.validate_sleep_boundary(guard.parse_functions(source + "\n" + STAMP), SYMBOLS, SECTIONS)

    def test_actual_old_fourteen_hex_byte_sdk_boundary(self):
        old = """4200650c <freeink::PowerManager::deepSleep()>:
4200650c: 1141 addi sp,sp,-16
4200650e: c606 sw ra,12(sp)
42006510: 5a1300ef jal 420372b0 <esp_sleep_config_gpio_isolate>
42006514: 1d92a0ef jal 42030eec <gpio_deep_sleep_hold_en>
42006518: fe37b097 auipc ra,0xfe37b
4200651c: 212080e7 jalr 530(ra) # 4038172a <esp_deep_sleep_start>
"""
        self.check_failure(guard.parse_functions(old), match="obsolete SDK")

    def test_old_sdk_still_linked_alongside_new(self):
        functions = valid_functions()
        functions["freeink::PowerManager::deepSleep()"] = []
        self.check_failure(functions, match="obsolete SDK")

    def test_terminal_api_linked_symbol_only(self):
        symbols = SYMBOLS + "4038172a g     F .iram0.text\t00000014 esp_deep_sleep_start\n"
        self.check_failure(symbols=symbols, match="Noreturn")

    def test_terminal_api_direct_call(self):
        functions = valid_functions()
        functions["unrelated"] = ["42000900: 000000ef jal 4038172a <esp_deep_sleep_start>"]
        self.check_failure(functions, match="Noreturn")

    def test_marker_before_try(self):
        calls = ENTRY_CALLS.copy()
        calls[2], calls[3] = calls[3], calls[2]
        self.check_failure(valid_functions(calls), match="must follow")

    def test_restart_before_marker(self):
        calls = ENTRY_CALLS.copy()
        calls[3], calls[-1] = calls[-1], calls[3]
        self.check_failure(valid_functions(calls), match="precede restart")

    def test_missing_try_restart_and_duplicate_marker(self):
        for target in ["esp_deep_sleep_try_to_start", "esp_restart"]:
            with self.subTest(target=target):
                self.check_failure(valid_functions([call for call in ENTRY_CALLS if call != target]),
                                   match="missing or duplicates")
        self.check_failure(valid_functions(ENTRY_CALLS + [guard.STAMP_SLEEP]), match="duplicates")

    def test_matching_calls_in_different_function_do_not_pass(self):
        functions = guard.parse_functions(function("decoy", ENTRY_CALLS) + "\n" + STAMP)
        functions[guard.ENTER_SLEEP] = []
        self.check_failure(functions, match="missing or duplicates")

    def test_branch_or_early_return_cannot_bypass_ordered_calls(self):
        for instruction in ["beqz a0,42000100", "j 42000100 <entry+0x10>", "ret", "jr a5"]:
            with self.subTest(instruction=instruction):
                functions = valid_functions()
                functions[guard.ENTER_SLEEP].insert(0, "420000fc: 000000ef " + instruction)
                self.check_failure(functions, match="branch bypass")

    def test_unresolved_transfers_cannot_bypass_ordered_calls(self):
        for instruction in ["jal 42000100", "jalr 0(t0)", "jalr zero,0(t0)", "j 42000100"]:
            with self.subTest(instruction=instruction):
                functions = valid_functions()
                functions[guard.ENTER_SLEEP].insert(0, "420000fc: 000000ef " + instruction)
                self.check_failure(functions, match="unresolved control transfer")

    def test_missing_named_stamp(self):
        functions = valid_functions()
        del functions[guard.STAMP_SLEEP]
        self.check_failure(functions, match="both be linked")

    def test_stamp_requires_both_retained_word_stores(self):
        for store in ["sw a5,28(a0)", "sw a0,64(s0)"]:
            with self.subTest(store=store):
                self.check_failure(valid_functions(stamp=STAMP.replace(store, "nop")), match="store")

    def test_checksum_store_through_unrelated_pointer_rejected(self):
        self.check_failure(valid_functions(stamp=STAMP.replace("mv s0,a0", "mv s0,a1")), match="store")

    def test_call_clobbered_record_alias_rejected(self):
        self.check_failure(valid_functions(stamp=STAMP.replace("64(s0)", "64(a0)")), match="store")

    def test_inline_checksum_record_stores(self):
        stamp = STAMP.replace("42000408: 000000ef jal 42000500 <newt::firmware::sleepChecksum(newt::firmware::SleepRecord const&)>",
                              "42000408: 4158 lw a4,4(a0)")
        guard.validate_sleep_boundary(valid_functions(stamp=stamp), SYMBOLS, SECTIONS)

    def test_old_record_size_or_missing_symbol(self):
        self.check_failure(symbols=SYMBOLS.replace("00000044", "00000040"), match="68-byte")
        self.check_failure(symbols="", match="68-byte")

    def test_wrong_record_name_or_duplicate_symbol(self):
        self.check_failure(symbols=SYMBOLS.replace("::retained\n", "::other\n"), match="68-byte")
        self.check_failure(symbols=SYMBOLS + SYMBOLS.splitlines()[1] + "\n", match="68-byte")

    def test_bss_placement_or_wrong_symbol_kind(self):
        sleep = SYMBOLS.splitlines()[1]
        self.check_failure(symbols=SYMBOLS.replace(sleep, sleep.replace(".rtc_noinit", ".rtc.bss")), match="68-byte")
        self.check_failure(symbols=SYMBOLS.replace(sleep, sleep.replace(" O ", " F ")), match="68-byte")

    def test_initialized_or_nonallocated_rtc_section(self):
        for flags in ["CONTENTS, ALLOC, LOAD, DATA", "CONTENTS", "ALLOC, LOAD", "ALLOC, DATA"]:
            with self.subTest(flags=flags):
                self.check_failure(sections=SECTIONS.replace("  2 .rtc_noinit   000000d0  50000014  50000014  001a0014  2**2\n                  ALLOC",
                                                             "  2 .rtc_noinit   000000d0  50000014  50000014  001a0014  2**2\n                  " + flags),
                                   match="ALLOC-only")

    def test_rtc_symbol_outside_section(self):
        self.check_failure(symbols=SYMBOLS.replace("500000a0", "50000010"), match="tile")
        self.check_failure(sections=SECTIONS.replace("000000d0", "000000cc"), match="reviewed")

    def test_canonical_sd_record_is_required_and_unchanged(self):
        self.check_failure(symbols=SYMBOLS.replace("retainedSd", "other"), match="140-byte")
        self.check_failure(symbols=SYMBOLS.replace("0000008c", "00000088"), match="140-byte")
        self.check_failure(symbols=SYMBOLS.replace(".rtc_noinit\t0000008c", ".rtc.bss\t0000008c"), match="140-byte")

    def test_records_must_exactly_tile_noinit(self):
        self.check_failure(symbols=SYMBOLS.replace("500000a0", "500000a4"), match="tile")
        self.check_failure(symbols=SYMBOLS + SYMBOLS.splitlines()[0] + "\n", match="140-byte")



    def test_retired_home_token_is_rejected(self):
        old = "500000e4  w    O .rtc_noinit\t00000004 newt::firmware::retainedHome\n"
        with self.assertRaisesRegex(RuntimeError, "Retired Home RTC"):
            guard.validate_retained(DEV_SYMBOLS + old, DEV_SECTIONS)

    def test_dev_marker_shape_is_pinned(self):
        for changed in (DEV_SYMBOLS.replace(" w    O .dram0.bss", " l    O .dram0.bss"),
                        DEV_SYMBOLS.replace(".dram0.bss\t00000001", ".dram0.data\t00000001"),
                        DEV_SYMBOLS.replace(".dram0.bss\t00000001", ".dram0.bss\t00000002")):
            with self.assertRaisesRegex(RuntimeError, "identity marker"):
                guard.validate_retained(changed, DEV_SECTIONS)

    def test_rtc_section_set_sizes_and_aggregate_are_pinned(self):
        extra = SECTIONS + "  5 .rtc.extra    00000000  50000104  50000104  001a0104  2**0\n                  CONTENTS\n"
        with self.assertRaisesRegex(RuntimeError, "section set"):
            guard.validate_retained(SYMBOLS, extra)
        for changed in (SECTIONS.replace(".rtc.text     00000014", ".rtc.text     00000018"),
                        SECTIONS.replace(".rtc.force_slow 00000020", ".rtc.force_slow 00000024"),
                        SECTIONS.replace(".rtc_reserved 00000028", ".rtc_reserved 0000002c")):
            with self.assertRaisesRegex(RuntimeError, "reviewed"):
                guard.validate_retained(SYMBOLS, changed)


class RustSleepGuardTest(unittest.TestCase):
    def functions(self):
        native = function("nb_try_sleep", ENTRY_CALLS[:3]) + "\n42000120: 8082 ret\n"
        outer = function(guard.RUST_SLEEP, [guard.RUST_TRY, guard.RUST_STAMP, guard.RUST_STORE, guard.RUST_RESTART, "abort"])
        stamp = STAMP.replace(guard.STAMP_SLEEP, guard.RUST_STAMP)
        store = function(guard.RUST_STORE, [guard.RUST_WRITE, "nsc_flush"])
        writer = function(guard.RUST_WRITE, ["nb_retained_words"]) + "\n" + "\n".join(
            f"42000108: 00052023 sw a1,{offset}(a0)" for offset in range(0, 68, 4))
        restart = function("nb_restart", ["esp_restart"])
        board_try = function(guard.RUST_TRY, ["nb_try_sleep"])
        board_restart = function(guard.RUST_RESTART, ["nb_restart"])
        return guard.parse_functions("\n".join((native, outer, stamp, store, restart, board_try, board_restart, writer)))

    def check(self, functions=None, symbols=None):
        symbols = symbols or SYMBOLS.replace("::retainedSd", "::sdResumeWords").replace("::retained", "::sleepWords")
        return guard.validate_rust_sleep_boundary(functions or self.functions(), symbols, SECTIONS)

    def test_actual_abi_names_keep_rtc_layout_and_required_edges(self):
        result = self.check()
        self.assertEqual(result["retained"]["bytes"], 68)
        self.assertEqual(result["rtc_retention"]["bytes"], 300)
        self.assertEqual(result["rejection_stamp_word_offsets"], [28, 64])

    def test_missing_duplicate_and_reordered_stamp_fail(self):
        for calls in ([guard.RUST_TRY, guard.RUST_STORE, guard.RUST_RESTART],
                      [guard.RUST_TRY, guard.RUST_STAMP, guard.RUST_STAMP, guard.RUST_STORE, guard.RUST_RESTART],
                      [guard.RUST_STAMP, guard.RUST_TRY, guard.RUST_STORE, guard.RUST_RESTART]):
            functions = self.functions()
            functions[guard.RUST_SLEEP] = guard.parse_functions(function(guard.RUST_SLEEP, calls))[guard.RUST_SLEEP]
            with self.assertRaisesRegex(RuntimeError, "required native call"):
                self.check(functions)

    def test_native_early_return_or_outer_branch_cannot_bypass_stamp(self):
        for name, extra in (("nb_try_sleep", "42000100: 8082 ret"),
                            (guard.RUST_SLEEP, "42000100: c119 beqz a0,42000118 <skip>")):
            functions = self.functions()
            functions[name].insert(0, extra)
            with self.assertRaisesRegex(RuntimeError, "branch bypass"):
                self.check(functions)

    def test_hidden_termination_in_sdk_leaf_fails(self):
        functions = self.functions()
        functions["nb_try_sleep"].append("42000122: 000000ef jal 42000400 <abort>")
        with self.assertRaisesRegex(RuntimeError, "unexpected call"):
            self.check(functions)

    def test_resolved_long_tail_calls_pass_but_unresolved_or_wrong_targets_fail(self):
        functions = self.functions()
        for name, target in (("nb_try_sleep", "esp_deep_sleep_try_to_start"),
                             (guard.RUST_TRY, "nb_try_sleep"), (guard.RUST_RESTART, "nb_restart")):
            functions[name] = [line.replace("jal 42000400 <" + target + ">", "jr 440(t1) # 403816f6 <" + target + ">")
                               for line in functions[name] if " ret" not in line]
        self.check(functions)
        for name in ("nb_try_sleep", guard.RUST_TRY, guard.RUST_RESTART):
            broken = {key: list(lines) for key, lines in functions.items()}
            broken[name][-1] = "42000124: 00030067 jr t1"
            with self.assertRaisesRegex(RuntimeError, "unresolved|forward only|required native call"):
                self.check(broken)
        functions[guard.RUST_TRY].insert(0, "420000fc: 000000ef jal 42000400 <abort>")
        with self.assertRaisesRegex(RuntimeError, "forward only"):
            self.check(functions)

    def test_post_restart_panic_is_only_the_real_abort_chain(self):
        functions = self.functions()
        functions[guard.RUST_SLEEP][-1] = "42000124: 000000ef jal 42000400 <__rustc::rust_begin_unwind>"
        functions.update(guard.parse_functions(function("__rustc::rust_begin_unwind", ["newt_app_image::stop"])
                                              + "\n" + function("newt_app_image::stop", ["abort"])))
        self.check(functions)
        functions["newt_app_image::stop"] = []
        with self.assertRaisesRegex(RuntimeError, "existing abort stop"):
            self.check(functions)

    def test_missing_checksum_store_or_rtc_flush_fails(self):
        functions = self.functions()
        functions[guard.RUST_STAMP] = [line.replace("64(s0)", "60(s0)") for line in functions[guard.RUST_STAMP]]
        with self.assertRaisesRegex(RuntimeError, "checksum words"):
            self.check(functions)
        functions = self.functions()
        functions[guard.RUST_STORE] = []
        with self.assertRaisesRegex(RuntimeError, "write its RTC record and flush"):
            self.check(functions)
        functions = self.functions()
        functions[guard.RUST_WRITE] = [line for line in functions[guard.RUST_WRITE] if "64(a0)" not in line]
        with self.assertRaisesRegex(RuntimeError, "all 68 record bytes"):
            self.check(functions)


if __name__ == "__main__":
    unittest.main()
