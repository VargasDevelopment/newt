"""Fault controls for the linked image keeping every flash write and NVS open
behind the Wi-Fi fence (docs/084-wifi-memory.md, "Wi-Fi never writes flash")."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("native_guard", ROOT / "tools/verify-nvs-guard.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)

OPEN_HANDLE = guard.NVS_OPEN_HANDLE
# Shaped on the pinned image's own lines: tail jumps (j, jr) and long calls.
FENCE = """42050600 <__wrap_nvs_open>:
42050610:\t1262206f          \tj\t42082736 <nvs_open>
42050628 <__wrap_esp_partition_write>:
4205063e:\te919                \tbnez\ta0,42050654 <__wrap_esp_partition_write+0x2c>
42050650:\t75f2506f          \tj\t420765ae <esp_partition_write>
420765ae <esp_partition_write>:
42076602:\td7a30067          \tjr\t-646(t1) # 4038b378 <esp_flash_write>
4208267c <nvs_open_from_partition>:
420826ac:\t655020ef          \tjal\t42085500 <{handle}>
42082736 <nvs_open>:
42082744:\tf39ff06f          \tj\t4208267c <nvs_open_from_partition>
4208b7f8 <esp_core_dump_flash_custom_write>:
4208b834:\tcee30067          \tjr\t-786(t1) # 4038b51e <esp_flash_write_encrypted>
""".format(handle=OPEN_HANDLE)
PHY_STORE = """420952ca <esp_phy_store_cal_data_to_nvs>:
420952e2:\tc54ed0ef          \tjal\t{target}
"""
FENCED_PHY = PHY_STORE.format(target="42050600 <__wrap_nvs_open>")
READER = "42010000 <readerTask>:\n"


class FlashWriterGuard(unittest.TestCase):
    def check(self, code):
        return guard.validate_flash_writers(guard.parse_functions(code))

    def reject(self, code, message="bypasses the Wi-Fi fence"):
        with self.assertRaisesRegex(RuntimeError, message):
            self.check(code)

    def test_fenced_image_passes(self):
        edges = self.check(FENCE + FENCED_PHY)
        self.assertEqual(edges["nvs_open"], ["__wrap_nvs_open"])
        self.assertEqual(edges["esp_flash_write"], ["esp_partition_write"])
        self.assertEqual(edges["esp_flash_write_encrypted"], ["esp_core_dump_flash_custom_write"])

    def test_unwrapped_phy_store_is_rejected(self):
        # eabd52f3's link: the PHY's calibration store opens NVS directly.
        self.reject(FENCE + PHY_STORE.format(target="42082736 <nvs_open>"))

    def test_phy_store_outside_the_fence_is_rejected(self):
        self.reject(FENCE + PHY_STORE.format(target="42082000 <nvs_set_blob>"), "through the Wi-Fi fence")

    def test_missing_fence_edge_is_rejected(self):
        self.reject(FENCE.replace("<nvs_open>\n", "<nvs_close>\n", 1) + FENCED_PHY, "__wrap_nvs_open -> nvs_open")

    def test_tail_jump_and_address_load_to_flash_are_rejected(self):
        tail = "42010010:\td7a30067          \tjr\t-646(t1) # 4038b378 <esp_flash_write>\n"
        self.assertEqual(guard.ordered_calls([tail]), ["esp_flash_write"])  # direct tail jumps count too
        self.reject(FENCE + FENCED_PHY + READER + tail)
        self.reject(FENCE + FENCED_PHY + READER + "42010010:\t3780051b          \taddi\ta0,a0,888 # 4038b378 <esp_flash_erase_region>\n")
        self.reject(FENCE + FENCED_PHY + READER + "42010010:\t3780051b          \tjal\t4038b388 <esp_rom_spiflash_write+0x10>\n")

    def test_unfenced_partition_write_is_rejected(self):
        self.reject(FENCE + FENCED_PHY + READER + "42010010:\t75f2506f          \tj\t420765ae <esp_partition_write>\n")

    def test_nvs_opens_beside_nvs_open_are_rejected(self):
        self.reject(FENCE + FENCED_PHY + READER + "42010010:\tf39ff06f          \tjal\t4208267c <nvs_open_from_partition>\n")
        self.reject(FENCE + FENCED_PHY + "42083000 <nvs::open_nvs_handle_from_partition>:\n"
                    f"42083010:\t655020ef          \tjal\t42085500 <{OPEN_HANDLE}>\n")


if __name__ == "__main__":
    unittest.main()
