"""Shared SRAM and RTC occupancy of the linked C3 image, read from `size -A`
and the linker map (docs/CODE_HEALTH.md "Memory policy")."""
import re

RAM_CAPACITY = 321296  # Actual shared alias region in the pinned C3 linker map.
# Wi-Fi's static data (tools/wifi-sections.py), lent to the heap in every boot
# that is not update mode: reported and capped as wifi_lent_bytes, not as the
# resident RAM that reading pays (docs/CODE_HEALTH.md "Memory policy").
LENT_SECTION = ".dram0.newt_wifi"


def ram_sections(text):
    sections = {}
    for line in text.splitlines():
        fields = line.split()
        if fields and (fields[0].startswith((".dram", ".iram")) or fields[0] == ".noinit"):
            if len(fields) != 3 or not fields[1].isdigit() or not fields[2].isdigit():
                raise ValueError("Invalid C3 size section")
            name, size = fields[0], int(fields[1])
            if name in sections:
                raise ValueError(f"Duplicate SRAM section: {name}")
            if name in {".dram0.data", ".dram0.bss", ".iram0.text", ".iram0.data", ".iram0.bss",
                        ".iram0.vectors", ".iram0.text_end", ".dram0.heap_start", ".noinit"}:
                sections[name] = size
            elif name not in (".dram0.dummy", LENT_SECTION):
                raise ValueError(f"Unaccounted C3 SRAM section: {name}")
    if not {".dram0.data", ".dram0.bss", ".iram0.text"} <= sections.keys():
        raise ValueError("Missing C3 SRAM sections")
    return sections


def ram_measurements(text, memory_map):
    regions = {}
    for name, origin, length in re.findall(r"^(iram0_0_seg|dram0_0_seg)\s+(0x[0-9a-fA-F]+)\s+(0x[0-9a-fA-F]+)\s", memory_map, re.MULTILINE):
        if name in regions:
            raise ValueError("Duplicate shared SRAM linker region")
        regions[name] = (int(origin, 16), int(length, 16))
    if regions != {"iram0_0_seg": (0x40380000, RAM_CAPACITY), "dram0_0_seg": (0x3FC80000, RAM_CAPACITY)}:
        raise ValueError("Missing or changed pinned shared SRAM linker capacity")
    sections, intervals, rtc, lent = ram_sections(text), [], {}, None
    for line in text.splitlines():
        fields = line.split()
        if len(fields) != 3 or not fields[1].isdigit() or not fields[2].isdigit():
            continue
        name, size, address = fields[0], int(fields[1]), int(fields[2])
        if name.startswith(".rtc"):
            if name not in {".rtc.text", ".rtc.data", ".rtc.bss", ".rtc.force_fast", ".rtc.force_slow", ".rtc_noinit", ".rtc_reserved"}:
                raise ValueError(f"Unaccounted RTC section: {name}")
            if name in rtc or not 0x50000000 <= address <= address + size <= 0x50002000:
                raise ValueError("Invalid RTC accounting")
            rtc[name] = size
        if name == ".dram0.dummy":
            if address != 0x3FC80000 or size > RAM_CAPACITY:
                raise ValueError("Invalid shared SRAM alias padding")
            continue
        in_pool = any(origin <= address < origin + capacity for origin, capacity in regions.values())
        if name in sections:
            expected = regions["iram0_0_seg" if name.startswith(".iram") else "dram0_0_seg"][0]
            start = address - expected
            if not 0 <= start <= start + size <= RAM_CAPACITY:
                raise ValueError(f"SRAM section outside pinned region: {name}")
            if size:
                intervals.append((start, start + size))
        elif name == LENT_SECTION:
            start = address - regions["dram0_0_seg"][0]
            if lent is not None or not 0 <= start <= start + size <= RAM_CAPACITY:
                raise ValueError(f"Invalid lent SRAM section: {name}")
            lent = size
            if size:
                intervals.append((start, start + size))
        elif size and in_pool:
            raise ValueError(f"Unaccounted SRAM-resident section: {name}")
    intervals.sort()
    if not intervals or any(right[0] < left[1] for left, right in zip(intervals, intervals[1:])):
        raise ValueError("Invalid overlapping shared SRAM accounting")
    # Resident RAM is the occupied pool less the lent range, which the heap
    # gets back in a reading boot; alignment stays what neither accounts for.
    resident = intervals[-1][1] - (lent or 0)
    return {"ram_sections": sections, "ram_bytes": resident, "ram_alignment_bytes": resident - sum(sections.values()),
            "wifi_lent_bytes": lent or 0,
            "ram_capacity_bytes": RAM_CAPACITY, "rtc_sections": rtc, "rtc_bytes": sum(rtc.values())}
