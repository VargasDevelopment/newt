"""Fault controls for tools/verify-next-boot.py (docs/UPDATES.md, "Secrets in RAM"): inputs
in the linked image's shape pass, and each broken fact fails."""
import importlib.util
from pathlib import Path
import struct
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("next_boot", ROOT / "tools/verify-next-boot.py")
next_boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(next_boot)

START, END = 0x3fc8f348, 0x3fc94b48
# As linked: .dram0.data's segment runs into the range, a second one ends it.
SEGMENTS = [(0x3c110020, 0x40), (0x3fc8d000, 0x376c), (0x3fc9076c, 0x43dc), (0x40380000, 0x20)]
# objdump -d of FreeRTOS's task layout, trimmed to what the check reads.
DISASSEMBLY = """
40385f76 <prvInitialiseNewTask.constprop.0>:
40385f7c:\t03082503          \tlw\ta0,48(a6)
40385f82:\t8b2e                \tmv\ts6,a1
40385f84:\t0a500593          \tli\ta1,165
40385f9e:\tffc7a097          \tauipc\tra,0xffc7a
40385fa2:\t3b6080e7          \tjalr\t950(ra) # 40000354 <memset>
40385fb2:\t020b0263          \tbeqz\ts6,40385fd6 <prvInitialiseNewTask.constprop.0+0x60>
40386032:\ta62ff0ef          \tjal\t40385294 <pxPortInitialiseStack>
40386052:\t8082                \tret

40385294 <pxPortInitialiseStack>:
40385298:\t00e7f363          \tbgeu\ta5,a4,4038529e <pxPortInitialiseStack+0xa>
4038529c:\t8082                \tret
"""
# objdump -d of IDF's heap registration and Newt's wraps, trimmed likewise.
HEAP = """
4207cfe8 <heap_caps_init>:
4207d010:\taddb80ef          \tjal\t42035aec <__wrap_soc_get_available_memory_regions>

4212ab50 <main_task>:
4212ab70:\tfa30a0ef          \tjal\t42035b12 <__wrap_heap_caps_enable_nonos_stack_heaps>

42035aec <__wrap_soc_get_available_memory_regions>:
42035af6:\t0f1470ef          \tjal\t4207d3e6 <soc_get_available_memory_regions>
42035afe:\t4601                \tli\ta2,0
42035b02:\tfa9ff0ef          \tjal\t42035aaa <_ZN4newt8firmware16clearHeapRegionsEPK19soc_memory_region_tjb>
42035b10:\t8082                \tret

42035b12 <__wrap_heap_caps_enable_nonos_stack_heaps>:
42035b2c:\t099470ef          \tjal\t4207d3c4 <soc_get_available_memory_region_max_count>
42035b46:\t0a1470ef          \tjal\t4207d3e6 <soc_get_available_memory_regions>
42035b4c:\t4605                \tli\ta2,1
42035b50:\tf5bff0ef          \tjal\t42035aaa <_ZN4newt8firmware16clearHeapRegionsEPK19soc_memory_region_tjb>
42035b54:\t448470ef          \tjal\t4207cf9c <heap_caps_enable_nonos_stack_heaps>
42035b64:\tc789                \tbeqz\ta5,42035b6e <__wrap_heap_caps_enable_nonos_stack_heaps+0x5c>
42035b7c:\t8082                \tret

42035aaa <_ZN4newt8firmware16clearHeapRegionsEPK19soc_memory_region_tjb>:
42035ade:\t4581                \tli\ta1,0
42035ae0:\tfdfcb097          \tauipc\tra,0xfdfcb
42035ae4:\t874080e7          \tjalr\t-1932(ra) # 40000354 <memset>
42035aea:\tbfe1                \tj\t42035ac2 <_ZN4newt8firmware16clearHeapRegionsEPK19soc_memory_region_tjb+0x18>
"""
SDKCONFIG = """CONFIG_ESP_COREDUMP_ENABLE_TO_FLASH=y
CONFIG_ESP_COREDUMP_DATA_FORMAT_ELF=y
# CONFIG_ESP_COREDUMP_CAPTURE_DRAM is not set
CONFIG_ESP_COREDUMP_CHECK_BOOT=y
"""


def image(segments) -> bytes:
    data = bytes([0xE9, len(segments), 2, 0x40]) + struct.pack("<I", 0x4038089e) + bytes(16)
    for address, length in segments:
        data += struct.pack("<II", address, length) + bytes(length)
    return data


def stacks(text: str) -> list:
    return next_boot.stacks_filled(next_boot.parse_functions(text))


def heap(text: str) -> list:
    return next_boot.heap_cleared(next_boot.parse_functions(text))


class NextBootTest(unittest.TestCase):
    def test_linked_shape_passes(self):
        self.assertEqual(next_boot.image_segments(image(SEGMENTS)), SEGMENTS)
        self.assertEqual(next_boot.reloaded(SEGMENTS, START, END), [])
        self.assertEqual(stacks(DISASSEMBLY), [])
        self.assertEqual(heap(HEAP), [])
        self.assertEqual(next_boot.coredump_dram(SDKCONFIG), [])

    def test_range_left_out_of_the_image_fails(self):
        # The range moved to a NOBITS section: its tail is no longer loaded.
        tail = [(0x3c110020, 0x40), (0x3fc8d000, 0x376c), (0x40380000, 0x20)]
        self.assertIn("from 0x3fc9076c to 0x3fc94b48", next_boot.reloaded(tail, START, END)[0])
        head = [(0x3fc90000, END - 0x3fc90000)]
        self.assertIn("from 0x3fc8f348", next_boot.reloaded(head, START, END)[0])
        self.assertTrue(next_boot.reloaded(SEGMENTS, END, END))
        with self.assertRaises(RuntimeError):
            next_boot.image_segments(image(SEGMENTS)[:40])

    def test_unfilled_stack_fails(self):
        for broken in (DISASSEMBLY.replace("li\ta1,165", "li\ta1,0"),
                       DISASSEMBLY.replace("<memset>", "<memcpy>"),
                       DISASSEMBLY.replace("auipc\tra,0xffc7a", "mv\ta1,a2")):
            self.assertIn("prvInitialiseNewTask.constprop.0 lays out", stacks(broken)[0])
        # A second layout path that does not fill.
        extra = DISASSEMBLY + "\n40387000 <xTaskCreateRaw>:\n40387004:\ta62ff0ef          \tjal\t40385294 <pxPortInitialiseStack>\n"
        self.assertEqual(stacks(extra), ["xTaskCreateRaw lays out a task stack without first filling it "
                                         "with 0xa5 through memset"])
        self.assertIn("has moved", stacks(DISASSEMBLY.replace("<pxPortInitialiseStack>", "<other>"))[0])

    def test_uncleared_heap_fails(self):
        clear = "42035b50:\tf5bff0ef          \tjal\t42035aaa <%s>\n" % next_boot.CLEAR
        register = "42035b54:\t448470ef          \tjal\t4207cf9c <heap_caps_enable_nonos_stack_heaps>\n"
        regions_wrap = "does not clear the regions it returns"
        for broken, expected in (
                # The clear removed, or moved after the registration.
                (HEAP.replace("42035b02:\tfa9ff0ef          \tjal\t42035aaa <%s>\n" % next_boot.CLEAR, ""), regions_wrap),
                (HEAP.replace(clear + register, register + clear), "does not clear the startup stack's regions"),
                # The flags swapped: the live startup stack cleared, the heap not.
                (HEAP.replace("li\ta2,0", "li\ta2,1"), regions_wrap),
                (HEAP.replace("li\ta2,1", "li\ta2,0"), "does not clear the startup stack's regions"),
                (HEAP.replace("li\ta1,0", "li\ta1,165"), "does not zero its regions"),
                (HEAP.replace("<memset>", "<memcpy>"), "does not zero its regions"),
                # IDF reaching the regions without the wrap, or the wrap unused.
                (HEAP.replace("addb80ef          \tjal\t42035aec <__wrap_soc_get_available_memory_regions>",
                              "addb80ef          \tjal\t4207d3e6 <soc_get_available_memory_regions>"),
                 "heap_caps_init does not reach soc_get_available_memory_regions only through"),
                (HEAP.replace("jal\t42035b12 <__wrap_heap_caps_enable_nonos_stack_heaps>", "ret"),
                 "has callers [], not only main_task"),
                # A caller after startup would zero live heap.
                (HEAP + "\n42090000 <heap_caps_print_heap_info>:\n42090004:\taddb80ef          \tjal\t"
                        "42035aec <__wrap_soc_get_available_memory_regions>\n",
                 "has callers ['heap_caps_init', 'heap_caps_print_heap_info'], not only heap_caps_init")):
            self.assertTrue(any(expected in failure for failure in heap(broken)), (expected, heap(broken)))

    def test_dram_core_dump_fails(self):
        enabled = SDKCONFIG.replace("# CONFIG_ESP_COREDUMP_CAPTURE_DRAM is not set",
                                    "CONFIG_ESP_COREDUMP_CAPTURE_DRAM=y")
        self.assertTrue(next_boot.coredump_dram(enabled))
        self.assertTrue(next_boot.coredump_dram(SDKCONFIG.replace("# CONFIG_ESP_COREDUMP_CAPTURE_DRAM is not set\n", "")))
        self.assertTrue(next_boot.coredump_dram(SDKCONFIG + "CONFIG_ESP_COREDUMP_CAPTURE_DRAM=y\n"))


if __name__ == "__main__":
    unittest.main()
