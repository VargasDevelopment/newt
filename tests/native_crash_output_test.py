"""Fault controls for the linked image keeping console output behind its
gates (rust/firmware/native/crash_output.h, docs/UPDATES.md "Secrets in
RAM"). Each fixture is a real RISC-V link by the pinned toolchain: the
actual gate source, compiled, and IDF's panic path, ROM print channels and
Arduino's channel installs as small assembly objects, laid out like the
firmware (IRAM, DRAM, flash, the sorted startup steps) and linked with the
firmware's --wrap options. Each control breaks one rule."""
import hashlib
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import crash_output_guard as guard
from linked_code import parse_functions

BIN = ROOT / ".toolchain/platformio/packages/toolchain-riscv32-esp/bin"
GATE = (ROOT / "rust/firmware/native/crash_output.h").read_text()
SETUP = """#include "crash_output.h"
namespace newt::firmware { enum class UpdateModeRequest : unsigned char { None, Update }; }
namespace {
[[gnu::noipa]] void enterRequestedUpdateMode(esp_reset_reason_t) {
  newt::firmware::crashOutput = newt::firmware::kReadingBoot;
}
[[gnu::noipa]] void runUpdateMode(newt::firmware::UpdateModeRequest) { newt::firmware::withholdConsole(); }
}  // namespace
extern "C" void setup() { enterRequestedUpdateMode(ESP_RST_SW); runUpdateMode(newt::firmware::UpdateModeRequest::Update); }
"""
WRAPS = ("esp_panic_handler", "esp_backtrace_print", "ets_install_putc1", "ets_install_putc2")
# The ROM's jump table entries the firmware links against (esp32c3.rom.ld).
ROM = {"ets_install_putc1": 0x40000044, "ets_install_putc2": 0x4000004C, "esp_rom_output_tx_one_char": 0x40000068,
       "uart_tx_one_char": 0x40000068, "send_packet": 0x400000A8, "usb_uart_tx_one_char": 0x400008D8}
LAYOUT = """SECTIONS {
  . = 0x40380000; .iram0.text : { *(.iram1 .iram1.*) }
  . = 0x3fc80000; .dram0.data : { *(.data .data.* .sdata .sdata.*) }
  .dram0.bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.* COMMON) }
  . = 0x3c000000; .flash.rodata : {
    _esp_system_init_fn_array_start = ABSOLUTE(.);
    KEEP(*(SORT_BY_INIT_PRIORITY(.esp_system_init_fn.*)))
    *(.rodata .rodata.* .srodata .srodata.* .gcc_except_table .gcc_except_table.*) }
  . = 0x42000000; .flash.text : { *(.text .text.*) }
}
"""


def function(name, *body, local=False):
    head = "" if local else f".globl {name}\n"
    return (f"{head}.type {name},@function\n{name}:\n" + "".join(f"  {line}\n" for line in body)
            + f"  ret\n.size {name}, .-{name}\n")


def table(name, *words, section=".section .rodata"):
    """A named object of words, as objdump -t lists IDF's tables."""
    return (f"{section}\n.p2align 2\n.type {name},@object\n{name}:\n" + "".join(f"  .word {word}\n" for word in words)
            + f".size {name}, .-{name}\n.text\n")


def init_step(name, priority):
    return f'.section .esp_system_init_fn.{priority},"aw"\n.p2align 2\n  .word {name}\n  .half 1, 0\n.text\n'


def registrar(name, path, ops, *before, local=False):
    """A console VFS table and the startup code registering it at a path."""
    label = name.replace("_", "") + "Path"
    return (f'.section .rodata\n{label}:\n  .asciz "{path}"\n.text\n'
            + function(name, *before, f"lla a0, {label}", f"lla a1, {ops}", "tail esp_vfs_register_fs", local=local))


IRAM = '.section .iram1.rom,"ax",@progbits\n'
# IDF's and Arduino's objects, as the pinned image links them: --wrap redirects
# only calls that leave an object, so each lives in its own, like panic_handler.c.
OBJECTS = {
    "panic_handler": function("panicHandler", "tail panic_handler")
    + function("panic_handler", "lla a5, print_state", "call esp_panic_handler", local=True)
    + function("print_state", "call panic_print_registers", "call panic_print_backtrace", local=True),
    "panic": function("esp_panic_handler", "lla a5, print_abort_details", "call esp_core_dump_write",
                      "call panic_restart")
    + function("print_abort_details", "tail panic_print_str", local=True)
    + function("panic_print_str", "tail panic_print_char") + function("panic_restart")
    # The panic's own FIFO writer: reviewed console register code and data.
    + function("panic_print_char", "lla a5, s_panic_uart", "lw a5, 0(a5)", "lui a4, 0x60043", "sw a0, 0(a4)",
               "sw a0, 0(a5)", local=True)
    + table("s_panic_uart", "0x60000000", section=".data"),
    "panic_arch": function("panic_print_registers") + function("panic_print_backtrace"),
    "system": function("esp_reset_reason_get_hint", "li a0, 0") + function("esp_reset_reason_set_hint"),
    "core_dump": function("esp_core_dump_write", "call esp_core_dump_write_data")
    + function("esp_core_dump_write_data", "call esp_core_dump_flash_custom_write")
    # IDF's alias of the writer: a reference by either name counts.
    + ".globl esp_core_dump_flash_write_data\n.type esp_core_dump_flash_write_data,@function\n"
    + ".set esp_core_dump_flash_write_data, esp_core_dump_write_data\n.size esp_core_dump_flash_write_data, 4\n"
    + function("esp_core_dump_flash_custom_write"),
    "debug_helpers": function("esp_backtrace_print", "call panic_print_registers"),
    "task_wdt": function("task_wdt_isr", "call esp_backtrace_print"),
    # esp_rom_sys.c: IDF's console putc, in IRAM.
    "esp_rom_sys": IRAM + function("esp_rom_output_putc", "tail esp_rom_output_tx_one_char"),
    # HWCDC::setDebugOutput: the link turns its installs into nothing.
    "hwcdc": function("setDebugOutput", "lla a0, cdc0_write_char", "call ets_install_putc2", "li a0, 0",
                      "tail ets_install_putc1") + function("cdc0_write_char", local=True),
    # An earlier IDF startup step (init_efuse_check's priority 1).
    "efuse": function("efuse_check", "li a0, 0", local=True)
    + '.section .esp_system_init_fn.1,"aw"\n.p2align 2\n  .word efuse_check\n  .half 1, 0\n',
    # newlib and esp_log, as the update mode entry calls them.
    "libc": function("freopen") + function("esp_log_level_set") + function("__getreent", "lla a0, reent")
    + ".bss\n.p2align 2\nreent:\n  .zero 64\n.text\n",
    # IDF's VFS and the console's three tables, each registered by a startup
    # step (vfs_console.c, uart_vfs.c, usb_serial_jtag_vfs.c).
    "vfs": function("esp_vfs_register_fs") + function("esp_vfs_unregister"),
    "vfs_console": table("s_vfs_console", "consoleWrite") + function("consoleWrite", local=True)
    + registrar("esp_vfs_console_register", "/dev/console", "s_vfs_console", "call esp_vfs_uart_get_vfs",
                "call esp_vfs_usb_serial_jtag_get_vfs")
    + function("__esp_system_init_fn_init_vfs_console", "tail esp_vfs_console_register", local=True)
    + init_step("__esp_system_init_fn_init_vfs_console", 114),
    "uart_vfs": table("s_vfs_uart", "uartWrite") + function("uartWrite", local=True)
    + function("esp_vfs_uart_get_vfs", "lla a0, s_vfs_uart")
    + registrar("esp_vfs_dev_uart_register", "/dev/uart", "s_vfs_uart")
    + function("__esp_system_init_fn_init_vfs_uart", "tail esp_vfs_dev_uart_register", local=True)
    + init_step("__esp_system_init_fn_init_vfs_uart", 112),
    "usj_vfs": table("s_vfs_jtag", "jtagWrite") + function("jtagWrite", local=True)
    + function("esp_vfs_usb_serial_jtag_get_vfs", "lla a0, s_vfs_jtag")
    + registrar("__esp_system_init_fn_init_vfs_usj_sec", "/dev/secondary", "s_vfs_jtag", local=True)
    + init_step("__esp_system_init_fn_init_vfs_usj_sec", 113),
    "reader": function("readerTask"),
}
REVIEWED = {
    guard.PANIC_GATE: {"panic_handler"},
    "esp_panic_handler": {guard.PANIC_GATE},
    guard.BACKTRACE_GATE: {"task_wdt_isr"},
    "esp_backtrace_print": {guard.BACKTRACE_GATE},
    guard.CRASH_WORD: {guard.SETUP, guard.PANIC_GATE, guard.BACKTRACE_GATE, guard.CONSOLE_GATE},
    "print_state": {"panic_handler"},
    "print_abort_details": {"esp_panic_handler"},
    "panic_print_registers": {"print_state", "esp_backtrace_print"},
    "panic_print_backtrace": {"print_state"},
    "esp_core_dump_write": {"esp_panic_handler"},
    "esp_core_dump_write_data": {"esp_core_dump_write"},
    "esp_core_dump_flash_custom_write": {"esp_core_dump_write_data"},
    "panic_print_char": {"panic_print_str"},
    **{name: guard.CRASH_REFERENCES[name] for name in (
        guard.CONSOLE_GATE, "ets_install_putc1", "ets_install_putc2", "esp_rom_output_putc",
        "esp_rom_output_tx_one_char", "usb_uart_tx_one_char", "esp_rom_output_to_channels", "send_packet",
        "s_vfs_console", "s_vfs_uart", "s_vfs_jtag", "esp_vfs_uart_get_vfs", "esp_vfs_usb_serial_jtag_get_vfs",
        "esp_vfs_console_register", "esp_vfs_dev_uart_register")},
}
REGISTERS = ({"panic_print_char": "the panic printer"},
             {"s_panic_uart": "the panic printer's UART",
              ".gcc_except_table": guard.CONSOLE_REGISTER_DATA[".gcc_except_table"]})
OUTSIDE = "Crash output reached outside its gate"


def tool(name, *arguments):
    return subprocess.run([str(BIN / f"riscv32-esp-elf-{name}"), *map(str, arguments)], check=True,
                          capture_output=True, text=True).stdout


class Fixture:
    """One linked image: the gate source and IDF's objects, each editable."""

    def __init__(self, scratch: Path, gate=GATE, objects=None, setup=SETUP, wraps=WRAPS):
        self.elf = scratch / "fixture.elf"
        (scratch / "crash_output.h").write_text(gate)
        (scratch / "setup.cpp").write_text(setup)
        (scratch / "layout.ld").write_text(LAYOUT)
        flags = ["-march=rv32imc_zicsr_zifencei", "-mabi=ilp32", "-Os", "-fno-exceptions", "-fno-rtti"]
        sources = [scratch / "setup.cpp"]
        for name, code in (OBJECTS if objects is None else objects).items():
            (scratch / f"{name}.S").write_text(".text\n" + code)
            sources.append(scratch / f"{name}.S")
        tool("g++", *flags, "-std=gnu++17", "-iquote", scratch, "-I", ROOT / "tests/crash_output_shim",
             "-nostdlib", "-nostartfiles", "-Wl,-e,setup", "-T", scratch / "layout.ld",
             *[f"-Wl,--defsym={name}=0x{address:x}" for name, address in ROM.items()],
             # Unwrapped, the gate's own install names the ROM's directly.
             *[f"-Wl,--defsym=__real_{name}={name}" for name in WRAPS[2:] if name not in wraps],
             *[f"-Wl,--wrap={name}" for name in wraps], f"-Wl,-Map={self.elf.with_name('firmware.map')}",
             *sources, "-o", self.elf)

    def patch(self, function, opcode, edit):
        """Rewrite the first `opcode` instruction of a linked function."""
        lines = parse_functions(tool("objdump", "-d", "-C", self.elf))[function]
        address = next(int(line.split(":")[0], 16) for line in lines
                       if re.match(rf"^\s*[0-9a-f]+:\s+[0-9a-f]+\s+{opcode}\s", line))
        offset = next(offset + address - start for _, start, size, offset, _ in
                      guard.section_headers(tool("objdump", "-h", self.elf)) if start <= address < start + size)
        image = bytearray(self.elf.read_bytes())
        image[offset:offset + 4] = edit(int.from_bytes(image[offset:offset + 4], "little")).to_bytes(4, "little")
        self.elf.write_bytes(bytes(image))

    def inputs(self):
        return (parse_functions(tool("objdump", "-d", "-C", self.elf)), tool("objdump", "-t", "-C", self.elf),
                tool("objdump", "-h", self.elf), self.elf.read_bytes(), self.elf.with_name("firmware.map").read_text())

    def check(self):
        return guard.validate_crash_output(*self.inputs(), REVIEWED, REGISTERS)


class DecimalPowerImageMode(unittest.TestCase):
    """Image-mode controls use linked-data fixtures without invoking a compiler."""

    def test_pinned_glyphs_must_hold_their_reviewed_bytes(self):
        runs = bytes(range(16))
        loaded = [(".rodata", 0x3c000000, bytes(8) + runs, {"READONLY", "CONTENTS", "ALLOC"})]
        with mock.patch.object(guard, "PINNED_GLYPHS", {"kRuns": (16, hashlib.sha256(runs).hexdigest())}):
            guard.pinned_glyphs("kRuns", 0x3c000008, 16, loaded)
            guard.pinned_glyphs("kOther", 0x3c000000, 4, loaded)
            for address, size in ((0x3c000007, 16), (0x3c000008, 15), (0x3c000010, 16)):
                with self.assertRaisesRegex(RuntimeError, "Reviewed glyph data changed: kRuns"):
                    guard.pinned_glyphs("kRuns", address, size, loaded)

    def check(self, contents, *, rust_app=False, protected=None):
        table = [(0x3fc80000, "g     O", ".dram0.bss", 4, guard.CRASH_WORD),
                 (0x3c000000, "g     O", ".rodata", len(contents), "__mprec_tens")]
        functions = {}
        for index, name in enumerate((*guard.GATES, guard.GATES[guard.CONSOLE_GATE])):
            address = 0x40380000 + index * 4
            table.append((address, "g     F", ".text", 4, name))
            functions[name] = [f"{address:08x}: 00008067 ret"]
        reviewed = {name: set() for name in functions}
        if protected is not None:
            table.append((protected, "g     F", ".text", 1, "protected_printer"))
            functions["protected_printer"] = [f"{protected:08x}: 00008067 ret"]
            reviewed["protected_printer"] = set()
        loaded = [(".rodata", 0x3c000000, contents, {"READONLY", "CONTENTS", "ALLOC"})]
        image_mode = {"rust_app": True} if rust_app else {}
        with (mock.patch.object(guard, "symbol_table", return_value=table),
              mock.patch.object(guard, "section_headers", return_value=[]),
              mock.patch.object(guard, "loaded_sections", return_value=loaded),
              mock.patch.object(guard, "validate_gate", return_value={}),
              mock.patch.object(guard, "validate_cache_safe"),
              mock.patch.object(guard, "validate_install", return_value={})):
            return guard.validate_crash_output(functions, "", "", b"", "", reviewed, ({}, {}),
                                                update_entry=None, **image_mode)

    def test_only_rust_exempts_verified_decimal_bytes_equal_to_a_protected_address(self):
        import struct
        values = struct.pack("<25d", *(float(f"1e{power}") for power in range(25)))
        # The high word of 1e10 is also a possible linked function address.
        protected = int.from_bytes(values[84:88], "little")
        with self.assertRaisesRegex(RuntimeError, "stores the address of protected_printer"):
            self.check(values, protected=protected)
        receipt = self.check(values, protected=protected, rust_app=True)
        self.assertEqual(receipt["checked_decimal_power_ranges"], [(0x3c000000, 0x3c0000c8)])

    def test_cpp_does_not_validate_or_exempt_a_named_decimal_table(self):
        self.assertEqual(self.check(bytes(4))["checked_decimal_power_ranges"], [])
        with self.assertRaisesRegex(RuntimeError, "Console registers reached by unreviewed data"):
            self.check((0x60000000).to_bytes(4, "little"))
        with self.assertRaisesRegex(RuntimeError, "exact read-only"):
            self.check(bytes(4), rust_app=True)


class CrashOutputGuard(unittest.TestCase):
    def link(self, gate=GATE, setup=SETUP, wraps=WRAPS, **objects):
        scratch = tempfile.TemporaryDirectory(prefix="newt-crash-output-")
        self.addCleanup(scratch.cleanup)
        return Fixture(Path(scratch.name), gate, {**OBJECTS, **objects}, setup, wraps)

    def reject(self, fixture, message):
        with self.assertRaisesRegex(RuntimeError, message):
            fixture.check()

    def test_gated_image_passes(self):
        receipt = self.link().check()
        self.assertEqual(receipt["referrers"]["esp_panic_handler"], [guard.PANIC_GATE])
        self.assertEqual(receipt["referrers"]["ets_install_putc1"], [guard.INSTALL])
        self.assertEqual(receipt["aliases"], {"esp_core_dump_flash_write_data": "esp_core_dump_write_data",
                                              "uart_tx_one_char": "esp_rom_output_tx_one_char"})
        self.assertIn("panic_restart", receipt["gates"][guard.PANIC_GATE]["withheld"])
        self.assertEqual(receipt["gates"][guard.CONSOLE_GATE]["withheld"], "drops the character")
        self.assertEqual(receipt["update_mode"]["first_call"], guard.WITHHOLD)
        self.assertEqual(receipt["update_mode"]["closed_paths"], ["/dev/console", "/dev/uart", "/dev/secondary"])
        self.assertEqual(receipt["console_registers"], {"code": ["panic_print_char"], "data": ["s_panic_uart"]})

    def test_page_aligned_withheld_literal_uses_the_same_gate(self):
        literal = r'"\r\nNewt: crash output withheld in this boot. Rebooting...\r\n"'
        aligned = ('alignas(4096) static const char alignedWithheld[] = ' + literal + ';\n'
                   + GATE.replace('panic_print_str(' + literal + ')', 'panic_print_str(alignedWithheld)'))
        fixture = self.link(aligned)
        lines = parse_functions(tool("objdump", "-d", "-C", fixture.elf))[guard.PANIC_GATE]
        address = next(args[0] for callee, args in guard.call_arguments(lines) if callee == "panic_print_str")
        self.assertIsInstance(address, int)
        self.assertEqual(address % 4096, 0)
        self.assertIn(("mv", "a0,a0"), [(op, args) for op, args, _ in guard.native_instructions(lines)])
        fixture.check()
        self.reject(self.link(aligned.replace("Newt: crash", "Newt: other")), "only when")
        self.reject(self.link(aligned.replace("panic_print_str(alignedWithheld)",
                                             "panic_print_str(info->reason)")), "only when")

    def test_shared_caller_of_the_dump_writer_is_rejected(self):
        # Astra's P2-3: readerTask runs on its own, though the dump also calls it.
        self.reject(self.link(core_dump=OBJECTS["core_dump"].replace(
            "call esp_core_dump_write_data", "call esp_core_dump_write_data\n  call readerTask"),
            reader=function("readerTask", "call esp_core_dump_flash_custom_write")),
            r"esp_core_dump_flash_custom_write is referred to by \['esp_core_dump_write_data', 'readerTask'\]")
        self.reject(self.link(reader=function("readerTask", "call esp_core_dump_flash_write_data")),
                    r"esp_core_dump_write_data is referred to by \['esp_core_dump_write', 'readerTask'\]")

    def test_stored_callback_is_rejected(self):
        # Astra's P2-4: a .rodata table holding the writer, jumped through.
        for section in (".section .rodata", ".data"):
            table = f"{section}\n.p2align 2\nwriters:\n  .word esp_core_dump_flash_custom_write\n.text\n"
            self.reject(self.link(reader=table + function("readerTask", "lla a5, writers", "lw a5, 0(a5)", "jalr a5")),
                        rf"{OUTSIDE}: \.[a-z0-9.]+\+0x[0-9a-f]+ stores the address of esp_core_dump_flash_custom_write$")

    def test_decimal_power_table_requires_exact_numeric_bytes_and_read_only_storage(self):
        import struct
        values = struct.pack("<25d", *(float(f"1e{power}") for power in range(25)))
        symbol = (0x3c000000, "g     O", ".rodata", len(values), "__mprec_tens")
        loaded = [(".rodata", symbol[0], values, {"READONLY", "CONTENTS", "ALLOC"})]
        self.assertEqual(guard.decimal_power_ranges([symbol], loaded), [(symbol[0], symbol[0] + 200)])
        bad_values = values[:84] + (0x42000000).to_bytes(4, "little") + values[88:]
        for table, data in (([symbol], [(".rodata", symbol[0], bad_values, loaded[0][3])]),
                            ([symbol], [(".rodata", symbol[0], values, {"CONTENTS", "ALLOC"})]),
                            ([symbol, symbol], loaded),
                            ([(symbol[0], symbol[1], symbol[2], 196, symbol[4])], loaded)):
            with self.assertRaisesRegex(RuntimeError, "exact read-only"):
                guard.decimal_power_ranges(table, data)

    def test_merged_literal_never_exempts_a_named_callback(self):
        fixture = self.link(reader=table("writers", "esp_core_dump_flash_custom_write") + function("readerTask"))
        address, _, _, size, _ = next(entry for entry in guard.symbol_table(fixture.inputs()[1]) if entry[4] == "writers")
        with self.assertRaisesRegex(RuntimeError, "stores the address of esp_core_dump_flash_custom_write"):
            guard.validate_crash_output(*fixture.inputs(), REVIEWED, REGISTERS, literals=[(address, address + size)])

    def test_glyph_bytes_equal_to_an_address_pass(self):
        # A run table's bytes may spell a protected address by chance; the
        # same word one past its end is still a stored address.
        runs = "_ZN4newt9kHomeRunsE"
        reviewed = (REGISTERS[0], {**REGISTERS[1], "newt::kHomeRuns": guard._GLYPHS})
        reader = function("readerTask", f"lla a5, {runs}", "lw a5, 0(a5)")
        fixture = self.link(reader=table(runs, "esp_core_dump_flash_custom_write+1") + reader)
        guard.validate_crash_output(*fixture.inputs(), REVIEWED, reviewed)
        fixture = self.link(reader=table(runs, "0") + ".section .rodata\n  .word esp_core_dump_flash_custom_write\n.text\n"
                            + reader)
        with self.assertRaisesRegex(RuntimeError, rf"{OUTSIDE}: .* stores the address of esp_core_dump_flash_custom_write$"):
            guard.validate_crash_output(*fixture.inputs(), REVIEWED, reviewed)

    def test_embedded_file_bytes_equal_to_an_address_pass(self):
        # The certificate bundle's DER bytes may spell a protected address by
        # chance; the same word one past its end is still a stored address.
        bundle = (".section .rodata\n.p2align 2\n.globl _binary_x509_crt_bundle_start\n_binary_x509_crt_bundle_start:\n"
                  "  .word esp_core_dump_flash_custom_write+1\n.globl _binary_x509_crt_bundle_end\n"
                  "_binary_x509_crt_bundle_end:\n")
        self.link(reader=bundle + ".text\n" + function("readerTask")).check()
        self.reject(self.link(reader=bundle + "  .word esp_core_dump_flash_custom_write\n.text\n" + function("readerTask")),
                    rf"{OUTSIDE}: .* stores the address of esp_core_dump_flash_custom_write$")

    def test_exception_table_bytes_equal_to_an_address_pass(self):
        # An exception table's call-site bytes may spell a protected address
        # (as ff_sd_initialize's did esp_panic_handler+0x1c5); the same word in
        # another unnamed section is still a stored address.
        words = "\n.p2align 2\n  .word esp_core_dump_flash_custom_write+1\n.text\n"
        self.link(reader='.section .gcc_except_table.readerTask,"a"' + words + function("readerTask")).check()
        self.reject(self.link(reader='.section .rodata.readerTask,"a"' + words + function("readerTask")),
                    rf"{OUTSIDE}: .* stores the address of esp_core_dump_flash_custom_write \+0x1$")

    def test_call_frame_bytes_equal_to_an_address_pass(self):
        # .eh_frame's ranges and opcode bytes may spell a protected address
        # (here one planted past a function's start); unreviewed, the section
        # is refused.
        fixture = self.link(reader=function("readerTask", ".cfi_startproc", "nop", ".cfi_endproc"))
        _, start, size, offset, _ = next(header for header in guard.section_headers(tool("objdump", "-h", fixture.elf))
                                         if header[0] == ".eh_frame")
        address = next(entry[0] for entry in guard.symbol_table(tool("objdump", "-t", "-C", fixture.elf))
                       if entry[4] == "esp_core_dump_flash_custom_write") + 4
        at = offset + ((start + size - 4) & ~3) - start
        image = bytearray(fixture.elf.read_bytes())
        image[at:at + 4] = address.to_bytes(4, "little")
        fixture.elf.write_bytes(bytes(image))
        fixture.check()
        with mock.patch.object(guard, "UNWIND_SECTIONS", set()):
            self.reject(fixture, rf"{OUTSIDE}: \.eh_frame\+0x[0-9a-f]+ stores the address of ")

    def test_gate_entered_past_its_check_is_rejected(self):
        # Astra's round-2 P2-2: the real panic_handler call patched to the
        # gate's tail, and a .rodata callback to a gate's tail.
        self.reject(self.link(panic_handler=OBJECTS["panic_handler"].replace(
            "call esp_panic_handler", f"call {guard.PANIC_GATE}+4")),
            rf"panic_handler enters {guard.PANIC_GATE} at \+0x4, not its first instruction")
        for gate in (guard.PANIC_GATE, guard.CONSOLE_GATE):
            table = f".section .rodata\n.p2align 2\ntail:\n  .word {gate}+8\n.text\n"
            self.reject(self.link(reader=table + function("readerTask", "lla a5, tail", "lw a5, 0(a5)", "jalr a5")),
                        rf"stores the address of {gate} \+0x8")

    def test_gate_branch_must_be_real_and_polarized(self):
        # Astra's P2-5: a nop'd branch, and an inverted one, both fall through.
        for gate in (guard.PANIC_GATE, guard.CONSOLE_GATE):
            nop = self.link()
            nop.patch(gate, "bne", lambda _: 0x00000013)
            self.reject(nop, f"{gate} does not reach {guard.GATES[gate]} only when")
        inverted = self.link()
        inverted.patch(guard.BACKTRACE_GATE, "bne", lambda word: word & ~(1 << 12))  # bne -> beq
        self.reject(inverted, "does not reach esp_backtrace_print only when")

    def test_gate_source_faults_are_rejected(self):
        check = "  if (newt::firmware::crashOutput == newt::firmware::kReadingBoot) return __real_esp_panic_handler(info);"
        putc = "  if (newt::firmware::crashOutput == newt::firmware::kReadingBoot) esp_rom_output_putc(c);"
        for old, new, message in (
                (check, "  return __real_esp_panic_handler(info);", rf"{guard.CRASH_WORD} is referred to by"),
                (check, check.replace("== newt::firmware::kReadingBoot", "!= 0u"), "only when"),
                (check, check + "\n  panic_print_str(info->reason);", "only when"),
                # The putc gate without its permission branch, or out of IRAM.
                (putc, "  esp_rom_output_putc(c);", rf"{guard.CRASH_WORD} is referred to by"),
                (putc, putc.replace("== newt::firmware::kReadingBoot", "!= 0u"), "only when"),
                ("IRAM_ATTR void newt_console_putc", "void newt_console_putc",
                 f"ROM print's path through {guard.CONSOLE_GATE} is not cache-safe")):
            self.reject(self.link(GATE.replace(old, new)), message)
        self.reject(self.link(GATE.replace("crashOutput = 0;", "crashOutput = kReadingBoot;")), "zero-initialized")
        self.reject(self.link(GATE.replace('"\\r\\nNewt: crash', '"\\r\\nNewt: %s crash')), "only when")

    def test_unwrapped_call_is_rejected(self):
        # --wrap misses a call inside the object that defines the function.
        objects = dict(OBJECTS, panic=OBJECTS["panic"] + OBJECTS["panic_handler"])
        del objects["panic_handler"]
        scratch = tempfile.TemporaryDirectory(prefix="newt-crash-output-")
        self.addCleanup(scratch.cleanup)
        self.reject(Fixture(Path(scratch.name), GATE, objects),
                    rf"{guard.PANIC_GATE} is referred to by \[\], not the reviewed \['panic_handler'\]")

    def test_abort_path_around_the_gate_is_rejected(self):
        # Abort details reach the console only through the real handler.
        self.reject(self.link(reader=function("abort", "call __real_esp_panic_handler")),
                    r"esp_panic_handler is referred to by \['__wrap_esp_panic_handler', 'abort'\]")
        self.reject(self.link(panic=OBJECTS["panic"] + function("abort", "lla a5, print_abort_details")),
                    r"print_abort_details is referred to by \['abort', 'esp_panic_handler'\]")

    def test_rom_print_around_the_putc_gate_is_rejected(self):
        # Without the link's wraps, Arduino's setDebugOutput installs its own putc.
        self.reject(self.link(wraps=WRAPS[:2]), r"ets_install_putc1 is referred to by \['.*setDebugOutput'\]")
        # A direct console writer, the ROM's USB writer, or IDF's own copy of
        # the channels (esp_rom_vprintf's path) skips the gate.
        self.reject(self.link(reader=function("readerTask", "li a0, 65", "call esp_rom_output_putc")),
                    r"esp_rom_output_putc is referred to by \['newt_console_putc', 'readerTask'\]")
        # The fixture's gate starts a page, so a lui alone builds its address.
        self.reject(self.link(reader=function("readerTask", f"lui a5, %hi({guard.CONSOLE_GATE})", "jr a5")),
                    rf"{guard.CONSOLE_GATE} is referred to by \['{re.escape(guard.INSTALL)}', 'readerTask'\]")
        self.reject(self.link(reader=function("readerTask", "li a0, 65", "call usb_uart_tx_one_char")),
                    r"usb_uart_tx_one_char is referred to by \['readerTask'\]")
        self.reject(self.link(esp_rom_sys=OBJECTS["esp_rom_sys"] + function(
            "esp_rom_output_to_channels", "lla a5, esp_rom_output_putc", "jr a5")),
            r"esp_rom_output_putc is referred to by \['esp_rom_output_to_channels', 'newt_console_putc'\]")

    def test_reload_before_a_page_use_is_not_the_use(self):
        # IDF's subscribe_idle: the early return reloads the lui's register
        # and the later addi names the real address, just below the gate's
        # page. Without a named use, the page still counts.
        below = f"{guard.CONSOLE_GATE}-4"
        self.link(reader=function("readerTask", f"lui s2, %hi({below})", "bnez a0, 1f", "lw s2, 0(sp)", "ret",
                                  f"1: addi a0, s2, %lo({below})")).check()
        self.reject(self.link(reader=function("readerTask", f"lui a5, %hi({guard.CONSOLE_GATE})", "lw a5, 0(sp)",
                                              "jr a5")),
                    rf"{guard.CONSOLE_GATE} is referred to by \['{re.escape(guard.INSTALL)}', 'readerTask'\]")
        # A one-operand jump reads its register, so it stays the use.
        self.reject(self.link(reader=function("readerTask", f"lui a5, %hi({guard.CONSOLE_GATE})", "jr a5",
                                              f"addi a0, a5, %lo({below})")),
                    rf"{guard.CONSOLE_GATE} is referred to by \['{re.escape(guard.INSTALL)}', 'readerTask'\]")

    def test_install_must_be_first_and_gate_only_channel_one(self):
        install = "  __real_ets_install_putc2(nullptr);"
        self.reject(self.link(GATE.replace(install, "  __real_ets_install_putc2(newt_console_putc);")),
                    "must be startup's first step and install only")
        self.reject(self.link(GATE.replace("CORE, BIT(0), 0)", "CORE, BIT(0), 2)")), "must be startup's first step")
        self.reject(self.link(GATE.replace("CORE, BIT(0), 0)", "SECONDARY, BIT(0), 0)")), "must be startup's first step")
        # Astra's round-3 P2-2: IDF skips an entry whose core mask is empty.
        self.reject(self.link(GATE.replace("CORE, BIT(0), 0)", "CORE, 0, 0)")), "must be startup's first step")

    def test_update_mode_must_first_withhold_logs(self):
        self.reject(self.link(setup=SETUP.replace("{ newt::firmware::withholdConsole(); }", "{}")),
                    "must first send esp_log, stdout and stderr nowhere")
        self.reject(self.link(GATE.replace('  std::freopen("/dev/null", "w", stderr);\n', "")),
                    "must first send esp_log, stdout and stderr nowhere")

    def test_update_mode_must_close_every_console_path(self):
        # Astra's round-3 P1-1: stdin, or a new descriptor, still reached the console.
        closing = "must first send esp_log, stdout and stderr nowhere and close every console path"
        for path in ("/dev/console", "/dev/uart", "/dev/secondary"):
            self.reject(self.link(GATE.replace(f'  esp_vfs_unregister("{path}");\n', "")), closing)
        self.reject(self.link(GATE.replace('"/dev/secondary"', '"/dev/usbserjtag"')), closing)
        # A table registered under a path update mode leaves open, or by new code.
        self.reject(self.link(uart_vfs=OBJECTS["uart_vfs"].replace('"/dev/uart"', '"/dev/tty"')), closing)
        self.reject(self.link(uart_vfs=OBJECTS["uart_vfs"] + function("reregister", "lla a1, s_vfs_uart",
                                                                      "tail esp_vfs_register_fs")),
                    r"s_vfs_uart is referred to by \['esp_vfs_dev_uart_register', 'esp_vfs_uart_get_vfs', 'reregister'\]")

    def test_console_registers_need_review(self):
        # Astra's round-3 P2-3: a RAM byte stored straight into the USB serial FIFO.
        self.reject(self.link(reader=function("readerTask", "lui a5, 0x60043", "lbu a4, 0(a0)", "sw a4, 0(a5)")),
                    r"unreviewed code: built by \['readerTask'\]")
        self.reject(self.link(reader=function("readerTask", "lui a5, 0x60000", "sw a0, 0(a5)")),
                    r"unreviewed code: built by \['readerTask'\]")
        # A register pointer table in .rodata or .data, named or not.
        for section in (".section .rodata", ".data"):
            self.reject(self.link(reader=table("consoleRegisters", "0x60043000", section=section)
                                  + function("readerTask", "lla a5, consoleRegisters", "lw a5, 0(a5)", "sw a0, 0(a5)")),
                        r"\(consoleRegisters\) holds a console register address")
        self.reject(self.link(reader=".section .rodata\n.p2align 2\n  .word 0x60000010\n.text\n" + function("readerTask")),
                    r"\(no object in \.rodata\) holds a console register address")

    def test_exception_tables_hold_register_words_by_chance(self):
        # A C++ exception table's call-site bytes can equal a register address;
        # the map names that unnamed word. The same word in an unnamed section
        # other than an exception table still fails.
        words = "\n.p2align 2\n  .word 0x60000004\n.text\n"
        receipt = self.link(reader='.section .gcc_except_table.readerTask,"a"' + words + function("readerTask")).check()
        self.assertEqual(receipt["console_registers"]["data"], [".gcc_except_table", "s_panic_uart"])
        self.reject(self.link(reader='.section .rodata.readerTask,"a"' + words + function("readerTask")),
                    r"\(no object in \.rodata\.readerTask\) holds a console register address")

    def test_new_rom_tx_caller_is_rejected(self):
        self.reject(self.link(reader=function("readerTask", "li a0, 65", "call uart_tx_one_char")),
                    r"esp_rom_output_tx_one_char is referred to by \['esp_rom_output_putc', 'readerTask'\]")
        self.reject(self.link(reader=function("readerTask", "call send_packet")),
                    r"send_packet is referred to by \['readerTask'\]")


class CrashGatePure(unittest.TestCase):
    def test_put_rx_requires_exactly_the_reviewed_builders(self):
        from crash_console_guard import rust_registers, validate_console_registers
        rx = {"nsc_put_rx_begin", "nsc_put_rx_end", "nsc_put_rx_read"}
        base = {"newt::firmware::UsbChannel::wakeTransmitter(unsigned int)": "existing TX review"}
        fonts = dict.fromkeys(("newt::kHomeKerns", "newt::kHomeRuns",
                               "newt_reader_atlas::data::kReaderAddedBitmap"), "existing font review")
        absent, original_data = rust_registers(base, fonts, {})
        self.assertFalse(rx & absent.keys())
        for present in (rx, {"nsc_put_rx_begin"}):
            code, data = rust_registers(base, fonts, present)
            self.assertEqual(code.keys() - absent.keys(), rx)
            self.assertEqual(code["nsc_wake_transmitter"], "existing TX review")
            self.assertEqual(data, original_data)
            self.assertEqual({code[name] for name in rx}, {
                "reviewed by Newt Lead, cross-family native review: Astra, 2026-11-24 (325bb44a, delta 7896ff2b); bulk-read rename #135"})
            linked = {name: ["42000000: 600437b7 lui a5,0x60043"] for name in code}
            validate_console_registers(linked, [], [], "", code, data)
            for name in rx:
                # Removing a reviewed entry or linked builder must still fail.
                with self.assertRaisesRegex(RuntimeError, "unreviewed code"):
                    validate_console_registers(linked, [], [], "", {k: v for k, v in code.items() if k != name}, data)
                with self.assertRaisesRegex(RuntimeError, "no longer by"):
                    validate_console_registers({k: v for k, v in linked.items() if k != name}, [], [], "", code, data)
            with self.assertRaisesRegex(RuntimeError, "unreviewed code"):
                validate_console_registers({**linked, "unreviewed_rx": linked["nsc_put_rx_begin"]},
                                           [], [], "", code, data)

    """Authored disassembly exercises the resolver without invoking a compiler."""

    def lines(self, aligned=True):
        literal = ["lui a0,0x3fc81", "mv a0,a0"] if aligned else [
            "lui a0,0x3fc81", "addi a0,a0,4 # 3fc81004 <withheld>"]
        instructions = [
            "lui a5,0x3fc80", f"lw a5,0(a5) # 3fc80000 <{guard.CRASH_WORD}>",
            "lui a4,0x52454", "addi a4,a4,324", "bne a5,a4,1018 <withheld_path>",
            "j 2000 <esp_panic_handler>", *literal,
            "jal 3000 <panic_print_str>", "jal 3010 <esp_reset_reason_get_hint>",
            "jal 3020 <esp_reset_reason_set_hint>", "j 3030 <panic_restart>",
        ]
        return [f"{0x1000 + index * 4:x}: 00000013 {instruction}" for index, instruction in enumerate(instructions)]

    def test_literal_resolver_retains_branch_order_and_exact_bytes(self):
        for aligned in (True, False):
            lines = self.lines(aligned)
            address = 0x3fc81000 + (0 if aligned else 4)
            loaded = [(".dram0.data", address, guard.WITHHELD, {"CONTENTS", "ALLOC"})]
            self.assertIn("withheld", guard.validate_gate(guard.PANIC_GATE, lines, loaded))
            controls = [
                [line.replace("bne a5,a4", "beq a5,a4") for line in lines],
                [line.replace("1018 <withheld_path>", "1014 <withheld_path>") for line in lines],
                [line.replace("<panic_print_str>", "<esp_reset_reason_get_hint>") for line in lines],
                [line.replace("mv a0,a0", "lw a0,0(a1)").replace(
                    "addi a0,a0,4 # 3fc81004 <withheld>", "lw a0,0(a1)") for line in lines],
                [*lines, "1030: 00008067 ret"],
            ]
            for broken in controls:
                with self.assertRaisesRegex(RuntimeError, "only when"):
                    guard.validate_gate(guard.PANIC_GATE, broken, loaded)
            for message in (guard.WITHHELD.replace(b"crash", b"other"), guard.WITHHELD[:-1]):
                with self.assertRaisesRegex(RuntimeError, "only when"):
                    guard.validate_gate(guard.PANIC_GATE, lines, [(loaded[0][0], address, message, loaded[0][3])])


if __name__ == "__main__":
    unittest.main()
