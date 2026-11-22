"""Fault controls for tools/verify-wifi-link.py (docs/084-wifi-memory.md): a
small synthetic link in the reviewed shape passes, and each broken rule fails."""
import copy
import fnmatch
import importlib.util
from pathlib import Path
import struct
import sys
import unittest
from types import SimpleNamespace
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import wifi_lent_heap  # noqa: E402
from object_relocations import literal_string_ranges, member_text
spec = importlib.util.spec_from_file_location("wifi_link", ROOT / "tools/verify-wifi-link.py")
link = importlib.util.module_from_spec(spec)
spec.loader.exec_module(link)

LEND, SCAN_WIFI = "newt::firmware::lendWifiMemory()", "newt::firmware::scanWifiNetworks(newt::UpdateView&)"
CONSTRUCTORS = "newt::firmware::runWifiConstructors()"
JOIN_WIFI = link.JOIN
FORGET_WIFI = "newt::firmware::forgetWifiBeforeRestart()"
MAIN, WIFI, PHY = "main.cpp.o", "libesp_wifi.a(wifi_init.c.obj)", "libphy.a(phy.o)"
# A file both share: its DS lock is Wi-Fi's (WIFI_SECTIONS), its SHA lock is not.
LOCKS, HEAP = "libesp_security.a(esp_crypto_lock.c.obj)", "libheap.a(heap_caps.c.obj)"
# Update mode's save and the store's call into the cipher (update-mode code),
# and mbedTLS, shared with Wi-Fi: its allocator file is Wi-Fi's.
CHANGE, SAVE, RESTORE, GCM = "changeUpdateNetwork", "saveWifiNetwork", "restoreUpdateNetwork", "mbedtls_gcm_setkey"
LENT_AT, LENT_END = 0x3fc8f000, 0x3fc8f020
SECTIONS = {  # name: (ELF type, flags, address); 1 PROGBITS; flags 2 alloc, 4 exec
    ".iram0.text": (1, 6, 0x40380000), ".flash.text": (1, 6, 0x42000000),
    ".dram0.data": (1, 3, 0x3fc8d000), ".dram0.newt_wifi": (1, 3, LENT_AT), ".flash.rodata": (1, 2, 0x3c000000),
}
# Code: section -> [(function, file, instructions)]; "@name" is that function's address.
CODE = {
    ".iram0.text": [("uart_isr", "libesp_driver_uart.a(uart.c.obj)", ["ret"])],
    ".flash.text": [
        ("setup", MAIN, ["jal @esp_log_level_set", f"jal @{link.ENTER}", "ret"]),
        (link.ENTER, MAIN, [f"jal @{link.RUN}", f"j @{LEND}"]),
        (link.RUN, MAIN, [f"jal @{CONSTRUCTORS}", f"jal @{link.SCAN}", f"jal @{link.CONNECT}", f"jal @{link.CHECK}",
                          f"jal @{RESTORE}", f"jal @{CHANGE}", f"jal @{FORGET_WIFI}", "ret"]),
        # Update mode runs the Wi-Fi constructor boot skipped, from its own list.
        (CONSTRUCTORS, MAIN, ["lui a5,0x3c000", "lw a5,12(a5)", "ret"]),
        ("esp_ds_conn_lock", "libmbedcrypto.a(esp_ds_common.c.obj)", ["ret"]),
        (RESTORE, MAIN, [f"jal @{link.OPEN}", "ret"]),
        (link.OPEN, MAIN, [f"jal @{GCM}", "ret"]),
        (CHANGE, MAIN, [f"jal @{SAVE}", "ret"]),
        (SAVE, MAIN, [f"jal @{link.SEAL}", "ret"]),
        (link.SEAL, MAIN, [f"jal @{GCM}", "ret"]),
        (GCM, "libmbedcrypto.a(gcm.c.obj)", ["jal @mbedtls_calloc", "ret"]),
        ("mbedtls_calloc", "libmbedcrypto.a(platform.c.obj)", ["ret"]),
        ("ccmp_encrypt", "libwpa_supplicant.a(ccmp.c.obj)", [f"jal @{GCM}", "ret"]),
        (link.SCAN, MAIN, [f"jal @{SCAN_WIFI}", "ret"]),
        (SCAN_WIFI, MAIN, ["jal @esp_wifi_init", "ret"]),
        (link.CONNECT, MAIN, [f"jal @{JOIN_WIFI}", "ret"]),
        (JOIN_WIFI, MAIN, ["jal @esp_wifi_init", "ret"]),
        # The GitHub check passes its fetch to the join, which calls it once joined.
        (link.CHECK, MAIN, [f"jal @{link.FETCH}", f"jal @{JOIN_WIFI}", "ret"]),
        (link.FETCH, MAIN, ["ret"]),
        (FORGET_WIFI, MAIN, ["jal @esp_wifi_init", "ret"]),
        (LEND, MAIN, ["lui a0,0x3fc8f", "addi a0,a0,0", "jal @heap_caps_add_region_with_caps", "ret"]),
        ("heap_caps_add_region_with_caps", "libheap.a(heap_caps_init.c.obj)", ["ret"]),
        ("esp_log_level_set", "liblog.a(log_level.c.obj)", ["ret"]),
        ("esp_wifi_init", WIFI, ["lui a5,0x3fc8f", "lw a4,4(a5)", "jal @phy_wakeup", "jal @ccmp_encrypt"]),
        ("s_set_default_wifi_log_level", WIFI, ["jal @esp_log_level_set", "ret"]),
        ("phy_wakeup", PHY, ["jal @xRingbufferSend", "ret"]),
        ("xRingbufferSend", "libesp_ringbuf.a(ringbuf.c.obj)", ["ret"]),
        # A FLASH_FUNCTIONS function: flash code, called only from flash.
        ("heap_caps_malloc_prefer", HEAP, ["ret"]),
        ("readerTask", MAIN, ["ret"]),
    ],
}
# Data: (output section, input section, file, address, words). As in the
# real image, the lent range ends with a merged constant and padding, which
# are no node: +0x18 a libphy string, +0x1c..+0x20 zero padding.
DATA = [
    (".dram0.data", ".data.readerState", MAIN, 0x3fc8d000, [0, 0]),
    (".dram0.newt_wifi", ".bss.s_wifi_state", WIFI, LENT_AT, [0] * 4),
    (".dram0.newt_wifi", ".data.phy_param", PHY, LENT_AT + 0x10, [0]),
    (".flash.rodata", ".init_array", WIFI, 0x3c000000, ["@s_set_default_wifi_log_level"]),
    (".dram0.newt_wifi", ".rodata.get_phy_version_str.str1.4", PHY, LENT_AT + 0x18, [0]),
    (".dram0.newt_wifi", ".sbss.s_crypto_ds_lock", LOCKS, LENT_AT + 0x14, [0]),
    # A deferred constructor's list (tools/wifi-sections.py WIFI_CONSTRUCTORS): outside boot's.
    (".flash.rodata", ".init_array", "libmbedcrypto.a(esp_ds_common.c.obj)", 0x3c00000c, ["@esp_ds_conn_lock"]),
]
SYMBOLS = {
    "_newt_wifi_start": LENT_AT, "_newt_wifi_end": LENT_END, "__global_pointer$": 0x3fc8d800,
    "__init_array_start": 0x3c000000, "__init_array_end": 0x3c000004,
    "_esp_system_init_fn_array_start": 0x3c000004, "_esp_system_init_fn_array_end": 0x3c000004,
}


def elf(sections):
    """A little-endian ELF32 with only section headers and contents."""
    names, body, headers = bytearray(b"\0"), bytearray(52), [bytes(40)]
    for name, (kind, flags, address, content) in sections.items():
        headers.append(struct.pack("<10I", len(names), kind, flags, address, len(body), len(content), 0, 0, 4, 0))
        names += name.encode() + b"\0"
        body += content
    headers.append(struct.pack("<10I", len(names), 3, 0, 0, len(body), len(names) + 10, 0, 0, 1, 0))
    names += b".shstrtab\0"
    body += names
    struct.pack_into("<4sBB", body, 0, b"\x7fELF", 1, 1)
    struct.pack_into("<I", body, 0x20, len(body))
    struct.pack_into("<HHH", body, 0x2e, 40, len(headers), len(headers) - 1)
    return bytes(body + b"".join(headers))


class Link:
    """The reviewed shape, to break one rule at a time."""

    def __init__(self):
        self.code, self.data, self.symbols = copy.deepcopy(CODE), copy.deepcopy(DATA), dict(SYMBOLS)
        self.relocations = None  # object path -> its `readelf -rW`, to read them as main does

    def function(self, name):
        return next(entry for entries in self.code.values() for entry in entries if entry[0] == name)

    def failures(self):
        addresses = {name: SECTIONS[section][2] + 0x10 * index
                     for section, entries in self.code.items() for index, (name, _, _) in enumerate(entries)}

        def resolve(text):
            return f"{addresses[text[1:]]:x} <{text[1:]}>" if text.startswith("@") else text

        disassembly, link_map, nm = [], ["Linker script and memory map"], []
        contents = {name: bytearray() for name in SECTIONS}
        for section, entries in self.code.items():
            link_map.append(f"{section} 0x{SECTIONS[section][2]:08x} 0x{0x10 * len(entries):x}")
            contents[section] = bytearray(0x10 * len(entries))
            for name, file, instructions in entries:
                at = addresses[name]
                nm.append(f"{at:08x} T {name}")
                disassembly.append(f"{at:08x} <{name}>:")
                for offset, instruction in enumerate(instructions):
                    mnemonic, _, operands = instruction.partition(" ")
                    # A whole "@name" operand may hold commas (a demangled signature).
                    operands = resolve(operands) if operands.startswith("@") else \
                        ",".join(resolve(part) for part in operands.split(","))
                    disassembly.append(f"{at + 4 * offset:08x}:\t00000013          \t{mnemonic}\t{operands}")
                link_map += [f" .text.{len(nm)}", f"                0x{at:08x}       0x10 /x/{file}"]
        for section, name, file, address, words in self.data:
            link_map += [section, f" {name}", f"                0x{address:08x}       0x{4 * len(words):x} {Path('/x') / file}"]
            values = b"".join(struct.pack("<I", addresses[word[1:]] if isinstance(word, str) else word)
                              for word in words)
            offset = address - SECTIONS[section][2]
            contents[section][len(contents[section]):] = bytes(max(0, offset + len(values) - len(contents[section])))
            contents[section][offset:offset + len(values)] = values
        contents[".dram0.newt_wifi"] += bytes(LENT_END - LENT_AT - len(contents[".dram0.newt_wifi"]))
        nm += [f"{address:08x} D {name}" for name, address in self.symbols.items()]
        image = link.Image(elf({name: (*SECTIONS[name][:3], bytes(content)) for name, content in contents.items()}),
                           "\n".join(nm), "\n".join(disassembly), "\n".join(link_map))
        if self.relocations is None:
            return link.analyse(image)["failures"]
        return link.analyse(image, link.chance_words(image, lambda path: self.relocations[path], ROOT))["failures"]


class WifiLinkGate(unittest.TestCase):
    def fails(self, edit, message):
        broken = Link()
        edit(broken)
        failures = broken.failures()
        self.assertTrue(any(message in failure for failure in failures), failures)

    def test_reviewed_shape_passes(self):
        self.assertEqual(Link().failures(), [])

    def test_lent_range_holds_only_reviewed_wifi_inputs(self):
        def foreign(link_):
            link_.data[2] = (".dram0.newt_wifi", ".data.readerCache", MAIN, LENT_AT + 0x10, [0] * 4)

        def wifi_code(link_):
            link_.data[2] = (".dram0.newt_wifi", ".text.ppTask", "libpp.a(pp.o)", LENT_AT + 0x10, [0] * 4)

        def merged(link_):
            link_.data[4] = (".dram0.newt_wifi", ".rodata.readerTitle.str1.4", MAIN, LENT_AT + 0x18, [0])

        def shared_file(link_):
            link_.data[5] = (".dram0.newt_wifi", ".sbss.s_crypto_sha_aes_lock", LOCKS, LENT_AT + 0x14, [0])
        self.fails(foreign, "holds .data.readerCache of main.cpp.o")
        self.fails(shared_file, f"holds .sbss.s_crypto_sha_aes_lock of {LOCKS}")
        self.fails(wifi_code, "holds .text.ppTask of libpp.a(pp.o)")
        self.fails(merged, "holds .rodata.readerTitle.str1.4 of main.cpp.o")

    def test_lent_range_is_exactly_the_section_and_out_of_gp_reach(self):
        self.fails(lambda link_: link_.symbols.update(_newt_wifi_end=LENT_END - 8), "is not exactly")
        self.fails(lambda link_: link_.symbols.update({"__global_pointer$": LENT_AT - 0x400}), "gp-relative")

    def test_reading_code_never_reaches_wifi(self):
        def call(link_):
            link_.function("readerTask")[2][:] = ["jal @esp_wifi_init", "ret"]

        def lent_data(link_):
            link_.function("readerTask")[2][:] = ["lui a5,0x3fc8f", "sw zero,16(a5)", "ret"]

        def pointer(link_):
            link_.data[0] = (".dram0.data", ".data.readerState", MAIN, 0x3fc8d000, ["@esp_wifi_init", 0])
        self.fails(call, "readerTask (main.cpp.o) refers to Wi-Fi: ['esp_wifi_init']")
        self.fails(lent_data, "readerTask (main.cpp.o) refers to Wi-Fi: ['.data.phy_param']")
        self.fails(pointer, ".data.readerState (main.cpp.o) refers to Wi-Fi")

    def test_reading_code_never_reaches_the_lent_range_by_address(self):
        # Padding and merged constants are no node: a reference counts by its
        # address alone (Astra's store into the last four lent bytes).
        def padding(link_):
            link_.function("readerTask")[2][:] = ["lui a5,0x3fc8f", "sw zero,28(a5)", "ret"]

        def padding_pointer(link_):
            link_.data[0] = (".dram0.data", ".data.readerState", MAIN, 0x3fc8d000, [LENT_AT + 0x1c, 0])

        def merged(link_):
            link_.function("readerTask")[2][:] = ["lui a0,0x3fc8f", "addi a0,a0,24", "ret"]

        def aligned(link_):
            # A 4 KiB-aligned address is a lui alone, then `mv` (addi rd,rs,0).
            link_.function("readerTask")[2][:] = ["lui a0,0x3fc8f", "mv a0,a0", "ret"]

        def wifi_constant(link_):
            # A Wi-Fi constant in flash that points into the range is Wi-Fi's.
            link_.data.append((".flash.rodata", ".rodata.phy_tables", PHY, 0x3c000008, [LENT_AT + 0x1c]))
            link_.function("readerTask")[2][:] = ["lui a5,0x3c000", "lw a0,8(a5)", "ret"]
        self.fails(padding, "readerTask (main.cpp.o) refers to Wi-Fi: ['lent range +0x1c']")
        self.fails(padding_pointer, ".data.readerState (main.cpp.o) refers to Wi-Fi: ['lent range +0x1c']")
        self.fails(merged, "readerTask (main.cpp.o) refers to Wi-Fi: ['lent range +0x18']")
        self.fails(aligned, "readerTask (main.cpp.o) refers to Wi-Fi")
        self.fails(wifi_constant, "readerTask (main.cpp.o) refers to Wi-Fi: ['.rodata.phy_tables']")

    def test_only_a_relocated_word_in_newts_own_objects_points(self):
        # A word in Newt's own data (a font's pixels) can equal a Wi-Fi function's start by chance.
        def word(file, offsets):
            def edit(link_):
                link_.data[0] = (".dram0.data", ".data.readerState", file, 0x3fc8d000, ["@esp_wifi_init", 0])
                link_.relocations = {f"/x/{file}": "Relocation section '.rela.data.readerState' at offset 0x0:\n"
                                     + "".join(f"{offset:08x}  00000a01 R_RISCV_32  00000000  x + 0\n"
                                               for offset in offsets)}
            return edit
        chance = Link()
        word(MAIN, [4])(chance)
        self.assertEqual(chance.failures(), [])
        self.fails(word(MAIN, [0, 4]), ".data.readerState (main.cpp.o) refers to Wi-Fi")
        # An archive member's words stay pointers: its object is never read.
        self.fails(word("libreader.a(state.o)", []), ".data.readerState (libreader.a(state.o)) refers to Wi-Fi")
        unreadable = Link()
        word(MAIN, [])(unreadable)
        unreadable.relocations = {}
        self.assertRaises(KeyError, unreadable.failures)

    def test_sdk_archive_inside_repository_still_exposes_stored_wifi_pointer(self):
        archive = str(ROOT / ".toolchain/sdk/libreader.a")
        path = archive + "(state.o)"
        fixture = Link()
        fixture.data[0] = (".dram0.data", ".data.readerState", path, 0x3fc8d000, ["@esp_wifi_init", 0])
        fixture.relocations = {path: f"File: {path}\n"}
        self.assertTrue(any(".data.readerState" in failure and "refers to Wi-Fi" in failure
                            for failure in fixture.failures()))

    def test_entries_are_reached_only_from_update_mode(self):
        for name in (SCAN_WIFI, link.SCAN, link.RUN, LEND, link.SEAL, link.OPEN, JOIN_WIFI, link.CONNECT, FORGET_WIFI,
                     CONSTRUCTORS):
            self.fails(lambda link_, name=name: link_.function("readerTask")[2].insert(0, f"jal @{name}"),
                       f"{name} is reached from outside update mode")

    def test_library_entry_callers_are_update_mode_code(self):
        # The cipher's caller, reached from a reading path too, is not update
        # mode's. A reading path into the library makes it no longer Wi-Fi's,
        # so its own reach into Wi-Fi's files fails.
        def shared_caller(link_):
            link_.function("readerTask")[2].insert(0, f"jal @{SAVE}")

        def library(link_):
            link_.function("readerTask")[2].insert(0, f"jal @{GCM}")
        self.fails(shared_caller, f"{link.SEAL} is reached from outside update mode: ['{SAVE}']")
        self.fails(library, f"{GCM} (libmbedcrypto.a(gcm.c.obj)) refers to Wi-Fi: ['mbedtls_calloc']")

    def test_lend_names_only_the_range_start(self):
        self.fails(lambda link_: link_.function(LEND)[2].insert(2, "lw a1,16(a0)"),
                   "may refer only to the lent range's start")

    def test_missing_lend_fails(self):
        def remove(link_):
            link_.code[".flash.text"].remove(link_.function(LEND))
            link_.function(link.ENTER)[2][:] = [f"jal @{link.RUN}", "ret"]
        self.fails(remove, f"missing {LEND}")

    def test_startup_roots_are_reviewed(self):
        def new_call(link_):
            link_.function("s_set_default_wifi_log_level")[2].insert(0, "jal @heap_caps_add_region_with_caps")

        def new_root(link_):
            link_.data[3] = (".flash.rodata", ".init_array", WIFI, 0x3c000000,
                             ["@s_set_default_wifi_log_level", "@esp_wifi_init"])
            link_.symbols["__init_array_end"] = link_.symbols["_esp_system_init_fn_array_start"] = \
                link_.symbols["_esp_system_init_fn_array_end"] = 0x3c000008
        def boot_runs_deferred(link_):
            # esp-tls's DS lock constructor back in a boot list: every boot would keep its mutex.
            link_.symbols.update(_esp_system_init_fn_array_start=0x3c00000c, _esp_system_init_fn_array_end=0x3c000010)
        self.fails(new_call, "now calls ['esp_log_level_set', 'heap_caps_add_region_with_caps']")
        self.fails(boot_runs_deferred, "unreviewed startup root into Wi-Fi code: esp_ds_conn_lock")
        self.fails(new_root, "unreviewed startup root into Wi-Fi code: esp_wifi_init")

    def test_iram_never_calls_flash_resident_code(self):
        def call(link_):
            link_.function("uart_isr")[2][:] = ["jal @xRingbufferSend", "ret"]

        def resident(link_):
            link_.code[".iram0.text"].append(("phy_get_romfuncs", PHY, ["ret"]))

        def moved_call(link_):
            link_.function("uart_isr")[2][:] = ["jal @heap_caps_malloc_prefer", "ret"]

        def moved_resident(link_):
            moved = link_.function("heap_caps_malloc_prefer")
            link_.code[".flash.text"].remove(moved)
            link_.code[".iram0.text"].append(moved)
        self.fails(call, "IRAM uart_isr calls flash code: ['xRingbufferSend']")
        self.fails(resident, "phy_get_romfuncs of libphy.a(phy.o) is still in IRAM")
        self.fails(moved_call, "IRAM uart_isr calls flash code: ['heap_caps_malloc_prefer']")
        self.fails(moved_resident, f"heap_caps_malloc_prefer of {HEAP} is still in IRAM")

    def test_flash_function_patterns_keep_the_files_other_iram_code(self):
        # ld matches input section names as fnmatch does.
        patterns = link.wifi_sections.all_iram_but(".iram1.4").split()
        names = [".iram1"] + [f".iram1.{index}" for index in range(60)]
        self.assertEqual([name for name in names if not any(fnmatch.fnmatchcase(name, p) for p in patterns)],
                         [".iram1.4"])


# readelf --debug-dump=info for another unit and IDF's vfs_fat.c, as the
# pinned toolchain prints them: the other unit's FIL is not the context's.
DWARF = """
  Compilation Unit @ offset 0x100:
 <0><10b>: Abbrev Number: 1 (DW_TAG_compile_unit)
    <10c>   DW_AT_name        : (indirect string, offset: 0x1): /x/fatfs/src/ff.c
 <1><120>: Abbrev Number: 4 (DW_TAG_typedef)
    <121>   DW_AT_name        : FIL
    <125>   DW_AT_type        : <0x130>
 <1><130>: Abbrev Number: 23 (DW_TAG_structure_type)
    <131>   DW_AT_byte_size   : 48
  Compilation Unit @ offset 0x200:
 <0><20b>: Abbrev Number: 1 (DW_TAG_compile_unit)
    <20c>   DW_AT_name        : (indirect string, offset: 0x2): /x/esp-idf/components/fatfs/vfs/vfs_fat.c
 <1><220>: Abbrev Number: 4 (DW_TAG_typedef)
    <221>   DW_AT_name        : FIL
    <225>   DW_AT_type        : <0x230>
 <1><230>: Abbrev Number: 23 (DW_TAG_structure_type)
    <231>   DW_AT_byte_size   : 4136
 <1><240>: Abbrev Number: 4 (DW_TAG_typedef)
    <241>   DW_AT_name        : (indirect string, offset: 0x3): vfs_fat_ctx_t
    <245>   DW_AT_type        : <0x250>
 <1><250>: Abbrev Number: 23 (DW_TAG_structure_type)
    <251>   DW_AT_byte_size   : 8832
"""
MOUNT = {"firmware/sd_library.cpp": "  mounted_ = SD.begin(board.sd.cs, SPI, spiHz, kSdMountPoint, 3, false);\n"}


class LentHeapGate(unittest.TestCase):
    """4. The lent range holds the SD card's FATFS context: 8,832 + 3 x 4,136
    = 21,240 B, a 21,504 B TLSF block, plus the lent heap's own overhead."""

    def test_range_that_holds_the_context_passes(self):
        failures, receipt = wifi_lent_heap.lent_heap(22528, DWARF, MOUNT)
        self.assertEqual(failures, [])
        self.assertEqual((receipt["request"], receipt["block"], receipt["needed"]), (21240, 21504, 22528))
        native = {"rust/salamander/board/native/board.cpp":
                  "return SD.begin(board.sd.cs, SPI, hz, kSdMountPoint, 3, false) ? NB_OK : NB_IO;"}
        self.assertEqual(wifi_lent_heap.lent_heap(22528, DWARF, native), (failures, receipt))

    def test_range_too_small_for_the_context_fails(self):
        self.assertIn("raise LENT_FLOOR", wifi_lent_heap.lent_heap(22527, DWARF, MOUNT)[0][0])
        more_files = {"firmware/sd_library.cpp": MOUNT["firmware/sd_library.cpp"].replace(", 3,", ", 4,")}
        self.assertIn("26624 B needed", wifi_lent_heap.lent_heap(22528, DWARF, more_files)[0][0])
        bigger = DWARF.replace("byte_size   : 8832", "byte_size   : 9832")
        self.assertIn("23552 B needed", wifi_lent_heap.lent_heap(22528, bigger, MOUNT)[0][0])

    def test_block_rounds_to_the_lent_heaps_classes(self):
        # 21,600 B: a 22,016 B block with 32 classes, but 22,528 B with the 16
        # that tlsf_create makes for a heap between 16 KiB and 256 KiB.
        context = DWARF.replace("byte_size   : 8832", "byte_size   : 9192")
        self.assertIn("23552 B needed", wifi_lent_heap.lent_heap(23040, context, MOUNT)[0][0])
        self.assertEqual([wifi_lent_heap.tlsf_block(21600, heap) for heap in (16384, 22528, 262145)],
                         [22528, 22528, 22016])

    def test_unreadable_context_size_fails(self):
        def fails(dwarf, sources, message):
            self.assertIn(message, wifi_lent_heap.lent_heap(22528, dwarf, sources)[0][0])
        fails(DWARF.replace("vfs_fat_ctx_t", "ctx"), MOUNT, "no typedef vfs_fat_ctx_t")
        fails(DWARF.replace("<221>   DW_AT_name        : FIL", "<221>   DW_AT_name        : FILE"), MOUNT,
              "no typedef FIL")
        fails(DWARF, {}, "found 0")
        fails(DWARF, {**MOUNT, "src/other.cpp": MOUNT["firmware/sd_library.cpp"]}, "found 2")
        fails(DWARF, {"firmware/sd_library.cpp": "SD.begin(cs, SPI, hz, kSdMountPoint);"}, "max_files as a literal")

class ObjectMetadata(unittest.TestCase):
    def test_archive_member_must_be_present_once(self):
        archive = "File: libapp.a(first.o)\nfirst\nFile: libapp.a(second.o)\nsecond\n"
        self.assertEqual(member_text(archive, "first.o"), "\nfirst\n")
        for text, member in ((archive, "missing.o"), (archive + archive, "first.o")):
            with self.assertRaisesRegex(RuntimeError, "Expected one relocation member"):
                member_text(text, member)

    def test_merged_literals_require_section_flags_no_relocations_and_bounded_placement(self):
        path = str(ROOT / ".build/rust-app/target/libapp.a") + "(app.o)"
        image = SimpleNamespace(inputs=[
            dict(name=".rodata.str1.1", address=0x1000, size=128, path=path),
            dict(name=".rodata.callbacks", address=0x1010, size=8, path=path)])
        header = "File: libapp.a(app.o)\n"
        section = "  [ 1] .rodata.str1.1 PROGBITS 00000000 000040 000080 01 AMS 0 0 1\n"
        self.assertEqual(literal_string_ranges(image, lambda _: header + section, ROOT), [(0x1000, 0x1010)])
        relocated = "Relocation section '.rela.rodata.str1.1' at offset 0x0:\n00000000 00000a01 R_RISCV_32 x + 0\n"
        for metadata in (section.replace("AMS", "AMSW"), section.replace("AMS", "AM"), section + relocated):
            self.assertEqual(literal_string_ranges(image, lambda _: header + metadata, ROOT), [])

    def test_project_archive_words_still_require_relocations_for_pointers(self):
        path = ".build/rust-app/target/libapp.a(app.o)"
        image = SimpleNamespace(nodes=[dict(kind="code", name="callback", address=0x42000000),
                                       dict(kind="data", name=".data.callbacks", address=0x1000)],
                                inputs=[dict(name=".data.callbacks", address=0x1000, path=path)],
                                words=lambda _: [(0x1000, 0x42000000)])
        header = "File: libapp.a(app.o)\n"
        self.assertEqual(link.chance_words(image, lambda _: header, ROOT), {0x1000})
        metadata = "Relocation section '.rela.data.callbacks' at offset 0x0:\n00000000 00000a01 R_RISCV_32 x + 0\n"
        self.assertEqual(link.chance_words(image, lambda _: header + metadata, ROOT), set())
        with self.assertRaisesRegex(RuntimeError, "Expected one relocation member"):
            link.chance_words(image, lambda _: "File: libapp.a(other.o)\n", ROOT)

    def test_sdk_and_other_non_build_archives_stay_conservative(self):
        image = SimpleNamespace(nodes=[dict(kind="code", name="callback", address=0x42000000),
                                       dict(kind="data", name=".rodata.str1.1", address=0x1000)],
                                inputs=[], words=lambda _: [(0x1000, 0x42000000)])
        for archive in (".toolchain/sdk/libc.a", "libapp.a", ".build/../.toolchain/sdk/libc.a",
                        "/external/sdk/libc.a"):
            for path in (archive, str(ROOT / archive)):
                with self.subTest(path=path):
                    image.inputs = [dict(name=".rodata.str1.1", address=0x1000, size=4, path=path + "(app.o)")]
                    read = mock.Mock(side_effect=AssertionError("External archives must not be read"))
                    self.assertEqual(link.chance_words(image, read, ROOT), set())
                    self.assertEqual(literal_string_ranges(image, read, ROOT), [])
                    read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
