#!/usr/bin/env python3
"""Derive Newt's linker script from the pinned ESP-IDF sections.ld (docs/084-wifi-memory.md).

Three edits, nothing else:
1. The .data/.bss of the files only Wi-Fi links (WIFI_FILES), the .bss inputs
   only Wi-Fi uses in files others share (WIFI_SECTIONS), and libphy's DRAM
   read-only data move into one output section, .dram0.newt_wifi, of at least
   LENT_FLOOR bytes. Every boot that is not update mode lends that whole
   section to the heap.
2. libphy's and libesp_ringbuf's IRAM code, and the IRAM functions in
   FLASH_FUNCTIONS, move to flash.
3. WIFI_CONSTRUCTORS' constructors leave boot's list for their own, which
   only update mode runs (firmware/wifi_memory.h).

The result goes to OUTPUT, whose directory the build's first -L flag names, so
the linker finds it before the framework's own sections.ld. It refuses any
pinned script or framework package other than the one it was written against,
and any anchor that is not found exactly once. The derived file keeps
Espressif's Apache-2.0 notices and states that Newt changed it.
"""
import argparse
import hashlib
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from freeink_sdk_routes import FIRMWARE_BUILD
from native_output import write_text

ROOT = Path(__file__).resolve().parents[1]
LIBS = ROOT / ".toolchain/platformio/packages/framework-arduinoespressif32-libs"
PINNED = {
    LIBS / "package.json": "b80d9ef9e2caf5a49356e2743feab17a26b99ffe07fdc7c5c5bac46431796cad",
    LIBS / "esp32c3/ld/sections.ld": "ab122c0802f444466937d5bd405c46f944657cb52eddde7bff0ec8191fcb2b68",
}
OUTPUT = ROOT / ".build" / FIRMWARE_BUILD / "ld/sections.ld"
# Input files that only Wi-Fi links: none of their data is kept in a build
# without Wi-Fi (docs/084-wifi-memory.md). verify-wifi-link.py fails the build
# if anything else lands in .dram0.newt_wifi or anything outside Wi-Fi uses it.
WIFI_FILES = (
    "*libcore.a", "*libesp_netif.a", "*libesp_wifi.a", "*libespnow.a", "*liblwip.a",
    "*libmesh.a", "*libnet80211.a", "*libphy.a", "*libpp.a", "*libsmartconfig.a",
    "*libwpa_supplicant.a", "*libesp_event.a:default_event_loop.c.obj",
    "*libesp_hw_support.a:sar_periph_ctrl_common.c.obj",
    "*libesp_phy.a:phy_common.c.obj", "*libesp_phy.a:phy_init.c.obj",
    "*libmbedcrypto.a:ecp.c.obj", "*libmbedcrypto.a:esp_aes_dma_core.c.obj",
    "*libmbedcrypto.a:esp_bignum.c.obj", "*libmbedcrypto.a:platform.c.obj",
    # Update mode's GitHub check (docs/088-ota-wifi-install.md): HTTP, TLS, the
    # certificate bundle and the DS peripheral code esp-tls links.
    "*libesp_http_client.a", "*libesp-tls.a", "*libmbedtls.a", "*libmbedtls_2.a",
    "*libmbedcrypto.a:esp_ds_common.c.obj",
)
# .bss inputs only Wi-Fi uses, in a file other code shares: the DS and HMAC
# locks esp-tls's DS path takes (reading boots take the SHA/AES and MPI locks).
# .bss only, since .dram0.data's .data/.sdata rules come first.
WIFI_SECTIONS = {"*libesp_security.a:esp_crypto_lock.c.obj": ".sbss.s_crypto_ds_lock .sbss.s_crypto_hmac_lock"}
# Wi-Fi files whose constructors a reading boot must not run: esp-tls's DS
# code makes its lock, a heap mutex, in one, which every boot would keep unused.
WIFI_CONSTRUCTORS = ("*libmbedcrypto.a:esp_ds_common.c.obj",)
FLASH_ARCHIVES = ("libphy.a", "libesp_ringbuf.a")
# Single IRAM functions that run from flash: (file, its .iram1.<digit> input in
# the pinned archive, the function). Only flash code calls each; tools/
# verify-wifi-link.py fails the build if one stays in IRAM or IRAM code calls it.
FLASH_FUNCTIONS = (("libheap.a:heap_caps.c.obj", ".iram1.4", "heap_caps_malloc_prefer"),)
DATA = ".sdata .sdata.* .data .data.* .dram1 .dram1.*"
BSS = ".sbss .sbss.* .bss .bss.* COMMON .scommon"
PHY_RODATA = "    *libphy.a:(.rodata .rodata.* .sdata2 .sdata2.* .srodata .srodata.*)\n"
START, END = "_newt_wifi_start", "_newt_wifi_end"
INIT_START, INIT_END = "_newt_wifi_init_start", "_newt_wifi_init_end"
# The lent heap must hold the SD card's FATFS context, which a reading boot
# allocates after the lend and keeps: the RAM heap has no room left for it, and
# in the large heap it would split the block the reader opens books in
# (docs/084-wifi-memory.md). Zero padding makes up any shortfall; a bigger
# Wi-Fi range needs none. tools/verify-wifi-link.py fails the health build if
# the linked context needs more.
LENT_FLOOR = 22528


def statement(name, sections):
    # A whole archive takes "archive:" in an input statement; EXCLUDE_FILE takes it bare.
    return f"    {name}{':' if name.endswith('.a') else ''}({sections})\n"


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError(f"Pinned sections.ld anchor not found exactly once: {old[:70]!r}")
    return text.replace(old, new)


def carve_out(text, force_load=False):
    """Keep Wi-Fi's .data out of .dram0.data and give it, its .bss and libphy's
    DRAM rodata one section after .dram0.data. Explicit .bss statements there
    run before .dram0.bss's wildcards, so they take Wi-Fi's .bss first."""
    files = " ".join(WIFI_FILES)
    text = replace_once(text, "    *(.sdata)\n    *(.sdata.*)\n",
                        f"    *(EXCLUDE_FILE({files}) .sdata EXCLUDE_FILE({files}) .sdata.*)\n")
    bt = "*libbt.a *libbtdm_app.a"
    text = replace_once(text, f"*(EXCLUDE_FILE({bt}) .data EXCLUDE_FILE({bt}) .data.*)",
                        f"*(EXCLUDE_FILE({bt} {files}) .data EXCLUDE_FILE({bt} {files}) .data.*)")
    text = replace_once(text, "    *(.dram1 .dram1.*)\n",
                        f"    *(EXCLUDE_FILE({files}) .dram1 EXCLUDE_FILE({files}) .dram1.*)\n")
    text = replace_once(text, PHY_RODATA, "")
    # Zero-filled .bss in a loaded section costs flash, but keeps the lent range
    # one section with one start and end that startup code never touches.
    section = ("  .dram0.newt_wifi ALIGN(8) :\n  {\n    " + START + " = ABSOLUTE(.);\n"
               + "".join(statement(name, DATA) for name in WIFI_FILES) + PHY_RODATA
               + "".join(statement(name, BSS) for name in WIFI_FILES)
               + "".join(statement(name, sections) for name, sections in WIFI_SECTIONS.items())
               # With no initialized Wi-Fi input, ld otherwise emits NOBITS.
               # A byte in the existing floor padding forces a loaded segment.
               + ("    BYTE(0);\n" if force_load else "")
               + "    . = ALIGN(8);\n"
               # Inside a section "." counts from the section's start, which
               # verify-wifi-link.py requires to be _newt_wifi_start.
               + f"    . = MAX(., {LENT_FLOOR});\n"
               + "    " + END + " = ABSOLUTE(.);\n  } > dram0_0_seg\n")
    data_end = "    _data_end = ABSOLUTE(.);\n  } > dram0_0_seg\n"
    return replace_once(text, data_end, data_end + section)


def all_iram_but(section):
    """ld patterns for every .iram1 input of a file except `section` (.iram1.<digit>)."""
    digit = section.removeprefix(".iram1.")
    if len(digit) != 1 or not digit.isdigit():
        raise RuntimeError(f"Unsupported IRAM input for FLASH_FUNCTIONS: {section}")
    return f".iram1 .iram1.[!{digit}]* .iram1.{digit}?*"


def code_to_flash(text):
    """Run FLASH_ARCHIVES' code and FLASH_FUNCTIONS from flash: drop the
    archives' IRAM rules and flash exclusions, keep the functions' files' other
    IRAM code in IRAM, and place the moved code ahead of .flash.text's Wi-Fi rules."""
    iram_at, flash_at = text.index("  .iram0.text :\n"), text.index("  .flash.text :\n")
    iram_end, flash_end = text.index("  } > ", iram_at), text.index("  } > ", flash_at)
    iram, flash = text[iram_at:iram_end], text[flash_at:flash_end]
    for name in FLASH_ARCHIVES:
        iram = "".join(line for line in iram.splitlines(True) if not line.startswith(f"    *{name}:"))
        # Only bare EXCLUDE_FILE tokens: a pattern followed by "(" is a placement rule.
        flash = re.sub(r"\*" + re.escape(name) + r"(:[^\s()]*)?( |(?=\)))", "", flash)
    excluded = " ".join("*" + name for name in FLASH_ARCHIVES + tuple(file for file, _, _ in FLASH_FUNCTIONS))
    kept = "".join(f"    *{file}({all_iram_but(section)})\n" for file, section, _ in FLASH_FUNCTIONS)
    iram = replace_once(iram, "    *(.iram1 .iram1.*)\n",
                        f"    *(EXCLUDE_FILE({excluded}) .iram1 EXCLUDE_FILE({excluded}) .iram1.*)\n" + kept)
    moved = "".join(f"    *{name}:(.iram1 .iram1.*)\n" for name in FLASH_ARCHIVES)
    moved += "".join(f"    *{file}({section})\n" for file, section, _ in FLASH_FUNCTIONS)
    flash = replace_once(flash, "    *(.wifi0iram .wifi0iram.*)\n", moved + "    *(.wifi0iram .wifi0iram.*)\n")
    return text[:iram_at] + iram + text[iram_end:flash_at] + flash + text[flash_end:]


def defer_constructors(text):
    """Keep WIFI_CONSTRUCTORS' .init_array out of boot's two constructor lists
    and give them a list of their own, right before: after the lists, its
    start would be __init_array_end, which boot's code refers to."""
    files = " ".join(WIFI_CONSTRUCTORS)
    for kind in (".init_array.*", ".init_array"):
        boot = f"    KEEP (*(EXCLUDE_FILE (*crtend.* *crtbegin.*) {kind}))\n"
        text = replace_once(text, boot, boot.replace("*crtbegin.*)", f"*crtbegin.* {files})"))
    boot = " . = ALIGN(4); \n __init_priority_array_start = ABSOLUTE(.);\n"
    own = (f"    . = ALIGN(4);\n    {INIT_START} = ABSOLUTE(.);\n"
           + "".join(f"    KEEP ({name}(.init_array .init_array.*))\n" for name in WIFI_CONSTRUCTORS)
           + f"    {INIT_END} = ABSOLUTE(.);\n")
    return replace_once(text, boot, own + boot)


def derive(text, force_load=False):
    header = ("/* Derived by Newt's tools/wifi-sections.py from the pinned ESP-IDF 5.5.2 sections.ld\n"
              " * (framework-arduinoespressif32-libs 5.5.0+sha.87912cd291), Apache-2.0 like the original.\n"
              " * Changed by Newt: Wi-Fi-only .data/.bss and libphy DRAM rodata in .dram0.newt_wifi\n"
              f" * (at least {LENT_FLOOR} B);\n"
              f" * libphy and libesp_ringbuf IRAM code and {', '.join(f for _, _, f in FLASH_FUNCTIONS)} in flash.\n"
              " * Wi-Fi-only constructors in their own list, which update mode runs.\n"
              " * See docs/084-wifi-memory.md. */\n")
    return header + defer_constructors(code_to_flash(carve_out(text, force_load=force_load)))


def pinned_text(root=ROOT):
    for path, expected in PINNED.items():
        selected = root / path.relative_to(ROOT)
        if hashlib.sha256(selected.read_bytes()).hexdigest() != expected:
            raise RuntimeError(f"Framework input differs from the one tools/wifi-sections.py was written for: {selected}")
    return (root / LIBS.relative_to(ROOT) / "esp32c3/ld/sections.ld").read_text()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust-app', action='store_true', help='the Rust image\'s existing loaded lend section')
    args = parser.parse_args()
    output = ROOT / '.build/rust-app/ld/sections.ld' if args.rust_app else OUTPUT
    write_text(ROOT, output, derive(pinned_text(), force_load=args.rust_app))


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Wi-Fi linker script stopped: {error}", file=sys.stderr)
        sys.exit(1)
