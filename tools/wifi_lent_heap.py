"""Property 4 of tools/verify-wifi-link.py (docs/084-wifi-memory.md): the lent
range holds the SD card's FATFS context.

The first SD mount allocates the context after the lend and keeps it for the
whole boot. The RAM heap has no room for it, so it takes the lent heap if it
fits and otherwise splits the large heap's free block that the reader opens
books in. Its size comes from the linked ELF's DWARF and from max_files in the
firmware's only SD.begin call, so a bigger context fails the build instead.
"""
import re

# The SD card's FATFS context (IDF fatfs/vfs/vfs_fat.c): one vfs_fat_ctx_t
# followed by max_files FILs, from the firmware's only SD.begin call.
SD_SOURCES = ("firmware", "src", "include")
SD_MOUNT = re.compile(r"\bSD\.begin\(([^;]*?)\)\s*(?:;|\?)")
SD_CONTEXT_UNIT = "fatfs/vfs/vfs_fat.c"
# The lent heap's own TLSF control, heap info and block header: the context's
# block starts 740 B into the 22,528 B lent heap on the bench, plus a margin.
# Each doubling of a heap adds 68 B of control (876 B in the 116,496 B heap),
# far less than the range itself grows.
LENT_HEAP_OVERHEAD = 1024


def firmware_sources(root) -> dict:
    """Every C/C++ source the firmware build may compile, by relative path."""
    return {str(path.relative_to(root)): path.read_text() for folder in SD_SOURCES if (root / folder).is_dir()
            for path in sorted((root / folder).rglob("*")) if path.suffix in (".c", ".cpp", ".h", ".hpp")}


def type_sizes(dwarf: str, unit: str, names) -> dict:
    """readelf --debug-dump=info: the byte size behind each named typedef in
    the compile unit whose name ends with `unit`."""
    attribute = re.compile(r"^\s*<[0-9a-f]+>\s+(DW_AT_\w+)\s*:\s*(?:\([^)]*\):\s*)?(.*?)\s*$")
    for chunk in dwarf.split("Compilation Unit @")[1:]:
        top = re.search(r"DW_AT_name\s*:\s*(?:\([^)]*\):\s*)?(\S+)", chunk)
        if not top or not top[1].endswith(unit):
            continue
        dies, current = {}, None
        for line in chunk.splitlines():
            die = re.match(r"^\s*<\d+><([0-9a-f]+)>: Abbrev Number: \d+ \((\w+)\)", line)
            if die:
                current = dies[int(die[1], 16)] = {"tag": die[2]}
            elif current is not None and (found := attribute.match(line)):
                current.setdefault(found[1], found[2])
        sizes = {}
        for die in dies.values():
            name = die.get("DW_AT_name")
            if die["tag"] != "DW_TAG_typedef" or name not in names:
                continue
            target = die
            while target is not None and "DW_AT_byte_size" not in target:
                reference = re.fullmatch(r"<0x([0-9a-f]+)>", target.get("DW_AT_type", ""))
                target = dies.get(int(reference[1], 16)) if reference else None
            if target is not None:
                sizes[name] = int(target["DW_AT_byte_size"], 0)
        return sizes
    return {}


def tlsf_block(size: int, heap_bytes: int) -> int:
    """The block IDF 5.5's TLSF gives a request in a heap of heap_bytes: the
    request rounds up to its second-level class, and tlsf_create (as linked)
    makes 8, 16 or 32 classes for heaps up to 16 KiB, up to 256 KiB, or more."""
    classes_log2 = 3 if heap_bytes <= 16384 else 4 if heap_bytes <= 262144 else 5
    step = 1 << max(size.bit_length() - 1 - classes_log2, 0)
    return -(-size // step) * step


def lent_heap(lent_bytes: int, dwarf: str, sources: dict) -> tuple:
    """Failures, and the receipt's sd_context, for the lent heap holding the
    SD card's FATFS context."""
    sizes = type_sizes(dwarf, SD_CONTEXT_UNIT, ("vfs_fat_ctx_t", "FIL"))
    calls = [(name, call) for name, text in sorted(sources.items()) for call in SD_MOUNT.findall(text)]
    failures = [f"{SD_CONTEXT_UNIT} has no typedef {name} in the DWARF"
                for name in ("vfs_fat_ctx_t", "FIL") if name not in sizes]
    arguments = [part.strip() for part in calls[0][1].split(",")] if len(calls) == 1 else []
    if len(calls) != 1:
        failures.append(f"expected one SD.begin call in {'/'.join(SD_SOURCES)}, found {len(calls)}")
    elif len(arguments) < 5 or not arguments[4].isdigit():
        failures.append(f"SD.begin in {calls[0][0]} does not state max_files as a literal fifth argument")
    if failures:
        return failures, None
    files = int(arguments[4])
    request = sizes["vfs_fat_ctx_t"] + files * sizes["FIL"]
    block = tlsf_block(request, lent_bytes)
    needed = block + LENT_HEAP_OVERHEAD
    if lent_bytes < needed:
        failures.append(f"the lent range ({lent_bytes} B) cannot hold the SD card's FATFS context: "
                        f"{needed} B needed; raise LENT_FLOOR in tools/wifi-sections.py")
    return failures, {"vfs_fat_ctx_t": sizes["vfs_fat_ctx_t"], "FIL": sizes["FIL"], "max_files": files,
                      "request": request, "block": block, "needed": needed, "lent": lent_bytes}
