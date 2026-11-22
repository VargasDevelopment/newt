#!/usr/bin/env python3
"""Verify Wi-Fi's memory carve-out in the linked C3 image (docs/UPDATES.md, "Wi-Fi memory"); no hardware access.

Each boot that is not update mode lends .dram0.newt_wifi to the heap
(firmware/wifi_memory.h). That is safe only while nothing else reads, writes
or calls into it; and with libphy, libesp_ringbuf and FLASH_FUNCTIONS code
moved to flash, the C3 stays cache-safe only while IRAM code never calls it. From the ELF and map:

1. Lent range: [_newt_wifi_start, _newt_wifi_end) is all of .dram0.newt_wifi
   and holds only WIFI_FILES' .data/.bss, WIFI_SECTIONS, libphy's DRAM
   rodata (merged strings and constants too) and zero padding
   (tools/wifi-sections.py).
   gp-relative code cannot reach it.
2. Wi-Fi code: WIFI_FILES' functions and data, WIFI_SECTIONS' data, and
   every function only Wi-Fi code refers to. No other function calls it, takes one of its addresses or
   refers to its data, and no other data word points at either, except
   ENTRIES. A library entry (update mode's AES-GCM) may share Wi-Fi's library
   code; its every caller is update-mode code (what only runUpdateMode reaches).
   Nor does any refer to an address anywhere in the lent range, whatever lies
   there: padding and merged constants have no node. Each startup root into
   Wi-Fi code (constructor or system init function) runs before the lend and
   may call exactly STARTUP's functions outside Wi-Fi code.
3. IRAM: no libphy, libesp_ringbuf or FLASH_FUNCTIONS function is in IRAM,
   and no IRAM function calls one or takes its address.
4. Lent heap: the range holds the SD card's FATFS context, which the first
   mount allocates for the whole boot (tools/wifi_lent_heap.py). Its size
   comes from the ELF's DWARF and the firmware's only SD.begin call, so a
   larger context fails the build instead of splitting the large heap.

Limits: a reference is what the linked code spells: a call or jump, an
address objdump resolves, a lui/auipc pair within a function, a gp offset or
an aligned data word (in Newt's own objects, one they relocate: tools/
object_relocations.py). A pointer built by other arithmetic, or kept by ROM
code, is not seen, and an indirect call's target is unknown; the receipt
counts IRAM's indirect calls. Device behavior is a separate claim.
"""
import argparse
import fnmatch
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from freeink_sdk_routes import FIRMWARE_BUILD
from native_output import owned_file, write_text
from object_relocations import chance_words
from wifi_lent_heap import firmware_sources, lent_heap
from wifi_link_image import IRAM, STOPPED, Image, indirect_calls, references

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / ".toolchain/platformio/packages/toolchain-riscv32-esp/bin"
_spec = importlib.util.spec_from_file_location("wifi_sections", ROOT / "tools/wifi-sections.py")
wifi_sections = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wifi_sections)
WIFI_FILES = wifi_sections.WIFI_FILES
WIFI_SECTIONS = wifi_sections.WIFI_SECTIONS
FLASH_ARCHIVES = wifi_sections.FLASH_ARCHIVES
FLASH_FUNCTIONS = {function: file for file, _, function in wifi_sections.FLASH_FUNCTIONS}
LENT = ".dram0.newt_wifi"
LENT_INPUTS = (".sdata", ".data", ".dram1", ".sbss", ".bss", "COMMON", ".scommon")
PHY_RODATA = (".rodata", ".sdata2", ".srodata")
ENTER = "(anonymous namespace)::enterRequestedUpdateMode(esp_reset_reason_t)"
RUN = "(anonymous namespace)::runUpdateMode(newt::firmware::UpdateModeRequest)"
SCAN = "(anonymous namespace)::scanUpdateNetworks(newt::UpdateView&)"
CONNECT = "(anonymous namespace)::connectUpdateNetwork(newt::UpdateView&)"
CHECK = "(anonymous namespace)::checkUpdateOnline(newt::UpdateView&)"
FETCH = "(anonymous namespace)::fetchUpdateOnline(void*)"
JOIN = ("newt::firmware::joinWifiNetwork(newt::WifiChoice const&, newt::Secret<64u>&, void (*)(), "
        "void (*)(void*), void*)")
SEAL = ("newt::firmware::MbedWifiCipher::seal(unsigned char const*, unsigned char const*, unsigned char const*, "
        "unsigned int, unsigned char const*, unsigned int, unsigned char*, unsigned char*)")
OPEN = ("newt::firmware::MbedWifiCipher::open(unsigned char const*, unsigned char const*, unsigned char const*, "
        "unsigned int, unsigned char const*, unsigned int, unsigned char const*, unsigned char*)")
UPDATE = "update mode"  # callers: runUpdateMode and what only it reaches
# Functions outside Wi-Fi code that may refer to it, their only callers, and what of the lent range each may name.
ENTRIES = {
    # firmware/update_wifi.h: update mode's scan, its join and its exit's Wi-Fi stop (leaveUpdateMode).
    "newt::firmware::scanWifiNetworks(newt::UpdateView&)": {"callers": {SCAN}, "range": None},
    JOIN: {"callers": {CONNECT, CHECK}, "range": None},
    # update_mode_runtime.h: the GitHub check's HTTPS (firmware/update_https.h) runs over Wi-Fi's
    # sockets while the join holds an address; the HTTP, TLS and certificate code only it reaches
    # counts as Wi-Fi's.
    FETCH: {"callers": {CHECK}, "range": None, "library": True},
    "newt::firmware::forgetWifiBeforeRestart()": {"callers": UPDATE, "range": None},
    # firmware/wifi_memory.h: takes the range's start address to lend it.
    "newt::firmware::lendWifiMemory()": {"callers": {ENTER}, "range": "start"},
    # firmware/wifi_memory.h: runs the Wi-Fi constructors boot skips (esp-tls's DS lock) as update mode starts.
    "newt::firmware::runWifiConstructors()": {"callers": {RUN}, "range": None},
    # firmware/wifi_network_device.h: AES-GCM uses Wi-Fi's mbedTLS allocator hooks and AES DMA
    # state, safe since update mode never lends the range.
    SEAL: {"callers": UPDATE, "range": None, "library": True},
    OPEN: {"callers": UPDATE, "range": None, "library": True},
}
# The scan's, join's and check's only path: update mode, which never returns, so no reading boot reaches them.
UPDATE_ONLY = {SCAN: {RUN}, CONNECT: {RUN}, CHECK: {RUN}, RUN: {ENTER}}
# Startup roots into Wi-Fi code, which run in every boot before the lend, and every function outside
# Wi-Fi code each may reach. A root that held heap, started a task or registered a callback would
# break the lend, so a change needs review.
STARTUP = {
    # esp_wifi wifi_init.c's constructor sets four Wi-Fi log tags' levels. Each takes a small
    # heap entry, which initArduino's esp_log_level_set("*", ...) frees before setup() runs.
    "s_set_default_wifi_log_level": {"esp_log_level_set"},
}
STARTUP_ARRAYS = (("__init_array_start", "__init_array_end"),
                  ("_esp_system_init_fn_array_start", "_esp_system_init_fn_array_end"))
FLASH = (0x3c000000, 0x3c800000)  # the C3's flash data bus: constants
def matches(name: str, pattern: str) -> bool:
    archive, _, member = pattern.lstrip("*").partition(":")
    return fnmatch.fnmatchcase(name, f"{archive}({member or '*'})")


def wifi_file(name: str) -> bool:
    return any(matches(name, pattern) for pattern in WIFI_FILES)


def wifi_input(name: str, section: str) -> bool:
    """A WIFI_FILES input, or one of WIFI_SECTIONS' named sections."""
    return wifi_file(name) or any(matches(name, pattern) and section in sections.split()
                                  for pattern, sections in WIFI_SECTIONS.items())


def rust_entries(reading_only=False):
    """Concrete native SDK leaves used only by the separate Rust update boot.

    Keep these names explicit: a new caller or SDK route still needs review.
    The graph and memory predicates below are shared with the C++ image.
    """
    run = "newt_app_image::app_startup::run_update"
    selector = "newt_native_input_owner::deferred_start::body"
    leaves = (
        "newt_wifi_create_loop", "newt_wifi_delete_loop", "newt_wifi_init_no_nvs", "newt_wifi_usb_bbpll",
        "newt_wifi_storage_ram", "newt_wifi_mode_station", "newt_wifi_start", "newt_wifi_scan",
        "newt_wifi_scan_count", "newt_wifi_scan_record", "newt_wifi_clear_ap_list", "newt_wifi_stop",
        "newt_wifi_deinit", "newt_wifi_clear_config", "newt_wifi_netif_init", "newt_wifi_join_station",
        "newt_wifi_join_register_wifi", "newt_wifi_join_register_ip", "newt_wifi_join_set_config",
        "newt_wifi_connect", "newt_wifi_disconnect", "newt_wifi_join_destroy_station",
        "newt_wifi_join_unregister_ip", "newt_wifi_join_unregister_wifi",
        "nh_http_init", "nh_http_set_header", "nh_http_open", "nh_http_set_context", "nh_http_headers",
        "nh_http_status", "nh_http_read", "nh_http_complete", "nh_http_close", "nh_http_cleanup",
        "nh_socket_shutdown", "nh_socket_sem_deinit", "nh_tls_install", "nh_tls_restore",
    )
    entries = {name: {"callers": UPDATE, "range": None} for name in leaves}
    # These native functions register real callbacks into the SDK graph.
    for name in ("newt_wifi_join_register_wifi", "newt_wifi_join_register_ip", "nh_http_init"):
        entries[name]["library"] = True
    for name in ("newt_wifi_gcm_open", "newt_wifi_gcm_seal", "newt_app_image::update_online_runtime::fetch"):
        entries[name] = {"callers": UPDATE, "range": None, "library": True}
    entries["newt_app_image::wifi_memory::lend"] = {"callers": {selector}, "range": "start"}
    entries["newt_app_image::wifi_memory::run_constructors"] = {"callers": {run}, "range": None}
    if reading_only:
        # Owner's first bench excludes the update boot. Keep the full memory,
        # caller, constructor and IRAM analysis; only the lend entry may exist.
        return {"newt_app_image::wifi_memory::lend": entries["newt_app_image::wifi_memory::lend"]}, {}, run
    return entries, {run: {selector}}, run


def analyse(image: Image, chance=frozenset(), entries=ENTRIES, update_only=UPDATE_ONLY, run=RUN) -> dict:
    start, end = image.symbol("_newt_wifi_start"), image.symbol("_newt_wifi_end")
    gp = image.symbols.get("__global_pointer$")
    lent = image.sections.get(LENT)
    nodes = image.nodes
    failures = []
    # 1. The lent range and what it holds.
    if not lent or lent["address"] != start or lent["address"] + lent["size"] != end or end <= start:
        failures.append(f"{LENT} is not exactly [_newt_wifi_start, _newt_wifi_end)")
    if gp is not None and start <= gp + 2047 and gp - 2048 < end:
        failures.append("gp-relative code can reach the lent range")
    # Every input the map places there, merged ones too (which are no node).
    held = [entry for entry in image.inputs if entry["output"] == LENT and entry["size"]]
    for entry in held:
        kind = "." + entry["name"].split(".")[1] if entry["name"].startswith(".") else entry["name"]
        phy = entry["file"].startswith("libphy.a(") and kind in PHY_RODATA
        if not wifi_input(entry["file"], entry["name"]) or not (kind in LENT_INPUTS or phy):
            failures.append(f"{LENT} holds {entry['name']} of {entry['file']}, not on the reviewed Wi-Fi list")
    # 2. Every reference between nodes: code's (see references) and data words.
    arrays = [(image.symbol(low), image.symbol(high)) for low, high in STARTUP_ARRAYS]
    edges = [set() for _ in nodes]
    targets = {}  # (referrer, referent) -> the addresses referred to
    # Each node's references into the lent range, by address alone: padding and merged constants there have no node.
    lent_refs = {}
    roots = set()
    for index, node in enumerate(nodes):
        # `chance`: data words that only equal a function's start (tools/object_relocations.py).
        found = references(node, gp) if node["kind"] == "code" else [v for at, v in image.words(node) if at not in chance]
        inside = {address for address in found if start <= address < end}
        if inside:
            lent_refs[index] = inside
        for address in found:
            target = image.node_at(address)
            if target is None or target == index:
                continue
            # A function pointer in data is its start: inside a function, a
            # data word is a jump table's (owned by its function) or chance.
            if node["kind"] == "data" and nodes[target]["kind"] == "code" and address != nodes[target]["address"]:
                continue
            edges[index].add(target)
            targets.setdefault((index, target), set()).add(address)
        if node["kind"] == "data" and any(low <= node["address"] < high for low, high in arrays):
            roots.add(index)
    referrers = [set() for _ in nodes]
    for index, referents in enumerate(edges):
        for target in referents:
            referrers[target].add(index)
    names = {}
    for index, node in enumerate(nodes):
        if node["kind"] == "code":
            names.setdefault(node["name"], index)
    library = {names[name] for name, rule in entries.items() if rule.get("library") and name in names}
    # Wi-Fi: WIFI_FILES' nodes, and every node only Wi-Fi (or a library entry)
    # refers to: the largest such set, so a cycle (mbedtls' cipher tables and
    # their functions) that only they enter is Wi-Fi's too.
    own = {index for index, node in enumerate(nodes) if index not in roots and (
        wifi_file(node["file"]) or node["kind"] == "data" and wifi_input(node["file"], node["name"]))}
    referred = {index for index in range(len(nodes)) if referrers[index] and index not in roots}
    wifi = closed(edges, referrers, referred - library, own, library)
    # Update-mode code: runUpdateMode and what only it reaches, likewise.
    update = closed(edges, referrers, referred, {names[run]}) if run in names else set()
    # What may not be reached outside Wi-Fi: its code, its RAM, and constants
    # that point at either or into the lent range. Wi-Fi's other constants are
    # only flash bytes.
    live = {index for index in wifi if nodes[index]["kind"] == "code" or index in lent_refs
            or not FLASH[0] <= nodes[index]["address"] < FLASH[1]}
    changed = True
    while changed:
        changed = False
        for index in wifi - live:
            if edges[index] & live:
                live.add(index)
                changed = True
    for name in list(entries) + list(update_only):
        if name not in names:
            failures.append(f"missing {name}")
    startup = {}
    for index, node in enumerate(nodes):
        if index in wifi:
            continue
        reached = edges[index] & live
        inside = lent_refs.get(index, set())
        if not reached and not inside:
            continue
        rule = entries.get(node["name"]) if node["kind"] == "code" else None
        if index in roots and not inside:
            for root in reached:
                startup[nodes[root]["name"]] = outside_calls(nodes, edges, wifi, root)
            continue
        addresses = sorted(inside.union(*(targets[(index, target)] for target in reached)))
        if rule and rule["range"] == "start":
            if addresses != [start]:
                failures.append(f"{node['name']} may refer only to the lent range's start")
        elif not rule:
            what = sorted({nodes[target]["name"] for target in reached} |
                          {f"lent range +0x{address - start:x}" for address in inside if image.node_at(address) is None})
            failures.append(f"{node['name']} ({node['file']}) refers to Wi-Fi: {what[:4]}")
    for name, callers in {**{name: rule["callers"] for name, rule in entries.items() if rule["callers"]},
                          **update_only}.items():
        if name in names:
            seen = {nodes[referrer]["name"] for referrer in referrers[names[name]]}
            outside = (seen - callers if callers != UPDATE else
                       {nodes[referrer]["name"] for referrer in referrers[names[name]] - update})
            if not seen or outside:
                failures.append(f"{name} is reached from outside update mode: {sorted(outside or seen)}")
    for name, outside in startup.items():
        if name not in STARTUP:
            failures.append(f"unreviewed startup root into Wi-Fi code: {name} (calls {outside})")
        elif outside != sorted(STARTUP[name]):
            failures.append(f"startup root {name} now calls {outside} outside Wi-Fi code")
    # 3. IRAM: nothing calls flash-resident libphy, libesp_ringbuf or FLASH_FUNCTIONS code.
    moved = {index for index, node in enumerate(nodes) if node["kind"] == "code" and (
        node["file"].split("(", 1)[0] in FLASH_ARCHIVES
        or node["name"] in FLASH_FUNCTIONS and matches(node["file"], FLASH_FUNCTIONS[node["name"]]))}
    iram = [index for index, node in enumerate(nodes) if node["kind"] == "code" and node["section"] in IRAM]
    for index in moved & set(iram):
        failures.append(f"{nodes[index]['name']} of {nodes[index]['file']} is still in IRAM")
    for index in iram:
        called = edges[index] & moved
        if called:
            failures.append(f"IRAM {nodes[index]['name']} calls flash code: "
                            f"{sorted(nodes[target]['name'] for target in called)[:4]}")
    return {
        "failures": failures,
        "lent_range": {"start": hex(start), "end": hex(end), "bytes": end - start,
                       "files": sorted({entry["file"].split("(", 1)[0] for entry in held})},
        "wifi_code_functions": sum(1 for index in wifi if nodes[index]["kind"] == "code"),
        "startup_roots": startup,
        "iram_functions": len(iram),
        "iram_indirect_calls": sum(indirect_calls(nodes[index]) for index in iram),
        "flash_functions_moved": len(moved),
    }


def closed(edges, referrers, candidates, fixed, extra=frozenset()):
    """`fixed` and the largest subset of `candidates` that only it, itself and `extra` refer to."""
    kept = set(candidates) | fixed
    pending = list(kept - fixed)
    while pending:
        index = pending.pop()
        if index in kept and index not in fixed and not referrers[index] <= kept | extra:
            kept.discard(index)
            pending.extend(target for target in edges[index] if target in kept)
    return kept


def outside_calls(nodes, edges, wifi, root):
    """Every function outside Wi-Fi code that Wi-Fi code reached from `root` refers to."""
    reached, pending, outside = {root}, [root], set()
    while pending:
        for target in edges[pending.pop()]:
            if target in reached:
                continue
            if target in wifi:
                reached.add(target)
                pending.append(target)
            elif nodes[target]["kind"] == "code":
                outside.add(nodes[target]["name"])
    return sorted(outside)


def tool(name, *arguments):
    return subprocess.run([str(TOOLS / f"riscv32-esp-elf-{name}"), *arguments], check=True,
                          capture_output=True, text=True).stdout


def main() -> None:
    global TOOLS
    parser = argparse.ArgumentParser(description=__doc__)
    build = Path(".pio") / FIRMWARE_BUILD / "build/newt_x3"
    parser.add_argument("--elf", type=Path, default=build / "firmware.elf")
    parser.add_argument("--map", type=Path, default=build / "firmware.map")
    parser.add_argument("--output", type=Path, default=Path(".build") / FIRMWARE_BUILD / "wifi-link-verification.json")
    parser.add_argument("--toolchain-bin", type=Path, default=TOOLS,
                        help="already installed pinned C3 tools (no acquisition)")
    parser.add_argument("--rust-app", action="store_true", help="bind the separate Rust update boot and native SDK leaves")
    parser.add_argument("--reading-only", action="store_true", help="owner-approved Rust bench without Wi-Fi/OTA; require update entries absent")
    args = parser.parse_args()
    if args.reading_only and not args.rust_app:
        parser.error("--reading-only requires --rust-app")
    TOOLS = args.toolchain_bin
    elf, link_map = ROOT / args.elf, ROOT / args.map
    output = owned_file(ROOT, ROOT / args.output, create_parents=True)
    image = Image(elf.read_bytes(), tool("nm", str(elf)), tool("objdump", "-d", "-C", str(elf)), link_map.read_text())
    profile = rust_entries(args.reading_only) if args.rust_app else (ENTRIES, UPDATE_ONLY, RUN)
    result = analyse(image, chance_words(image, lambda path: tool("readelf", "-rWS", str(ROOT / path)), ROOT), *profile)
    if args.reading_only:
        full_entries, full_update, _ = rust_entries()
        forbidden = (set(full_entries) | set(full_update)) - set(profile[0])
        present = {node["name"] for node in image.nodes if node["kind"] == "code"} & forbidden
        result["failures"] += [f"reading-only image contains update entry: {name}" for name in sorted(present)]
        result["scope"] = "Owner-approved reading-only bench; update entries required absent"
    sources = firmware_sources(ROOT)
    if args.rust_app:
        # This is the board SDK object actually compiled by rust-app-build.py;
        # the compatibility C++ application's mount is not in the Rust image.
        path = "rust/salamander/board/native/board.cpp"
        sources = {path: (ROOT / path).read_text()}
    failures, result["sd_context"] = lent_heap(
        result["lent_range"]["bytes"], tool("readelf", "--debug-dump=info", "--dwarf-depth=2", str(elf)),
        sources)
    result["failures"] += failures
    if result["failures"]:
        raise RuntimeError(f"{STOPPED}:\n" + "\n".join(result["failures"]))
    receipt = {"elf_sha256": hashlib.sha256(image.elf).hexdigest(), **result, "result": "PASS",
               "limits": "Static references in the linked image only; indirect calls and ROM code are not "
                         "followed (iram_indirect_calls counts IRAM's). Device behavior is a separate gate."}
    write_text(ROOT, output, json.dumps(receipt, indent=2) + "\n")
    print(f"PASS: Wi-Fi lends {result['lent_range']['bytes']} B that only Wi-Fi code uses and that holds "
          f"the SD card's {result['sd_context']['block']} B FATFS context; "
          f"no IRAM call into flash-resident libphy, libesp_ringbuf or {', '.join(FLASH_FUNCTIONS)} code")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
