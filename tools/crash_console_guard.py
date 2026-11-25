"""Console register references and numeric tables in the linked C3 image."""
from bisect import bisect_right
import re
import struct

from linked_code import native_instructions

CONSOLE_REGISTERS = {"UART0": range(0x60000000, 0x60000084), "USB_SERIAL_JTAG": range(0x60043000, 0x60043084)}


def decimal_power_ranges(table, loaded):
    """Newlib's decimal conversion table is numeric data, checked byte for byte.

    The high word of 1e10 can equal a linked instruction address. A name alone
    is insufficient: a writable, resized or modified table still fails.
    """
    found = [entry for entry in table if entry[4] == "__mprec_tens"]
    if not found:
        return []
    expected = struct.pack("<25d", *(float(f"1e{power}") for power in range(25)))
    if len(found) == 1:
        address, flags, section, size, _ = found[0]
        for name, start, contents, attributes in loaded:
            # C3 DROM is read-only hardware even when the output section
            # inherits a DATA flag from another input in the SDK linker script.
            drom = name == ".flash.rodata" and 0x3c000000 <= address and address + size <= 0x3c800000
            if (name == section and "O" in flags and ("READONLY" in attributes or drom) and "CODE" not in attributes
                    and size == len(expected) and start <= address and address + size <= start + len(contents)
                    and contents[address - start:address - start + size] == expected):
                return [(address, address + size)]
    raise RuntimeError("Newlib __mprec_tens is not the exact read-only 25-double decimal power table")


def validate_console_registers(linked, table, loaded, link_map, code,
                               data, literals=(), numbers=()):
    """Only reviewed functions build a console register address (a lui or
    auipc whose value, give or take its 12-bit offset, reaches a register
    file) and only reviewed objects hold one. A word outside every object is
    named by the input section the link map places it in."""
    def reaches(value):
        return any(value - 0x800 < block.stop and block.start < value + 0x800 for block in CONSOLE_REGISTERS.values())
    builders = set()
    for name, lines in linked.items():
        for line in lines:
            built = re.match(r"^\s*([0-9a-fA-F]+):\s+[0-9a-fA-F]+\s+(lui|auipc)\s+\w+,(-?(?:0x[0-9a-fA-F]+|\d+))", line)
            if built:
                value = int(built[3], 0) << 12
                if reaches((value + (int(built[1], 16) if built[2] == "auipc" else 0)) & 0xFFFFFFFF):
                    builders.add(name)
    if builders != set(code):
        raise RuntimeError(f"Console registers reached by unreviewed code: built by {sorted(builders - set(code))}, "
                           f"no longer by {sorted(set(code) - builders)}")
    objects = sorted((address, address + size, name) for address, flags, _, size, name in table if size and "O" in flags)
    # The map lists a merged string section at its size before merging, so
    # the last section starting at or before an address places it.
    inputs = sorted((int(m[2], 16), int(m[2], 16) + int(m[3], 16), m[1]) for m in
                    re.finditer(r"^ (\.\S+)\s+0x([0-9a-fA-F]+)\s+0x([0-9a-fA-F]+)\s", link_map, re.M))
    holders = set()
    for section, start, contents, flags in loaded:
        if "CODE" in flags:
            continue
        for offset in range(-start % 4, len(contents) - 3, 4):
            if any(struct.unpack_from("<I", contents, offset)[0] in block for block in CONSOLE_REGISTERS.values()):
                address = start + offset
                if any(low <= address and address + 4 <= high for low, high in numbers):
                    continue
                if (any(low <= address and address + 4 <= high for low, high in literals)
                        and not any(low < address + 4 and address < high for low, high, _ in objects)):
                    continue
                holder = next((name for low, high, name in objects if low <= address < high), None)
                if holder is None:
                    index = bisect_right(inputs, (address, 1 << 32)) - 1
                    placed = inputs[index][2] if index >= 0 and address < inputs[index][1] else ""
                    holder = ".gcc_except_table" if placed.startswith(".gcc_except_table.") else None
                if holder not in data:
                    raise RuntimeError(f"Console registers reached by unreviewed data: {section}+0x{offset:x} "
                                       f"({holder or 'no object in ' + (placed or 'the map')}) holds a console "
                                       f"register address")
                holders.add(holder)
    return {"code": sorted(code), "data": sorted(holders)}


def rust_registers(base_code, base_data, functions, reading_only=False):
    """Same reviewed register/numeric roles for Rust's concrete linked owners."""
    code = dict(base_code)
    code["nsc_wake_transmitter"] = code.pop("newt::firmware::UsbChannel::wakeTransmitter(unsigned int)")
    put_rx = dict.fromkeys(("nsc_put_rx_begin", "nsc_put_rx_end", "nsc_put_rx_read"),
                          "reviewed by Newt Lead, cross-family native review: Astra, 2026-11-24 (325bb44a, delta 7896ff2b); bulk-read rename #135")
    if any(name in functions for name in put_rx):
        code.update(put_rx)
    # Rust 1.93.1 core/fmt/mod.rs: default FormattingOptions flags are
    # (3 << 29) | ' ' = 0x60000020, stored in the stack Formatter, not MMIO.
    code["core::fmt::write"] = "packed default formatting flags; no console register access"
    if reading_only:
        if "ip6_output_if_src" in functions:
            raise RuntimeError("Reading-only image contains the excluded IPv6 output entry")
        code.pop("ip6_output_if_src")
    data = dict(base_data)
    data["newt_ui_canvas::data::KERNS"] = data.pop("newt::kHomeKerns")
    data["newt_ui_canvas::data::RUNS"] = data.pop("newt::kHomeRuns")
    # Same font bytes, now reached through lossless u16 block indices.
    # pack-reader-atlas.py --check reconstructs the entire pinned payload.
    glyphs = data["newt_reader_atlas::data::kReaderAddedBitmap"]
    for name in ("kReaderBitmap", "kReaderExtraBitmap", "kReaderBoldBitmap", "kReaderBoldExtraBitmap",
                 "kReaderItalicBitmap", "kReaderItalicExtraBitmap", "kReaderBoldItalicBitmap",
                 "kReaderBoldItalicExtraBitmap", "kReaderAddedBitmap"):
        data["newt_reader_atlas::data::" + name + "Blocks"] = glyphs
    data["newt_reader_atlas::data::BITMAP_BLOCKS"] = glyphs
    # Native shapes for the other reading sizes: 1-bit font pixels whose bytes
    # make-native-reader-sizes.py --check binds to provenance.json.
    for px in (22, 24, 27, 34):
        data[f"newt_reader_atlas::native::BITMAP_{px}PX"] = glyphs
    return code, data
