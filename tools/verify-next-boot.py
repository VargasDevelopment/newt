#!/usr/bin/env python3
"""Verify that a panic's leftover secrets stay out of the next boot (docs/UPDATES.md, "Secrets in RAM"); no hardware access.

A panic in update mode skips leaveUpdateMode's wipe, and RAM survives the
reset (so does a clean restart, for the Wi-Fi driver's freed blocks). The next
boot exposes no leftover only while four facts hold:

1. Reloaded range: the app image's load segments cover all of
   .dram0.newt_wifi, so the bootloader rewrites the whole range every boot,
   before a reading boot lends it to the heap.
2. Filled stacks: every function that lays out a new task's stack
   (pxPortInitialiseStack's referrers, FreeRTOS's prvInitialiseNewTask) first
   calls memset with tskSTACK_FILL_BYTE (0xa5), so no task starts on old bytes.
3. No DRAM in core dumps: the pinned sdkconfig keeps
   CONFIG_ESP_COREDUMP_CAPTURE_DRAM disabled, so a core dump holds task stacks
   only, not .data, .bss or the heap.
4. Cleared heap (firmware/heap_clear.h): IDF reaches the available regions
   and the startup stack's registration only through Newt's wraps, called
   only by heap_caps_init and main_task (a later caller would zero live heap).
   The regions wrap gets the list, then clears every region but the startup
   stack's before heap_caps_init registers them; the stack wrap clears the
   startup stack's regions before it registers them; the clear zeroes each
   through memset.

Limits: (2) checks the call and its fill byte, not memset's address or
length; (4) checks the calls and their flag and fill arguments, not the
loop. Device behavior is a separate claim.
"""
import argparse
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from pathlib import Path

from freeink_sdk_routes import FIRMWARE_BUILD

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / ".toolchain/platformio/packages/toolchain-riscv32-esp/bin"
SDKCONFIG = ROOT / ".toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3/sdkconfig"
STOPPED = "Next-boot check stopped"
FILL_BYTE = 0xa5  # FreeRTOS tskSTACK_FILL_BYTE
REGIONS, STACK_HEAPS = "soc_get_available_memory_regions", "heap_caps_enable_nonos_stack_heaps"
CLEAR = "_ZN4newt8firmware16clearHeapRegionsEPK19soc_memory_region_tjb"  # clearHeapRegions(regions, count, startupStack)
# The startup step each wrap belongs to: its only caller.
CALLERS = {REGIONS: "heap_caps_init", STACK_HEAPS: "main_task"}
CAPTURE_DRAM = "CONFIG_ESP_COREDUMP_CAPTURE_DRAM"
# Instructions whose first operand is a register they read, not write.
NO_DESTINATION = re.compile(r"^(?:s[bhw]|c\.s[bhw]\w*|b\w*|j|jr|ret)$")


def image_segments(image: bytes) -> list:
    """The app image's load segments as (address, length): an 8-byte header,
    a 16-byte extended header, then each segment's address, length and data."""
    if len(image) < 24 or image[0] != 0xE9:
        raise RuntimeError(f"{STOPPED}: not an ESP app image")
    segments, offset = [], 24
    for _ in range(image[1]):
        if offset + 8 > len(image):
            raise RuntimeError(f"{STOPPED}: the image ends inside its segment table")
        address, length = struct.unpack_from("<II", image, offset)
        segments.append((address, length))
        offset += 8 + length
    return segments


def reloaded(segments: list, start: int, end: int) -> list:
    """Failures unless the segments cover [start, end) without a gap."""
    if end <= start:
        return [f".dram0.newt_wifi is empty ({start:#x}-{end:#x})"]
    covered = start
    for address, length in sorted(segments):
        if address <= covered < address + length:
            covered = address + length
    if covered < end:
        return [f"no image segment loads .dram0.newt_wifi from {covered:#x} to {end:#x}, "
                "so a boot may keep the previous boot's bytes there"]
    return []


def parse_functions(disassembly: str) -> dict:
    """objdump -d: each function's instructions as (mnemonic, operands, annotation),
    where the annotation is the <symbol> a target or address resolves to."""
    functions, current = {}, None
    for line in disassembly.splitlines():
        label = re.match(r"^[0-9a-f]+ <(.+)>:$", line)
        if label:
            current = functions.setdefault(label[1], [])
            continue
        instruction = re.match(r"^\s*[0-9a-f]+:\s+[0-9a-f]+\s+(\S+)\s*([^#<]*)(.*)$", line)
        if instruction and current is not None:
            current.append((instruction[1], instruction[2].strip(), instruction[3]))
    return functions


def target(annotation: str) -> str:
    """The function an instruction names by its start (a branch inside a
    function reads <name+0x..>, which is no reference)."""
    match = re.search(r"<([^>+]+)>", annotation)
    return match[1] if match else ""


def calls(code: list, register: str) -> list:
    """Each call or tail jump in order, as (callee, the value `register` last
    took from li before it, or None)."""
    found, value = [], None
    for mnemonic, operands, annotation in code:
        if mnemonic in ("jal", "jalr", "call", "j", "tail"):
            callee = target(annotation)
            if callee or mnemonic != "j":
                found.append((callee, value))
            continue
        registers = operands.split(",")
        if registers[0] == register and not NO_DESTINATION.match(mnemonic):
            value = int(registers[1], 0) if mnemonic == "li" and len(registers) == 2 else None
    return found


def fills_first(code: list) -> bool:
    """The function's first call is memset, and a1 (the fill value) last
    holds FILL_BYTE before it."""
    first = calls(code, "a1")[:1]
    return first == [("memset", FILL_BYTE)]


def stacks_filled(functions: dict) -> list:
    """Failures unless every function that refers to pxPortInitialiseStack
    fills the stack first."""
    layouts = sorted(name for name, code in functions.items()
                     if any(target(annotation) == "pxPortInitialiseStack" for *_, annotation in code))
    if not layouts:
        return ["nothing calls pxPortInitialiseStack; the stack layout this check pins has moved"]
    return [f"{name} lays out a task stack without first filling it with {FILL_BYTE:#x} through memset"
            for name in layouts if not fills_first(functions[name])]


def heap_cleared(functions: dict) -> list:
    """Failures unless fact 4 holds (see the module docstring)."""
    wraps = {name: "__wrap_" + name for name in (REGIONS, STACK_HEAPS)}
    failures = [f"{name} does not reach {real} only through {wraps[real]}"
                for real in wraps for name, code in functions.items()
                if name not in wraps.values() and any(target(note) == real for *_, note in code)]
    for real, wrap in wraps.items():
        callers = {name for name, code in functions.items() if name != wrap for *_, note in code
                   if target(note) == wrap}
        if callers != {CALLERS[real]}:
            failures.append(f"{wrap} has callers {sorted(callers)}, not only {CALLERS[real]}")
    def order(wrap):  # the wrap's calls of interest, a clear with its startupStack flag
        return [(callee, flag if callee == CLEAR else None) for callee, flag in calls(functions.get(wrap, []), "a2")
                if callee in (REGIONS, CLEAR, STACK_HEAPS)]
    if order(wraps[REGIONS]) != [(REGIONS, None), (CLEAR, 0)]:
        failures.append(f"{wraps[REGIONS]} does not clear the regions it returns before heap_caps_init registers them")
    if order(wraps[STACK_HEAPS]) != [(REGIONS, None), (CLEAR, 1), (STACK_HEAPS, None)]:
        failures.append(f"{wraps[STACK_HEAPS]} does not clear the startup stack's regions before registering them")
    if ("memset", 0) not in calls(functions.get(CLEAR, []), "a1"):
        failures.append("clearHeapRegions does not zero its regions through memset")
    return failures


def coredump_dram(sdkconfig: str) -> list:
    """Failures unless the sdkconfig states CAPTURE_DRAM is disabled."""
    lines = {line.strip() for line in sdkconfig.splitlines()}
    if f"# {CAPTURE_DRAM} is not set" in lines and not any(line.startswith(CAPTURE_DRAM + "=") for line in lines):
        return []
    return [f"the pinned sdkconfig does not keep {CAPTURE_DRAM} disabled, so a core dump may hold DRAM"]


def tool(name, *arguments):
    return subprocess.run([str(TOOLS / f"riscv32-esp-elf-{name}"), *arguments], check=True,
                          capture_output=True, text=True).stdout


def main() -> None:
    global TOOLS
    parser = argparse.ArgumentParser(description=__doc__)
    build = Path(".pio") / FIRMWARE_BUILD / "build/newt_x3"
    parser.add_argument("--elf", type=Path, default=build / "firmware.elf")
    parser.add_argument("--image", type=Path, default=build / "firmware.bin")
    parser.add_argument("--sdkconfig", type=Path, default=SDKCONFIG)
    parser.add_argument("--toolchain-bin", type=Path, default=TOOLS, help="existing pinned C3 tools; no acquisition")
    args = parser.parse_args()
    TOOLS = args.toolchain_bin
    elf = ROOT / args.elf
    symbols = {match[2]: int(match[1], 16) for match in
               re.finditer(r"^([0-9a-f]+) \S (\S+)$", tool("nm", str(elf)), re.MULTILINE)}
    if "_newt_wifi_start" not in symbols or "_newt_wifi_end" not in symbols:
        raise RuntimeError(f"{STOPPED}: the ELF has no _newt_wifi_start/_newt_wifi_end")
    failures = reloaded(image_segments((ROOT / args.image).read_bytes()),
                        symbols["_newt_wifi_start"], symbols["_newt_wifi_end"])
    functions = parse_functions(tool("objdump", "-d", str(elf)))
    failures += stacks_filled(functions)
    failures += coredump_dram((ROOT / args.sdkconfig).read_text())
    failures += heap_cleared(functions)
    if failures:
        raise RuntimeError(f"{STOPPED}:\n" + "\n".join(failures))
    print("PASS: every boot reloads .dram0.newt_wifi, fills each new task stack, clears the heap's RAM "
          "before registering it, and core dumps leave DRAM out")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
