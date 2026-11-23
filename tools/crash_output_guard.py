"""Console output's gates in the linked image (rust/firmware/native/crash_output.h,
docs/UPDATES.md "Secrets in RAM"); verify-nvs-guard.py
runs it on every firmware build."""
from bisect import bisect_right
import hashlib
import re
import struct

from linked_code import native_instructions, ordered_calls, references
from crash_console_guard import CONSOLE_REGISTERS, decimal_power_ranges
from crash_console_guard import validate_console_registers as _validate_console_registers


# Console output's gates (rust/firmware/native/crash_output.h, docs/UPDATES.md).
# Every panic and abort reaches IDF's panic handler only through its wrap, the
# task watchdog's register printout only through esp_backtrace_print's, and
# ROM print only through the putc gate in the ROM's printf channel. Each gate
# loads one word and goes to the real function only when it holds
# READING_BOOT; otherwise the panic gate prints WITHHELD and restarts.
CRASH_WORD = "newt::firmware::crashOutput"
READING_BOOT = 0x52454144
WITHHELD = b"\r\nNewt: crash output withheld in this boot. Rebooting...\r\n\0"
PANIC_GATE, BACKTRACE_GATE, CONSOLE_GATE = "__wrap_esp_panic_handler", "__wrap_esp_backtrace_print", "newt_console_putc"
GATES = {PANIC_GATE: "esp_panic_handler", BACKTRACE_GATE: "esp_backtrace_print", CONSOLE_GATE: "esp_rom_output_putc"}
SETUP = "(anonymous namespace)::enterRequestedUpdateMode(esp_reset_reason_t)"
# Startup's first init step installs the putc gate (ESP_SYSTEM_INIT_FN).
INSTALL = "__esp_system_init_fn_newt_console_gate()"
INIT_ARRAY = "_esp_system_init_fn_array_start"
UPDATE_MODE = "(anonymous namespace)::runUpdateMode(newt::firmware::UpdateModeRequest)"
WITHHOLD = "newt::firmware::withholdConsole()"
ROM = range(0x40000000, 0x40060000)
# The console's VFS tables, each registered by one startup step under a path
# update mode unregisters (vfs_console.c, uart_vfs.c, usb_serial_jtag_vfs.c).
CONSOLE_TABLES = {"s_vfs_console": "esp_vfs_console_register", "s_vfs_uart": "esp_vfs_dev_uart_register",
                  "s_vfs_jtag": "__esp_system_init_fn_init_vfs_usj_sec"}
# Each protected function, or the word, and the exact functions that may refer
# to it. Inside the gates: the handler, its printers (run through info's state
# and details, which only the handler calls), abort details, the core dump
# writer down to its flash write (FENCED_REFERENCES pins that) and the
# panic's direct FIFO writer. A shared caller that can also run on its own is
# not in a set, so it fails. An empty set may also be absent from the image.
CRASH_REFERENCES = {
    PANIC_GATE: {"panic_handler"},
    "esp_panic_handler": {PANIC_GATE},
    BACKTRACE_GATE: {"esp_crosscore_isr", "task_wdt_isr"},
    "esp_backtrace_print": {BACKTRACE_GATE},
    CRASH_WORD: {SETUP, PANIC_GATE, BACKTRACE_GATE, CONSOLE_GATE},
    "print_state": {"panic_handler"},
    "print_abort_details": {"esp_panic_handler"},
    "panic_print_registers": {"print_state", "esp_backtrace_print"},
    "panic_print_backtrace": {"print_state"},
    "print_assist_debug_details": {"panic_soc_fill_info"},
    "print_memprot_err_details": {"panic_soc_fill_info"},
    "panic_print_hex": {"esp_panic_handler", "panic_print_backtrace", "panic_print_registers",
                        "print_assist_debug_details", "print_memprot_err_details"},
    "panic_print_dec": {"esp_panic_handler", "panic_print_registers"},
    # The re-entry check prints a fixed line before the gate, then restarts.
    "panic_print_str": {PANIC_GATE, "esp_panic_handler", "esp_panic_handler_increment_entry_count",
                        "panic_print_backtrace", "panic_print_registers", "print_abort_details",
                        "print_assist_debug_details", "print_cache_err_details", "print_memprot_err_details",
                        "print_state"},
    "panic_print_char": {"panic_print_str", "panic_print_hex", "panic_print_dec"},
    "esp_core_dump_write": {"esp_panic_handler"},
    "esp_core_dump_write_elf_and_check": {"esp_core_dump_write"},
    "esp_core_dump_store": {"esp_core_dump_write_elf_and_check"},
    "esp_core_dump_do_write_elf_pass": {"esp_core_dump_store"},
    "esp_core_dump_write_prepare": {"esp_core_dump_store"},
    "esp_core_dump_write_end": {"esp_core_dump_store"},
    "esp_core_dump_write_data": {"elf_add_note", "elf_add_segment.constprop.0", "elf_process_note_segment",
                                 "elf_write_core_dump_note_cb", "elf_write_note_header",
                                 "esp_core_dump_do_write_elf_pass", "esp_core_dump_store"},
    "esp_core_dump_flash_custom_write": {"esp_core_dump_write_data", "esp_core_dump_write_end"},
    "elf_add_note": {"elf_add_regs", "esp_core_dump_do_write_elf_pass"},
    "elf_add_regs": {"esp_core_dump_do_write_elf_pass"},
    "elf_add_segment.constprop.0": {"elf_save_task", "esp_core_dump_do_write_elf_pass"},
    "elf_process_note_segment": {"esp_core_dump_do_write_elf_pass"},
    "elf_save_task": {"esp_core_dump_do_write_elf_pass"},
    "elf_write_core_dump_note_cb": {"esp_core_dump_do_write_elf_pass"},
    "elf_write_note_header": {"elf_add_note", "esp_core_dump_do_write_elf_pass"},
    # ROM print: only Newt's first init step names the ROM's channel installs
    # (the link sends every other caller to an empty wrap), and only the gate
    # reaches the console writer. Nothing else installs, prints around the
    # gate or keeps IDF's own copy of the channels.
    CONSOLE_GATE: {INSTALL},
    "ets_install_putc1": {INSTALL},
    "ets_install_putc2": {INSTALL},
    "esp_rom_output_putc": {CONSOLE_GATE},
    "esp_rom_output_tx_one_char": {"esp_rom_output_putc"},
    "ets_install_uart_printf": set(),
    "uart_tx_one_char2": set(),
    "usb_uart_tx_one_char": set(),
    "esp_rom_install_channel_putc": set(),
    "esp_rom_install_uart_printf": set(),
    "esp_rom_output_to_channels": set(),
    "esp_rom_vprintf": set(),
    # The ROM's other console writers (esp32c3.rom.ld): the download
    # protocol's packet sender. uart_tx_one_char is esp_rom_output_tx_one_char.
    "send_packet": set(),
    "UartDwnLdProc": set(),
    # The UART and USB serial drivers never start, so their writers return
    # without transmitting (Newt's DEV bridge is Arduino's HWCDC, no driver).
    "uart_driver_install": set(),
    "usb_serial_jtag_driver_install": set(),
    # The console's VFS tables: only their startup steps register them (and
    # vfs_console.c's select reads two), so after update mode unregisters
    # their paths nothing reaches them again.
    "s_vfs_console": {"esp_vfs_console_register"},
    "s_vfs_uart": {"esp_vfs_dev_uart_register", "esp_vfs_uart_get_vfs"},
    "s_vfs_jtag": {"__esp_system_init_fn_init_vfs_usj_sec", "esp_vfs_usb_serial_jtag_get_vfs"},
    "esp_vfs_uart_get_vfs": {"esp_vfs_console_register"},
    "esp_vfs_usb_serial_jtag_get_vfs": {"esp_vfs_console_register"},
    "esp_vfs_console_register": {"__esp_system_init_fn_init_vfs_console"},
    "esp_vfs_dev_uart_register": {"__esp_system_init_fn_init_vfs_uart"},
}
# The console's hardware (soc/reg_base.h, uart_reg.h, usb_serial_jtag_reg.h:
# each register file ends at +0x80). Each function that builds an address in
# one, and each object that holds one, is reviewed: why it cannot put RAM on
# the console in update mode.
_HWCDC = "Arduino's USB serial (HWCDC): Newt's Serial and DEV bridge, Newt's lines only"
_VFS = "a console VFS driver; update mode unregisters its path first"
_PAD = "pad, clock or interrupt setup; no data"
_GLYPHS = "glyph or run data equal to a register address by chance; never loaded as a pointer"
CONSOLE_REGISTER_CODE = {
    "HWCDC::begin(unsigned long)": _HWCDC, "HWCDC::end()": _HWCDC, "HWCDC::flush()": _HWCDC,
    "HWCDC::isCDC_Connected()": _HWCDC, "HWCDC::write(unsigned char const*, unsigned int)": _HWCDC,
    "flushTXBuffer(unsigned char const*, unsigned int)": _HWCDC, "hw_cdc_isr_handler(void*)": _HWCDC,
    "newt::firmware::UsbChannel::wakeTransmitter(unsigned int)": _HWCDC,
    "cdc0_write_char(char)": "ROM putc adapter; only setDebugOutput installs it, wrapped into nothing",
    "uart0_write_char": "ROM putc adapter; only uart_install_putc installs it, wrapped into nothing",
    "panic_print_char": "the panic printer; its callers are pinned behind the panic gate",
    "usb_serial_jtag_tx_char_no_driver": _VFS, "usb_serial_jtag_rx_char_no_driver": _VFS,
    "usb_serial_jtag_wait_tx_done_no_driver": _VFS,
    "log_printfv": "waits for UART0 to idle, then prints through ROM print's gate",
    "uartAvailableForWrite": "reads UART0's FIFO count", "uartFlushTxOnly": "waits for UART0's FIFO to drain",
    "usb_serial_jtag_sof_tick_hook": "reads the frame counter", "usb_serial_jtag_ll_ena_intr_mask": _PAD,
    "sleep_console_usj_pad_backup_and_disable": _PAD, "sleep_console_usj_pad_restore": _PAD,
    "esp_sleep_isolate_digital_gpio": _PAD, "esp_sleep_start.constprop.0": _PAD,
    "esp_system_reset_modules_on_exit": _PAD, "system_early_init": _PAD, "gpio_config": _PAD,
    "gpio_config_as_analog": _PAD, "gpio_func_sel": _PAD, "gpio_hal_iomux_in": _PAD, "gpio_hal_iomux_out": _PAD,
    "gpio_output_disable": _PAD, "gpio_output_enable": _PAD, "gpio_pullup_dis": _PAD,
    "ip6_output_if_src": "lwIP's IPv6 header word: version 6 << 28 equals UART0's base; a value, never an address",
}
# Call frame information: the unwinder reads its ranges and rules. The only
# word it calls through is a CIE's personality routine, which the compiler
# sets (__gxx_personality_v0), never a crash output function. Its opcode bytes
# (advance_loc 2, restore_state: 42 0b) can spell a code address at 0x420bxxxx.
UNWIND_SECTIONS = {".eh_frame"}
# Files the linker embeds (EMBED_FILES, _binary_<name>_start to _end): bytes parsed in place.
EMBEDDED_DATA = {"x509_crt_bundle": "mbedTLS's DER certificate bundle; its text can spell a code address"}
# Rust glyph data seen to spell a protected address, each pinned to its
# reviewed size and SHA-256: changed bytes fail until reviewed again. The Rust
# Sunset icon's runs spelled esp_vfs_dev_uart_register+0x2c (#26).
PINNED_GLYPHS = {
    "newt_device_ui::assets::kSunsetRuns": (368, "5e708a9c9c2bdcc26e4c472acb387ad8c5c8242478b4fae2253341216f3dae56"),
}
CONSOLE_REGISTER_DATA = {
    "s_panic_uart": "the panic printer's UART",
    "uart_context": "the UART driver's context; its install is not linked",
    "s_context": "/dev/uart's context; update mode unregisters the path",
    "newt::kHomeKerns": _GLYPHS, "newt::kHomeRuns": _GLYPHS, "newt::(anonymous namespace)::kReaderItalicBitmap": _GLYPHS,
    **{f"{o}::kReader{n}Bitmap": _GLYPHS for o in ("newt::(anonymous namespace)", "newt_reader_atlas::data") for n in ("Added", "BoldExtra")},
    **{f"newt::(anonymous namespace)::k{n}Runs": _GLYPHS for n in ("Mug", "Sun", "Sunset", "Moon", "ChristmasHat")},
    **dict.fromkeys(PINNED_GLYPHS, _GLYPHS),
    # Unnamed, so named by its input section in firmware.map.
    ".gcc_except_table": "C++ exception tables: the unwinder's call-site bytes, never loaded as a pointer",
}


def symbol_table(symbols: str) -> list[tuple[int, str, str, int, str]]:
    """objdump -t: (address, flags, section, size, name) per symbol."""
    found = (re.match(r"^([0-9a-fA-F]{8}) (.{7}) (\S+)\s+([0-9a-fA-F]+)\s+(.+)$", line) for line in symbols.splitlines())
    return [(int(symbol[1], 16), symbol[2], symbol[3], int(symbol[4], 16), symbol[5]) for symbol in found if symbol]


def section_headers(sections: str) -> list[tuple[str, int, int, int, set[str]]]:
    """objdump -h: (name, address, size, file offset, flags) of each allocated section."""
    result, lines = [], sections.splitlines()
    for index, line in enumerate(lines[:-1]):
        section = re.match(r"^\s*\d+\s+(\S+)\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)\s", line)
        flags = {flag.strip() for flag in lines[index + 1].split(",")}
        if section and "ALLOC" in flags:
            result.append((section[1], int(section[3], 16), int(section[2], 16), int(section[4], 16), flags))
    return result


def loaded_sections(sections: str, elf: bytes) -> list[tuple[str, int, bytes, set[str]]]:
    """(name, address, contents, flags) of each allocated section with contents."""
    return [(name, start, elf[offset:offset + size], flags)
            for name, start, size, offset, flags in section_headers(sections) if "CONTENTS" in flags]


def section_of(address, headers):
    return next((name for name, start, size, *_ in headers if start <= address < start + size), None)


def branch_target(operands: str) -> int | None:
    """The address a branch or jump names, resolved by objdump."""
    target = re.search(r"\b([0-9a-fA-F]+) <", operands)
    return int(target[1], 16) if target else None


def targets(line: str) -> list[int]:
    """Every address objdump resolved in one instruction: calls, jumps,
    branches, loads and address builds, with any +offset kept."""
    return [int(address, 16) for address in re.findall(r"\b([0-9a-fA-F]+) <[^>]", line.split(":", 1)[-1])]


def resolved(lines) -> list[tuple[str, str, str, list[int]]]:
    """(opcode, operands, line, addresses) per instruction. objdump resolves a
    lui only through the next instruction reading its register (a load past a
    branch, as in an epilogue, only sets it), and names nothing when the low
    part is zero (mv a0,a0; jr a5): then the lui's page is the address."""
    code = [[opcode, operands, line, targets(line)] for opcode, operands, line in native_instructions(lines)]
    for index, (opcode, operands, _, _) in enumerate(code):
        if opcode != "lui":
            continue
        register, page = operands.split(",")
        use = next((entry for entry in code[index + 1:] if re.search(rf"\b{register}\b", entry[1] if "," not in entry[1]
                    or entry[0] in ("sb", "sh", "sw") or entry[0][0] in "bj" else entry[1].split(",", 1)[1])), None)
        if use is None or "#" not in use[2]:
            code[index][3].append(int(page, 16) << 12)
    return [tuple(entry) for entry in code]


class Owners:
    """Which protected function or word an address lies in."""

    def __init__(self, ranges: dict[str, tuple[int, int]]):
        self.ranges = sorted((start, end, name) for name, (start, end) in ranges.items())
        self.starts = [start for start, *_ in self.ranges]

    def at(self, address):
        index = bisect_right(self.starts, address) - 1
        if index >= 0 and address < self.ranges[index][1]:
            return self.ranges[index][2], address - self.ranges[index][0]
        return None, 0


def validate_gate(gate, lines, loaded):
    """Pin each gate's reviewed shape: load the word, build READING_BOOT, bne
    to the withheld path, and fall through to one tail jump to the real
    function, which nothing else enters or names."""
    real = GATES[gate]
    code = [(int(line.split(":", 1)[0], 16), opcode, operands, line)
            for opcode, operands, line in native_instructions(lines)]
    shape = RuntimeError(f"{gate} does not reach {real} only when {CRASH_WORD} holds the reading-boot value")
    if len(code) < 7:
        raise shape
    base = re.fullmatch(r"(\w+),0x[0-9a-f]+", code[0][2])
    load = re.fullmatch(r"(\w+),-?\d+\((\w+)\)", code[1][2])
    high = re.fullmatch(r"(\w+),0x([0-9a-f]+)", code[2][2])
    low = re.fullmatch(r"(\w+),(\w+),(-?\d+)", code[3][2])
    if not (code[0][1] == "lui" and base and code[1][1] == "lw" and load and load[2] == base[1]
            and f"<{CRASH_WORD}>" in code[1][3] and code[2][1] == "lui" and high and code[3][1] == "addi"
            and low and low[1] == low[2] == high[1] != load[1]
            and ((int(high[2], 16) << 12) + int(low[3])) & 0xFFFFFFFF == READING_BOOT
            and code[4][1] == "bne" and code[4][2].split(",")[:2] in ([load[1], high[1]], [high[1], load[1]])):
        raise shape
    # Equal falls through to the real function: j, or auipc then jr.
    end = 5 if code[5][1] == "j" else 6
    if end == 6 and (code[5][1], code[6][1]) != ("auipc", "jr"):
        raise shape
    if not code[end][3].rstrip().endswith(f"<{real}>") or branch_target(code[4][2]) != code[end + 1][0]:
        raise shape
    equal = {address for address, *_ in code[5:end + 1]}
    if real in references(line for *_, line in code[:5] + code[end + 1:]):
        raise shape
    if CRASH_WORD in references(line for *_, line in code[:1] + code[2:]):
        raise shape
    if any(branch_target(operands) in equal for _, opcode, operands, _ in code
           if opcode.startswith("b") or opcode in {"j", "jal"}):
        raise shape
    withheld = [(opcode, operands) for _, opcode, operands, _ in code[end + 1:]]
    if gate == BACKTRACE_GATE:
        if withheld != [("li", "a0,-1"), ("ret", "")]:
            raise shape
        return {"withheld": "returns ESP_FAIL"}
    if gate == CONSOLE_GATE:
        if withheld != [("ret", "")]:
            raise shape
        return {"withheld": "drops the character"}
    # The panic gate never returns: it prints the fixed line, keeps the
    # handler's reset reason and restarts as the handler does.
    withheld = code[end + 1:]
    calls = ordered_calls([line for *_, line in withheld])
    if calls != ["panic_print_str", "esp_reset_reason_get_hint", "esp_reset_reason_set_hint", "panic_restart"]:
        raise shape
    if any(opcode in {"ret", "jr", "jalr"} and "<" not in line for _, opcode, _, line in withheld):
        raise shape
    # Reuse the reviewed resolver: an aligned address ends in mv, without an objdump comment.
    address = call_arguments([line for *_, line in withheld])[0][1][0]
    if not isinstance(address, int):
        raise shape
    text = next((contents[address - start:address - start + len(WITHHELD)] for _, start, contents, _ in loaded
                 if start <= address < start + len(contents)), b"")
    if text != WITHHELD:
        raise shape
    return {"withheld": "prints the fixed line, sets the panic reset reason and calls panic_restart"}


def validate_cache_safe(name, lines, start, headers):
    """ROM print runs with the cache off: the code is IRAM and names no flash
    code or data; it jumps only within IRAM or into ROM."""
    unsafe = RuntimeError(f"ROM print's path through {name} is not cache-safe")
    if not (section_of(start, headers) or "").startswith(".iram0"):
        raise unsafe
    for opcode, _, _, addresses in resolved(lines):
        for target in addresses:
            section = section_of(target, headers) or ""
            if section.startswith(".flash"):
                raise unsafe
            if (opcode.startswith("b") or opcode in {"j", "jal", "jalr", "jr"}) and not (
                    section.startswith(".iram0") or (not section and target in ROM)):
                raise unsafe


def call_arguments(lines, registers=("a0",)):
    """(callee, arguments) per call: the address last built into each
    register (lui, then addi, or mv when its low part is zero), "zero" or
    "unknown"."""
    arguments, calls = {}, []
    for opcode, operands, line, addresses in resolved(lines):
        called = ordered_calls([line])
        register = operands.split(",", 1)[0]
        if called:
            calls.append((called[0], tuple(arguments.get(name) for name in registers)))
            arguments.clear()  # a call leaves its argument registers unknown
        elif register in registers and opcode not in {"sw", "sh", "sb"}:
            if (opcode, operands) in {("li", f"{register},0"), ("mv", f"{register},zero")}:
                arguments[register] = "zero"
            elif opcode in {"lui", "addi"} and addresses:
                arguments[register] = addresses[-1]
            elif (opcode, operands) != ("mv", f"{register},{register}"):
                arguments[register] = "unknown"
    return calls


def text_at(address, loaded):
    """The C string a loaded section holds at an address."""
    for _, start, contents, _ in loaded:
        if isinstance(address, int) and start <= address < start + len(contents):
            return contents[address - start:contents.index(b"\0", address - start)].decode(errors="replace")
    return None


def validate_install(lines, gate, table, loaded):
    """Startup's first init step, run on core 0 at the CORE stage: channel 1
    gets the gate, channel 2 nothing."""
    shape = RuntimeError(f"{INSTALL} must be startup's first step and install only {CONSOLE_GATE}")
    if call_arguments(lines) != [("ets_install_putc1", (gate,)), ("ets_install_putc2", ("zero",))]:
        raise shape
    array = [entry[0] for entry in table if entry[4] == INIT_ARRAY]
    first = next((contents[array[0] - start:array[0] - start + 8] for _, start, contents, _ in loaded
                  if array and start <= array[0] < start + len(contents)), b"")
    install = [entry[0] for entry in table if entry[4] == INSTALL]
    # (function, core mask, stage): IDF skips an entry whose mask lacks core 0.
    if len(first) != 8 or len(install) != 1 or struct.unpack("<IHH", first) != (install[0], 1, 0):
        raise shape
    return {"channel_1": CONSOLE_GATE, "channel_2": None, "init_step": "first, core 0, CORE stage"}


def validate_update_entry(linked, loaded, update_mode=UPDATE_MODE, withhold_entry=WITHHOLD):
    """Update mode's first call sends esp_log, stdout and stderr nowhere and
    unregisters every path a console VFS table is registered under."""
    shape = RuntimeError(f"{UPDATE_MODE} must first send esp_log, stdout and stderr nowhere and close every "
                         f"console path ({WITHHOLD})")
    calls = [(call, text_at(path, loaded)) for call, (path,) in call_arguments(linked.get(WITHHOLD, []))
             if call in {"esp_log_level_set", "freopen", "esp_vfs_unregister"}]
    closed = [path for call, path in calls if call == "esp_vfs_unregister"]
    registered = sorted(text_at(path, loaded) or "unknown" for registrar in CONSOLE_TABLES.values()
                        for call, (path,) in call_arguments(linked.get(registrar, [])) if call == "esp_vfs_register_fs")
    if (ordered_calls(linked.get(update_mode, []))[:1] != [withhold_entry]
            or withhold_entry != WITHHOLD and ordered_calls(linked.get(withhold_entry, [])) != [WITHHOLD]
            or [call for call, _ in calls] != ["esp_log_level_set", "freopen", "freopen"] + ["esp_vfs_unregister"] * 3
            or len(registered) != len(CONSOLE_TABLES) or sorted(closed) != registered):
        raise shape
    return {"first_call": withhold_entry, "withholds": [call for call, _ in calls], "closed_paths": closed}



def validate_console_registers(linked, table, loaded, link_map, code=CONSOLE_REGISTER_CODE,
                               data=CONSOLE_REGISTER_DATA, literals=(), numbers=()):
    """Keep the C++ defaults while sharing Rust's register and numeric checks."""
    return _validate_console_registers(linked, table, loaded, link_map, code, data,
                                       literals=literals, numbers=numbers)


def pinned_glyphs(name, address, size, loaded):
    """A pinned object must still hold exactly its reviewed bytes."""
    if name not in PINNED_GLYPHS:
        return
    data = next((contents[address - start:address - start + size] for _, start, contents, _ in loaded
                 if start <= address and address + size <= start + len(contents)), b"")
    if (len(data), hashlib.sha256(data).hexdigest()) != PINNED_GLYPHS[name]:
        raise RuntimeError(f"Reviewed glyph data changed: {name}; review its bytes and pin them again")


def validate_crash_output(functions, symbols, sections, elf, link_map, reviewed=CRASH_REFERENCES,
                          registers=(CONSOLE_REGISTER_CODE, CONSOLE_REGISTER_DATA),
                          update_entry=(UPDATE_MODE, WITHHOLD), literals=(), rust_app=False):
    table = symbol_table(symbols)
    words = [entry for entry in table if entry[4] == CRASH_WORD]
    if len(words) != 1 or words[0][3] != 4 or not words[0][2].endswith("bss"):
        raise RuntimeError(f"{CRASH_WORD} must be one zero-initialized 4-byte word, so every boot starts withheld")
    ranges, absolute, objects = {}, set(), set()
    for name, expected in reviewed.items():
        found = [entry for entry in table if entry[4] == name]
        if not found and not expected:
            continue
        if len(found) != 1:
            raise RuntimeError(f"Protected crash output symbol must be defined once: {name}")
        address, flags, section, size, _ = found[0]
        ranges[name] = (address, address + max(size, 1))
        if section == "*ABS*":
            absolute.add(name)
        if "O" in flags:
            objects.add(name)
    owners = Owners(ranges)
    # Another name at the same address is the same function: objdump may
    # label it, or name it in a reference, by either.
    starts = {start: name for name, (start, _) in ranges.items()}
    aliases = {entry[4]: starts[entry[0]] for entry in table if entry[0] in starts and entry[4] != starts[entry[0]]
               and (entry[3] or starts[entry[0]] in absolute)}
    linked: dict[str, list[str]] = {}
    for label, lines in functions.items():
        first = next((int(line.split(":", 1)[0], 16) for line in lines if re.match(r"^\s*[0-9a-fA-F]+:", line)), None)
        name = owners.at(first)[0] if first is not None and owners.at(first)[1] == 0 else None
        linked.setdefault(name or aliases.get(label, label), []).extend(lines)
    for name in ranges.keys() - absolute - objects - linked.keys():
        raise RuntimeError(f"Crash output gate or protected function is not linked: {name}")
    # Every resolved address counts, offsets kept: entering a gate anywhere
    # but its first instruction skips its check.
    referrers: dict[str, set[str]] = {}
    for caller, lines in linked.items():
        for *_, addresses in resolved(lines):
            for target in addresses:
                name, offset = owners.at(target)
                if not name or name == caller:
                    continue
                if name in GATES and offset:
                    raise RuntimeError(f"Crash output reached outside its gate: {caller} enters {name} at "
                                       f"+0x{offset:x}, not its first instruction")
                referrers.setdefault(name, set()).add(caller)
    for name, expected in reviewed.items():
        if referrers.get(name, set()) != expected:
            raise RuntimeError(f"Crash output reached outside its gate: {name} is referred to by "
                               f"{sorted(referrers.get(name, set()))}, not the reviewed {sorted(expected)}")
    # A stored address is a reference too: a callback table any code could run, and one inside a protected
    # function enters past its start. Glyph and run bytes may equal one by chance, as they may a console
    # register (reviewed as _GLYPHS in the register data), an embedded file's (EMBEDDED_DATA) and an exception
    # table's (.gcc_except_table, placed by the map: call-site offsets and typeinfo, never code); a word wholly
    # inside them is never a pointer. So may call frame bytes (UNWIND_SECTIONS).
    headers = section_headers(sections)
    loaded = loaded_sections(sections, elf)
    numbers = decimal_power_ranges(table, loaded) if rust_app else []
    glyphs = []
    for address, flags, _, size, name in table:
        if size and "O" in flags and registers[1].get(name) == _GLYPHS:
            pinned_glyphs(name, address, size, loaded)
            glyphs.append((address, address + size))
    at = {name: address for address, _, _, _, name in table}
    glyphs += [(at[f"_binary_{n}_start"], at[f"_binary_{n}_end"]) for n in EMBEDDED_DATA if f"_binary_{n}_end" in at]
    glyphs += [(int(m[1], 16), int(m[1], 16) + int(m[2], 16)) for m in
               re.finditer(r"^ \.gcc_except_table\S*\s+0x([0-9a-fA-F]+)\s+0x([0-9a-fA-F]+)\s", link_map, re.M)]
    glyphs += numbers
    for section, start, contents, flags in loaded:
        if "CODE" in flags or section in UNWIND_SECTIONS:
            continue
        for offset in range(-start % 4, len(contents) - 3, 4):
            name, inside = owners.at(struct.unpack_from("<I", contents, offset)[0])
            address = start + offset
            literal = (bool(name) and any(low <= address and address + 4 <= high for low, high in literals)
                       and not any(at < address + 4 and address < at + size for at, flags, _, size, _ in table
                                   if size and "O" in flags))
            if name and not literal and not any(low <= address and address + 4 <= high for low, high in glyphs):
                where = f" +0x{inside:x}" if inside else ""
                raise RuntimeError(f"Crash output reached outside its gate: {section}+0x{offset:x} "
                                   f"stores the address of {name}{where}")
    gates = {gate: validate_gate(gate, linked[gate], loaded) for gate in GATES}
    for name in (CONSOLE_GATE, GATES[CONSOLE_GATE]):
        validate_cache_safe(name, linked[name], ranges[name][0], headers)
    install = validate_install(linked.get(INSTALL, []), ranges[CONSOLE_GATE][0], table, loaded)
    return {"word": CRASH_WORD, "reading_boot": f"0x{READING_BOOT:08x}", "gates": gates,
            "rom_print": install, "update_mode": (None if update_entry is None else validate_update_entry(linked, loaded, *update_entry)),
            "console_registers": validate_console_registers(linked, table, loaded, link_map, *registers,
                                                             literals=literals, numbers=numbers),
            "merged_string_ranges": literals,
            "checked_decimal_power_ranges": numbers,
            "referrers": {name: sorted(callers) for name, callers in reviewed.items()},
            "aliases": {alias: name for alias, name in sorted(aliases.items())}, "data_references": 0,
            "limits": "Resolved code addresses, console register addresses built by lui or auipc, and "
                      "4-byte-aligned stored addresses in loaded data sections; an address computed at run time "
                      "from other values is not seen, and the reviewed functions' own control flow is trusted "
                      "as pinned IDF."}
