"""The lent Wi-Fi range (.dram0.newt_wifi) is not resident RAM: reported and
capped as wifi_lent_bytes, a ceiling of its own (docs/CODE_HEALTH.md "Memory policy")."""
import copy
import unittest

from resource_fixture import BUILD, SIZE_OUTPUT, ResourceFixture, recorded
import code_health_resources as resources

# The fixture's link with a 40 B lent range between .dram0.data and .dram0.bss.
LENT_OUTPUT = SIZE_OUTPUT.replace(".dram0.bss 80 1070071888", ".dram0.newt_wifi 40 1070071888\n.dram0.bss 80 1070071928")


class LentRangeTests(ResourceFixture):
    def lend(self, ceiling=40):
        """Build with the lent range and give it a recorded ceiling."""
        self.command.return_value.stdout = LENT_OUTPUT
        self.report = self.build(BUILD)
        self.save()
        self.budget["resources"][BUILD]["wifi_lent_bytes"] = ceiling
        recorded(self.budget)

    def test_lent_range_is_reported_apart_from_resident_ram(self):
        plain = (self.report["ram_bytes"], self.report["ram_alignment_bytes"], self.report["wifi_lent_bytes"])
        self.lend()
        self.assertEqual(plain, (160, 0, 0))
        # .dram0.bss moved up by the lent 40 B; reading still pays 160.
        self.assertEqual((self.report["ram_bytes"], self.report["ram_alignment_bytes"],
                          self.report["wifi_lent_bytes"]), (160, 0, 40))
        self.assertNotIn(".dram0.newt_wifi", self.report["ram_sections"])
        self.assertEqual(self.check(), [])
        for label, output in (
                ("twice", LENT_OUTPUT.replace(".dram0.bss 80", ".dram0.newt_wifi 0 1070071928\n.dram0.bss 80")),
                ("outside SRAM", LENT_OUTPUT.replace("40 1070071888", "40 1342177280")),
                ("overlapping", LENT_OUTPUT.replace(".dram0.bss 80 1070071928", ".dram0.bss 80 1070071900"))):
            self.command.return_value.stdout = output
            with self.subTest(lent=label), self.assertRaises(ValueError):
                self.build(BUILD)

    def test_lent_range_needs_its_own_ceiling(self):
        self.command.return_value.stdout = LENT_OUTPUT
        self.report = self.build(BUILD)
        self.save()
        self.assertEqual(self.check(), [f"{BUILD} wifi_lent_bytes: 40 exceeds reviewed ceiling 0"])
        self.lend()
        self.assertEqual(self.check(), [])
        self.command.return_value.stdout = LENT_OUTPUT.replace("newt_wifi 40", "newt_wifi 48").replace(
            "1070071928", "1070071936")
        self.report = self.build(BUILD)
        self.save()
        self.assertEqual(self.check(), [f"{BUILD} wifi_lent_bytes: 48 exceeds reviewed ceiling 40"])

    def test_adoption_binds_the_added_lent_ceiling_to_the_fresh_build(self):
        base = copy.deepcopy(self.budget)
        self.lend()
        adopt = lambda: resources.check_resources(self.root, self.budget, BUILD, base)
        self.assertEqual(adopt(), [])
        self.budget["resources"][BUILD]["wifi_lent_bytes"] += 8
        recorded(self.budget)
        self.assertEqual(self.check(), [])
        self.assertTrue(any(f"{BUILD} wifi_lent_bytes: raised ceiling 48" in failure for failure in adopt()))


if __name__ == "__main__":
    unittest.main()
