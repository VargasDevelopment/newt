#!/usr/bin/env python3
"""Verify guard reachability in the actual linked C3 image; no hardware access."""
import hashlib
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from crash_output_guard import (validate_crash_output, CRASH_REFERENCES, CRASH_WORD, SETUP,
                                CONSOLE_REGISTER_CODE, CONSOLE_REGISTER_DATA)
from crash_console_guard import rust_registers
from linked_code import native_instructions, ordered_calls, parse_functions, references
from native_output import owned_file, write_text
from rust_open_stack import validate_open_frames

ROOT = Path(__file__).resolve().parents[1]
ENTER_SLEEP = "newt::firmware::enterDeepSleep(newt::firmware::SleepRecord&)"
STAMP_SLEEP = "newt::firmware::stampSleepRejection(newt::firmware::SleepRecord&)"
RETAINED = "(anonymous namespace)::retained"
RETAINED_SD = "(anonymous namespace)::retainedSd"
RETAINED_HOME = "newt::firmware::retainedHome"
DEV_MARKER = "newt::firmware::readerStackObservation"
READER_STACK = "getArduinoLoopTaskStackSize()"
ROLLBACK_HOOK = "verifyRollbackLater"
TIMER_TASK = "xTimerCreateTimerTask"
# Entry points of Arduino's RGB LED and RMT code, which the X3 build leaves out.
RGB_PATH = ("rgbLedWrite", "rmtInit")
RTC_SECTION_BYTES = {
    ".rtc.text": 20,
    ".rtc.force_fast": 0,
    ".rtc_noinit": 208,
    ".rtc.force_slow": 32,
    ".rtc_reserved": 40,
}


def validate_loop_arguments(lines):
    registers = None
    for opcode, operands, line in native_instructions(lines):
        targets = ordered_calls([line])
        if opcode.startswith("b") or opcode in {"jr", "ret"}:
            raise RuntimeError("Unresolved control transfer can bypass the reader stack provider")
        if opcode in {"jal", "jalr", "j"} and not targets:
            raise RuntimeError("Unresolved control transfer in loop task creation")
        if opcode == "j" and targets != ["xTaskCreateUniversal"]:
            raise RuntimeError("Unresolved control transfer can bypass the reader stack provider")
        if targets == [READER_STACK]:
            registers = {"a0": "stack"}
            continue
        if registers is None:
            continue
        if targets == ["xTaskCreateUniversal"]:
            if registers.get("a2") == "stack" and registers.get("a0") == "loop":
                return
            raise RuntimeError("Reader stack override is not passed to the loop task")
        if targets or opcode.startswith("b") or opcode in {"j", "jr", "ret", "jal", "jalr"}:
            raise RuntimeError("Unresolved control transfer between reader stack and task creation")
        parts = operands.split(",")
        destination = parts[0]
        if opcode in {"sw", "sh", "sb", "nop"}:
            continue
        copied = registers.get(parts[1]) if opcode == "mv" and len(parts) == 2 else None
        registers.pop(destination, None)
        if copied:
            registers[destination] = copied
        if destination == "a0" and re.search(r"#\s+[0-9a-fA-F]+\s+<loopTask\(void\*\)>", line):
            registers[destination] = "loop"
    raise RuntimeError("Reader loop task creation edge is absent")


def validate_rollback_hook(functions, symbols):
    """Arduino confirms a pending image before setup() unless this hook returns
    true; Newt confirms after its first finished screen (docs/UPDATES.md, "Card install")."""
    definitions = [line for line in symbols.splitlines() if line.endswith(" " + ROLLBACK_HOOK)]
    if len(definitions) != 1 or not re.match(r"^[0-9a-fA-F]+\s+g\s+F\s+\S+\s+[0-9a-fA-F]+\s+", definitions[0]):
        raise RuntimeError("The rollback hook must have one strong Newt definition")
    instructions = [(op, args) for op, args, _ in native_instructions(functions.get(ROLLBACK_HOOK, []))]
    if instructions != [("li", "a0,1"), ("ret", "")]:
        raise RuntimeError("The rollback hook must defer confirmation to Newt's first finished screen")


def validate_no_timer_task(functions, symbols):
    """FreeRTOS's weak xTimerCreateTimerTask in tasks.c does nothing. Linking
    timers.c replaces it, and every boot then starts the timer task, whose 4 KiB
    stack and TCB come from the heap where no RAM ceiling sees them. Nothing in
    Newt uses FreeRTOS timers; Arduino's RGB LED path (RMT, event groups) pulled
    them in until -DESP32_ARDUINO_NO_RGB_BUILTIN."""
    definitions = [line for line in symbols.splitlines() if line.endswith(" " + TIMER_TASK)]
    instructions = [(op, args) for op, args, _ in native_instructions(functions.get(TIMER_TASK, []))]
    if (len(definitions) != 1 or not re.match(r"^[0-9a-fA-F]+\s+w\s+F\s+", definitions[0])
            or instructions != [("li", "a0,1"), ("ret", "")]):
        raise RuntimeError("FreeRTOS timers are linked: the timer task would hold heap in every boot")
    linked = [name for name in RGB_PATH if name in functions]
    if linked:
        raise RuntimeError(f"Arduino's RGB LED path is linked: {linked}")


# Wi-Fi never writes flash (docs/UPDATES.md). While Wi-Fi may run, the
# NVS guard's fence refuses partition writes and read-write NVS opens, so every
# flash write primitive and every NVS open must lie behind a fenced entry point.
FLASH_PRIMITIVES = (
    "esp_flash_write", "esp_flash_write_encrypted", "esp_flash_erase_region", "esp_flash_erase_chip",
    "spi_flash_write", "spi_flash_write_encrypted", "spi_flash_erase_range", "spi_flash_erase_sector",
    "esp_rom_spiflash_write", "esp_rom_spiflash_write_encrypted", "esp_rom_spiflash_erase_area",
    "esp_rom_spiflash_erase_sector", "esp_rom_spiflash_erase_block", "esp_rom_spiflash_erase_chip",
    "bootloader_flash_write", "bootloader_flash_erase_sector", "bootloader_flash_erase_range",
)
PARTITION_MUTATIONS = ("esp_partition_write", "esp_partition_write_raw", "esp_partition_erase_range")
# The panic handler's core dump writes only after the system has stopped
# (validate_crash_output keeps it behind the panic gate).
CORE_DUMP = ("esp_core_dump_flash_custom_write", "esp_core_dump_write_prepare")
NVS_OPEN_HANDLE = ("nvs::NVSPartitionManager::open_handle(char const*, char const*, nvs_open_mode_t, "
                   "nvs::NVSHandleSimple**)")
# Each symbol may be referenced only by these functions.
FENCED_REFERENCES = {
    "nvs_open": {"__wrap_nvs_open"},
    "nvs_open_from_partition": {"nvs_open"},
    NVS_OPEN_HANDLE: {"nvs_open_from_partition"},
    **{api: {"__wrap_" + api} for api in PARTITION_MUTATIONS},
    **{primitive: {*PARTITION_MUTATIONS, *CORE_DUMP} for primitive in FLASH_PRIMITIVES},
}
# The PHY's calibration store, which must meet the fence at its open.
PHY_STORE = "esp_phy_store_cal_data_to_nvs"


def validate_flash_writers(functions):
    referrers: dict[str, set[str]] = {name: set() for name in FENCED_REFERENCES}
    for caller, lines in functions.items():
        for name in references(lines) & referrers.keys():
            if name != caller:
                referrers[name].add(caller)
    for name, callers in referrers.items():
        unfenced = callers - FENCED_REFERENCES[name]
        if unfenced:
            raise RuntimeError(f"Flash write path bypasses the Wi-Fi fence: {sorted(unfenced)} -> {name}")
    if "nvs_open" not in references(functions.get("__wrap_nvs_open", [])):
        raise RuntimeError("Missing native guard edge: __wrap_nvs_open -> nvs_open")
    if PHY_STORE in functions and "__wrap_nvs_open" not in references(functions[PHY_STORE]):
        raise RuntimeError(f"{PHY_STORE} does not open NVS through the Wi-Fi fence")
    return {name: sorted(callers) for name, callers in referrers.items() if callers}


def validate_reader_stack(functions, symbols, *, rust_app=False):
    definitions = [line for line in symbols.splitlines() if line.endswith(READER_STACK)]
    if len(definitions) != 1 or not re.match(r"^[0-9a-fA-F]+\s+g\s+F\s+\S+\s+[0-9a-fA-F]+\s+", definitions[0]):
        raise RuntimeError("Reader loop stack must have one strong owned definition")
    instructions = [(op, args) for op, args, _ in native_instructions(functions.get(READER_STACK, []))]
    stack_bytes = 20480 if rust_app else 16384
    if instructions != [("lui", "a0,0x5" if rust_app else "a0,0x4"), ("ret", "")]:
        raise RuntimeError(f"Reader loop stack override must return exactly {stack_bytes} bytes")
    entry = functions.get("app_main", [])
    calls = ordered_calls(entry)
    if calls.count(READER_STACK) != 1 or calls.count("xTaskCreateUniversal") != 1:
        raise RuntimeError("Reader stack and task creation must occur once in app_main")
    if ordered_calls(entry, preserve_offsets=True) != ["setCpuFrequencyMhz", "initArduino", READER_STACK, "xTaskCreateUniversal"]:
        raise RuntimeError("Unresolved control transfer can bypass exact reader task entrypoints")
    validate_loop_arguments(entry)
    return {"symbol": READER_STACK, "strong_definition": True, "stack_bytes": stack_bytes,
            "additional_dynamic_bytes": stack_bytes - 8192, "creator": "app_main",
            "stack_argument": "a2", "task_entry": "loopTask(void*)",
            "limits": "Linked constant and task-argument flow only; runtime free heap, stack watermark and physical reading remain separate gates."}


def record_stores(instructions: list[str]) -> set[int]:
    """Find word stores through the incoming SleepRecord pointer or its copies."""
    aliases = {"a0"}
    stores = set()
    for line in instructions:
        instruction = re.match(r"^\s*[0-9a-fA-F]+:\s+[0-9a-fA-F]+\s+(\S+)\s*(.*)", line)
        if not instruction:
            continue
        opcode, operands = instruction.groups()
        operands = operands.split("#", 1)[0].strip()
        store = re.fullmatch(r"\w+,(-?\d+)\((\w+)\)", operands)
        if opcode == "sw" and store and store[2] in aliases:
            stores.add(int(store[1]))
        if opcode in {"jal", "jalr"}:
            aliases -= {"ra", *(f"a{i}" for i in range(8)), *(f"t{i}" for i in range(7))}
        elif opcode == "mv":
            destination, source = operands.split(",")
            if source in aliases:
                aliases.add(destination)
            else:
                aliases.discard(destination)
        elif opcode == "addi":
            destination, source, offset = operands.split(",")
            if source in aliases and offset == "0":
                aliases.add(destination)
            else:
                aliases.discard(destination)
        elif opcode not in {"sw", "sh", "sb", "j", "ret", "nop"} and not opcode.startswith("b"):
            aliases.discard(operands.split(",", 1)[0])
    return stores


def object_symbols(symbols: str) -> dict[str, list[tuple[int, str, int, str]]]:
    objects = {}
    for line in symbols.splitlines():
        symbol = re.match(r"^([0-9a-fA-F]+)\s+(\S+)\s+O\s+(\S+)\s+([0-9a-fA-F]+)\s+(.+)$", line)
        if symbol:
            objects.setdefault(symbol[5], []).append(
                (int(symbol[1], 16), symbol[3], int(symbol[4], 16), symbol[2]))
    return objects


def rtc_sections(sections: str) -> dict[str, tuple[int, int, set[str]]]:
    result = {}
    lines = sections.splitlines()
    for index, line in enumerate(lines):
        section = re.match(r"^\s*\d+\s+(\.rtc\S*)\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+", line)
        if section and index + 1 < len(lines):
            name = section[1]
            if name in result:
                raise RuntimeError(f"Duplicate linked RTC section: {name}")
            result[name] = (int(section[2], 16), int(section[3], 16),
                            {flag.strip() for flag in lines[index + 1].split(",")})
    return result


def validate_retained(symbols: str, sections: str, retained_name=RETAINED, sd_name=RETAINED_SD) -> dict:
    objects = object_symbols(symbols)
    marker = objects.get(DEV_MARKER, [])
    if len(marker) > 1 or (marker and marker[0][1:] != (".dram0.bss", 1, "w")):
        raise RuntimeError("DEV identity marker must be one weak one-byte DRAM object")
    development = bool(marker)
    expected = {retained_name: 68, sd_name: 140}

    records = {}
    intervals = []
    for name, size in expected.items():
        matches = objects.get(name, [])
        label = "68-byte" if name == RETAINED else f"{size}-byte"
        if len(matches) != 1 or matches[0][1:3] != (".rtc_noinit", size):
            raise RuntimeError(f"Newt retained record must be one {label} object in .rtc_noinit: {name}")
        address = matches[0][0]
        if address % 4:
            raise RuntimeError(f"Newt retained record must be word aligned: {name}")
        intervals.append((address, address + size, name))
        records[name] = {"symbol": name, "bytes": size, "section": ".rtc_noinit",
                         "section_flags": ["ALLOC"], "word_aligned": True,
                         "within_section": True}

    if objects.get(RETAINED_HOME):
        raise RuntimeError("Retired Home RTC token must not remain linked")

    measured = rtc_sections(sections)
    expected_sections = dict(RTC_SECTION_BYTES)
    if set(measured) != set(expected_sections):
        raise RuntimeError("Linked RTC section set differs from the reviewed C3 allocation")
    for name, size in expected_sections.items():
        if measured[name][0] != size:
            raise RuntimeError(f"Linked {name} size differs from the reviewed C3 allocation")
    noinit_size, noinit_address, noinit_flags = measured[".rtc_noinit"]
    if noinit_flags != {"ALLOC"}:
        raise RuntimeError("RTC noinit section must be ALLOC-only, without initialized contents")
    intervals.sort()
    if (intervals[0][0] != noinit_address or intervals[-1][1] != noinit_address + noinit_size or
            any(left[1] != right[0] for left, right in zip(intervals, intervals[1:]))):
        raise RuntimeError("Newt retained records must exactly tile RTC noinit without overlap, gaps or unrelated bytes")
    total = sum(size for size, _, _ in measured.values())
    expected_total = 300
    if total != expected_total:
        raise RuntimeError(f"Linked RTC aggregate must be the reviewed {expected_total}-byte allocation")

    return {"development": development, "bytes": total,
            "expected_bytes": expected_total,
            "sections": {name: measured[name][0] for name in expected_sections},
            "records": records, "retained": records[retained_name],
            "dev_token_store_proof": "not-linked"}


def validate_sleep_boundary(functions: dict[str, list[str]], symbols: str, sections: str) -> dict:
    terminal = {"esp_deep_sleep_start", "freeink::PowerManager::deepSleep()"}
    linked_symbols = {name for name in terminal for line in symbols.splitlines()
                      if re.search(r"\s" + re.escape(name) + r"\s*$", line)}
    linked_calls = {target for lines in functions.values() for target in ordered_calls(lines)}
    if terminal & (functions.keys() | linked_calls | linked_symbols):
        raise RuntimeError("Noreturn sleep API or obsolete SDK sleep boundary remains linked")
    if not {ENTER_SLEEP, STAMP_SLEEP} <= functions.keys():
        raise RuntimeError("Named Newt sleep entry and rejection stamp must both be linked")
    sequence = ["esp_sleep_config_gpio_isolate", "gpio_deep_sleep_hold_en",
                "esp_deep_sleep_try_to_start", STAMP_SLEEP, "esp_restart"]
    calls = ordered_calls(functions[ENTER_SLEEP])
    if any(calls.count(name) != 1 for name in sequence):
        raise RuntimeError("Newt sleep boundary is missing or duplicates a required native call")
    if [calls.index(name) for name in sequence] != sorted(calls.index(name) for name in sequence):
        raise RuntimeError("Native sleep rejection stamp must follow try entry and precede restart")
    # Address order proves the sequence only for a straight-line boundary. A
    # branch that skips the try or stamp must fail, rather than look ordered.
    for line in functions[ENTER_SLEEP]:
        instruction = re.match(r"^\s*[0-9a-fA-F]+:\s+[0-9a-fA-F]+\s+(\S+)\s*(.*)", line)
        if not instruction:
            continue
        opcode, operands = instruction.groups()
        if opcode in {"jal", "jalr", "j"} and not ordered_calls([line]):
            raise RuntimeError("Native sleep entry contains an unresolved control transfer")
        if opcode.startswith("b") or opcode in {"ret", "jr"} or (
                opcode == "j" and ordered_calls([line]) != ["esp_restart"]):
            raise RuntimeError("Native sleep entry must have no branch bypass around its required calls")
    stores = record_stores(functions[STAMP_SLEEP])
    if not {28, 64} <= stores:
        raise RuntimeError("Linked rejection stamp must store rejectedSleep and checksum words")
    rtc = validate_retained(symbols, sections)
    return {"entry": ENTER_SLEEP, "ordered_required_calls": sequence,
            "entry_calls": calls, "rejection_stamp_word_offsets": sorted(stores),
            "terminal_sleep_implementations_linked": False, "retained": rtc["retained"],
            "rtc_retention": rtc,
            "limits": "Compiled straight-line call order and retained-word stores only; not checksum correctness, successful sleep entry or physical sleep/wake proof."}


RUST_SLEEP = "newt_app_image::app_power::enter_prepared_sleep"
RUST_STAMP = "newt_app_image::retained_runtime::record::stamp_rejection"
RUST_STORE = "newt_app_image::app_power::store_rejection"
RUST_UPDATE = "newt_app_image::app_startup::run_update"
RUST_SELECTOR = "newt_native_input_owner::deferred_start::body"
RUST_TRY = "newt_c3_board::Board::try_sleep"
RUST_RESTART = "newt_c3_board::Board::restart"
RUST_WRITE = ("newt_app_image::retained_runtime::<impl "
              "newt_app_image::home_startup::HomeRuntime<L,N,P>>::write_retained_sleep")


def validate_rust_sleep_boundary(functions, symbols, sections):
    """Same native try/stamp/restart proof across Rust's explicit SDK leaves."""
    terminal = {"esp_deep_sleep_start", "freeink::PowerManager::deepSleep()"}
    linked = set(functions) | {target for lines in functions.values() for target in ordered_calls(lines)}
    if terminal & linked or any(re.search(r"\s" + re.escape(name) + r"\s*$", symbols, re.M) for name in terminal):
        raise RuntimeError("Noreturn sleep API or obsolete SDK sleep boundary remains linked")
    required = {RUST_SLEEP, RUST_STAMP, RUST_STORE, RUST_TRY, RUST_RESTART, "nb_try_sleep", "nb_restart"}
    if not required <= functions.keys():
        raise RuntimeError(f"Named Rust sleep boundary is missing: {sorted(required - functions.keys())}")
    native = ["esp_sleep_config_gpio_isolate", "gpio_deep_sleep_hold_en", "esp_deep_sleep_try_to_start"]
    sequence = [RUST_TRY, RUST_STAMP, RUST_STORE, RUST_RESTART]
    for name, target in ((RUST_TRY, "nb_try_sleep"), (RUST_RESTART, "nb_restart")):
        if ordered_calls(functions[name]) != [target]:
            raise RuntimeError("Rust Board sleep/restart must forward only to its exact native leaf")
        for opcode, _, line in native_instructions(functions[name]):
            if opcode.startswith("b") or opcode in {"j", "jr", "jal", "jalr"} and ordered_calls([line]) != [target]:
                raise RuntimeError("Rust Board sleep/restart contains a bypass or unresolved transfer")
    panic = "__rustc::rust_begin_unwind"
    if panic in ordered_calls(functions[RUST_SLEEP]) and (
            ordered_calls(functions.get(panic, [])) != ["newt_app_image::stop"] or
            ordered_calls(functions.get("newt_app_image::stop", [])) != ["abort"]):
        raise RuntimeError("Rust post-restart panic must lead only to the existing abort stop")
    for name, needed, returns in (("nb_try_sleep", native, True), (RUST_SLEEP, sequence, False)):
        calls = ordered_calls(functions[name])
        if any(calls.count(target) != 1 for target in needed) or (
                [calls.index(target) for target in needed] != sorted(calls.index(target) for target in needed)):
            raise RuntimeError("Rust sleep boundary is missing, duplicates or reorders a required native call")
        if returns and calls != needed or not returns and (
                calls[:len(needed)] != needed or any(call not in {"abort", "newt_app_image::stop", panic} for call in calls[len(needed):])):
            raise RuntimeError("Rust sleep boundary has an unexpected call around its required native calls")
        seen = set()
        for opcode, _, line in native_instructions(functions[name]):
            targets = ordered_calls([line])
            seen.update(set(targets) & set(needed))
            if opcode in {"jal", "jalr", "j", "jr"} and not targets:
                raise RuntimeError("Rust sleep boundary contains an unresolved control transfer")
            tail = [needed[-1]]
            allowed_jump = targets in (tail, ["abort"], ["newt_app_image::stop"], [panic]) and seen == set(needed)
            if opcode.startswith("b") or opcode == "ret" and (not returns or seen != set(needed)) or (
                    opcode in {"j", "jr"} and not allowed_jump):
                raise RuntimeError("Rust sleep entry must have no branch bypass around its required calls")
    if ordered_calls(functions["nb_restart"]) != ["esp_restart"]:
        raise RuntimeError("Rust restart leaf must call only the actual SDK restart")
    stores = record_stores(functions[RUST_STAMP])
    if not {28, 64} <= stores:
        raise RuntimeError("Linked rejection stamp must store rejectedSleep and checksum words")
    stored = ordered_calls(functions[RUST_STORE])
    if stored.count(RUST_WRITE) != 1 or stored.count("nsc_flush") != 1 or stored.index(RUST_WRITE) > stored.index("nsc_flush"):
        raise RuntimeError("Rust rejection must write its RTC record and flush before restart")
    writer = functions.get(RUST_WRITE, [])
    if ordered_calls(writer).count("nb_retained_words") != 1 or {
            int(match[1]) for opcode, args, _ in native_instructions(writer)
            if opcode == "sw" and (match := re.fullmatch(r"\w+,(-?\d+)\(a0\)", args))} != set(range(0, 68, 4)):
        raise RuntimeError("Rust retained writer must obtain the native regions and store all 68 record bytes")
    rtc = validate_retained(symbols, sections, "(anonymous namespace)::sleepWords", "(anonymous namespace)::sdResumeWords")
    return {"entry": RUST_SLEEP, "ordered_required_calls": sequence, "native_try_calls": native,
            "rejection_stamp_word_offsets": sorted(stores), "terminal_sleep_implementations_linked": False,
            "retained": rtc["retained"], "rtc_retention": rtc,
            "limits": "Compiled straight-line try/stamp/store/restart order and exact RTC layout; not physical sleep/wake proof."}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=Path(".pio/regional-dev-usb/build/newt_x3/firmware.elf"))
    parser.add_argument("--output", type=Path, default=Path("artifacts/regional-dev-usb-nvs-guard-link-verification.json"))
    parser.add_argument("--toolchain-bin", type=Path,
                        default=ROOT / ".toolchain/platformio/packages/toolchain-riscv32-esp/bin",
                        help="already installed pinned C3 tools (no acquisition)")
    parser.add_argument("--rust-app", action="store_true", help="check the Rust image's concrete ABI counterparts")
    parser.add_argument("--reading-only", action="store_true", help="owner-approved Rust bench excludes the update entry")
    args = parser.parse_args()
    if args.reading_only and not args.rust_app:
        parser.error("--reading-only requires --rust-app")
    output = ROOT / args.output
    if not output.is_relative_to(ROOT / "artifacts"):
        raise RuntimeError("Native guard reports must remain inside project artifacts")
    output = owned_file(ROOT, output, create_parents=True)
    elf = ROOT / args.elf
    objdump = args.toolchain_bin / "riscv32-esp-elf-objdump"
    text = subprocess.run([str(objdump), "-d", "-C", str(elf)], check=True,
                          capture_output=True, text=True).stdout
    functions = parse_functions(text)
    calls = {caller: set(ordered_calls(lines)) for caller, lines in functions.items()}
    required = {
        "initArduino": {"__wrap_nvs_flash_init", "__wrap_esp_partition_erase_range", ROLLBACK_HOOK},
        "__wrap_nvs_flash_init": {"nvs_flash_init"},
    }
    for caller, targets in required.items():
        if not targets <= calls.get(caller, set()):
            raise RuntimeError(f"Missing native guard edge: {caller} -> {sorted(targets)}")
    nvs_edges = {}
    for method, target in (("write(", "__wrap_esp_partition_write"),
                           ("write_raw(", "__wrap_esp_partition_write_raw"),
                           ("erase_range(", "__wrap_esp_partition_erase_range")):
        matches = {name: sorted(targets) for name, targets in calls.items()
                   if name.startswith("nvs::NVSPartition::" + method)}
        if not matches or not all(target in targets for targets in matches.values()):
            raise RuntimeError(f"Native NVS {method} bypasses its partition guard")
        nvs_edges.update(matches)
    real_apis = {"esp_partition_write", "esp_partition_write_raw", "esp_partition_erase_range"}
    direct_edges = {}
    for name, targets in calls.items():
        direct = targets & real_apis
        if not direct:
            continue
        if not name.startswith("__wrap_") and name not in real_apis:
            raise RuntimeError(f"Unguarded native partition mutation: {name} -> {sorted(direct)}")
        direct_edges[name] = sorted(direct)
    erase_apis = {"nvs_flash_erase", "nvs_flash_erase_partition", "nvs_flash_erase_partition_ptr"}
    if erase_apis & calls.keys():
        raise RuntimeError("Unwrapped whole-NVS formatting implementation remains reachable")
    symbols = subprocess.run([str(objdump), "-t", "-C", str(elf)], check=True,
                             capture_output=True, text=True).stdout
    sections = subprocess.run([str(objdump), "-h", str(elf)], check=True,
                              capture_output=True, text=True).stdout
    flash_writers = validate_flash_writers(functions)
    crash_args = {}
    if args.rust_app:
        from object_relocations import literal_string_ranges
        from wifi_link_image import Image
        def read(name, *arguments):
            return subprocess.run([str(args.toolchain_bin / f"riscv32-esp-elf-{name}"), *map(str, arguments)],
                                  check=True, capture_output=True, text=True).stdout
        layout = Image(elf.read_bytes(), read("nm", "-C", elf), text, elf.with_name("firmware.map").read_text())
        literals = literal_string_ranges(layout, lambda path: read("readelf", "-rWS", ROOT / path), ROOT)
        reviewed = {**CRASH_REFERENCES,
                    CRASH_WORD: CRASH_REFERENCES[CRASH_WORD] - {SETUP} | {"nb_allow_reading_crash_output"},
                    "nb_allow_reading_crash_output": {RUST_SELECTOR}}
        code, data = rust_registers(CONSOLE_REGISTER_CODE, CONSOLE_REGISTER_DATA, functions, args.reading_only)
        crash_args = dict(reviewed=reviewed, registers=(code, data), update_entry=(RUST_UPDATE, "nb_withhold_console"),
                          literals=literals, rust_app=True)
        if args.reading_only:
            if RUST_UPDATE in functions or "nb_withhold_console" in functions:
                raise RuntimeError("Reading-only image contains an update-mode entry")
            crash_args["update_entry"] = None
    crash_output = validate_crash_output(functions, symbols, sections, elf.read_bytes(),
                                         elf.with_name("firmware.map").read_text(), **crash_args)
    sleep = (validate_rust_sleep_boundary(functions, symbols, sections) if args.rust_app
             else validate_sleep_boundary(functions, symbols, sections))
    reader_stack = validate_reader_stack(functions, symbols, rust_app=args.rust_app)
    if args.rust_app:
        reader_stack["selected_open_frames"] = validate_open_frames(functions)
    validate_rollback_hook(functions, symbols)
    validate_no_timer_task(functions, symbols)
    receipt = {
        "elf_sha256": hashlib.sha256(elf.read_bytes()).hexdigest(),
        "startup_edges": {name: sorted(calls[name]) for name in required},
        "nvs_partition_edges": nvs_edges,
        "real_mutation_edges": direct_edges,
        "whole_nvs_format_implementations_linked": False,
        "wifi_fenced_flash_references": flash_writers,
        "update_mode_crash_output": crash_output,
        "sleep_boundary": sleep,
        "reader_loop_task": reader_stack,
        "freertos_timer_task_and_rgb_path_linked": False,
        "result": "PASS",
        "limits": "Linked reachability only; host fault tests and native behavior are separate gates.",
    }
    write_text(ROOT, output, json.dumps(receipt, indent=2) + "\n")
    print("PASS: linked C3 NVS/sleep guards, Wi-Fi flash fence, update mode's console output gates, owned reader loop task and first-paint image confirmation; "
          "exact RTC retention verified; no FreeRTOS timer task or RGB LED path")


if __name__ == "__main__":
    main()
