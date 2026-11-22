"""Read one linked C3 image for tools/verify-wifi-link.py: its sections,
symbols, functions, map inputs and the addresses each function's code refers
to (that verifier's Limits say what counts as a reference)."""
import bisect
import re
import struct

STOPPED = "Wi-Fi link check stopped"
IRAM = (".iram0.text", ".iram0.vectors")
CODE = IRAM + (".flash.text", ".rtc.text")
# Merged strings and constants (".str1.4", ".cst8"): pointer-free, placed by the linker after the map's sizes: no node.
MERGED = re.compile(r"\.(?:str|cst)\d|^\.eh_frame")
# Instructions whose first operand is not a register they write.
NO_DESTINATION = re.compile(r"^(?:s[bhwd]|fs[wd]|b\w*|j|jr|ret|fence(?:\.i)?|ecall|ebreak|nop|mret|wfi|unimp|csr[wsc]i?)$")
CALLER_SAVED = ["ra"] + [f"a{i}" for i in range(8)] + [f"t{i}" for i in range(7)]


class Image:
    """One linked image: sections, symbols, and one node per function or data
    object (a map input section outside code). Merged string and constant
    sections hold no pointers, and the map cannot place them, so they are not
    nodes."""

    def __init__(self, elf: bytes, symbols: str, disassembly: str, link_map: str):
        self.elf = elf
        self.sections = elf_sections(elf)
        self.symbols = parse_symbols(symbols)
        functions = parse_functions(disassembly)
        self.inputs = inputs = parse_map(link_map)
        code = {name for name in CODE if name in self.sections}
        loaded = {name for name, section in self.sections.items()
                  if section["flags"] & 2 and name not in code}
        owner = Lookup([(entry["address"], entry["address"] + entry["size"], entry) for entry in inputs
                        if entry["size"] and entry["output"] in code])
        self.nodes = [{"kind": "code", "name": f["name"], "address": f["address"], "end": f["end"],
                       "code": f["code"], "file": (owner.find(f["address"]) or {}).get("file", "?"),
                       "section": self.section_of(f["address"])} for f in functions]
        self.nodes += [{"kind": "data", "name": entry["name"], "address": entry["address"],
                        "end": entry["address"] + entry["size"], "file": entry["file"], "section": entry["output"]}
                       for entry in inputs if entry["size"] and entry["output"] in loaded
                       and not MERGED.search(entry["name"])]
        self.nodes.sort(key=lambda node: node["address"])
        self.lookup = Lookup([(node["address"], node["end"], index) for index, node in enumerate(self.nodes)])

    def symbol(self, name):
        if name not in self.symbols:
            raise RuntimeError(f"{STOPPED}: the image has no {name}")
        return self.symbols[name]

    def node_at(self, address):
        return self.lookup.find(address)

    def section_of(self, address):
        for name, section in self.sections.items():
            if section["flags"] & 2 and section["address"] <= address < section["address"] + section["size"]:
                return name
        return None

    def words(self, node):
        """Each aligned 32-bit word a loaded data object holds, with its address."""
        section = self.sections[node["section"]]
        if section["type"] != 1:  # NOBITS: zeros
            return
        first = (node["address"] + 3) & ~3
        for address in range(first, node["end"] - 3, 4):
            yield address, struct.unpack_from("<I", self.elf, section["offset"] + address - section["address"])[0]


class Lookup:
    """Non-overlapping [start, end) ranges, each with a value."""

    def __init__(self, ranges):
        self.ranges = sorted((r for r in ranges if r[1] > r[0]), key=lambda r: r[0])
        self.starts = [start for start, _, _ in self.ranges]

    def find(self, address):
        index = bisect.bisect_right(self.starts, address) - 1
        if index >= 0 and address < self.ranges[index][1]:
            return self.ranges[index][2]
        return None


def elf_sections(data: bytes) -> dict:
    if data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
        raise RuntimeError(f"{STOPPED}: not a little-endian ELF32 image")
    offset = struct.unpack_from("<I", data, 0x20)[0]
    size, count, names = struct.unpack_from("<HHH", data, 0x2e)
    headers = [struct.unpack_from("<10I", data, offset + index * size) for index in range(count)]
    table = headers[names][4]
    result = {}
    for name, kind, flags, address, start, length, *_ in headers:
        label = data[table + name:data.index(b"\0", table + name)].decode()
        result[label] = {"type": kind, "flags": flags, "address": address, "offset": start, "size": length}
    return result


def parse_symbols(text: str) -> dict:
    """nm output: the address of each name (the first, if repeated)."""
    symbols = {}
    for line in text.splitlines():
        match = re.match(r"^([0-9a-fA-F]+)\s+\S\s+(.+)$", line)
        if match:
            symbols.setdefault(match[2], int(match[1], 16))
    return symbols


def parse_functions(text: str) -> list:
    """objdump -d -C: each label with its instructions, in address order."""
    functions, current = [], None
    for line in text.splitlines():
        label = re.match(r"^([0-9a-fA-F]+) <(.+)>:$", line)
        if label:
            current = {"address": int(label[1], 16), "name": label[2], "code": []}
            functions.append(current)
            continue
        instruction = re.match(r"^\s*([0-9a-fA-F]+):\s+[0-9a-fA-F]+\s+(\S+)\s*([^#]*?)\s*(?:#\s*([0-9a-fA-F]+).*)?$", line)
        if current is not None and instruction:
            current["code"].append((int(instruction[1], 16), instruction[2], instruction[3],
                                    int(instruction[4], 16) if instruction[4] else None))
    functions.sort(key=lambda function: function["address"])
    for index, function in enumerate(functions):
        following = functions[index + 1]["address"] if index + 1 < len(functions) else None
        last = function["code"][-1][0] + 4 if function["code"] else function["address"]
        function["end"] = min(following, last) if following is not None and following > function["address"] else last
    return functions


def parse_map(text: str) -> list:
    """The map's input sections after "Linker script and memory map": output
    section, input section, address, size and file (archive(member) or path)."""
    if "Linker script and memory map" not in text:
        raise RuntimeError(f"{STOPPED}: the map has no memory map")
    lines = text.split("Linker script and memory map", 1)[1].splitlines()
    entries, output, pending = [], None, None
    for line in lines:
        header = re.match(r"^(\.\S+)(?:\s+0x[0-9a-f]+\s+0x[0-9a-f]+)?\s*$", line)
        if header:
            output, pending = header[1], None
            continue
        whole = re.match(r"^ ([^\s*]\S*)\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line)
        name_only = re.match(r"^ ([^\s*]\S*)$", line)
        rest = re.match(r"^\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line)
        if whole:
            entry = (whole[1], whole[2], whole[3], whole[4])
        elif rest and pending:
            entry = (pending, rest[1], rest[2], rest[3])
        else:
            pending = name_only[1] if name_only else None
            continue
        pending = None
        entries.append({"output": output, "name": entry[0], "address": int(entry[1], 16), "size": int(entry[2], 16),
                        "file": file_name(entry[3].strip()), "path": entry[3].strip()})
    return sorted(entries, key=lambda entry: entry["address"])


def file_name(path: str) -> str:
    """archive(member) for archive members, else the object's file name."""
    match = re.match(r"^.*?([^/]+\.a)\(([^)]+)\)$", path)
    return f"{match[1]}({match[2]})" if match else path.rsplit("/", 1)[-1]


def references(function, gp):
    """Every address one function's code refers to (see Limits)."""
    found, known = set(), {}
    for address, mnemonic, operands, resolved in function["code"]:
        parts = [part.strip() for part in operands.split(",")] if operands else []
        if resolved is not None:
            found.add(resolved)
        target = re.search(r"\b([0-9a-f]+) <", operands)
        if target:
            found.add(int(target[1], 16))
        if mnemonic in ("lui", "auipc") and len(parts) == 2:
            value = int(parts[1], 0) << 12
            known[parts[0]] = (value + (address if mnemonic == "auipc" else 0)) & 0xffffffff
            continue
        for offset, base in re.findall(r"(-?\d+)\((\w+)\)", operands):
            if base in known:
                found.add((known[base] + int(offset)) & 0xffffffff)
            elif base == "gp" and gp is not None:
                found.add((gp + int(offset)) & 0xffffffff)
        if mnemonic == "addi" and len(parts) == 3 and (parts[1] in known or parts[1] == "gp"):
            value = (known[parts[1]] if parts[1] in known else gp) + int(parts[2], 0)
            found.add(value & 0xffffffff)
            if parts[1] != "gp":
                known[parts[0]] = value & 0xffffffff
                continue
        # objdump spells `addi rd,rs,0` as `mv`: a lui alone names a 4 KiB-aligned address.
        if mnemonic == "mv" and len(parts) == 2 and parts[1] in known:
            found.add(known[parts[1]])
            known[parts[0]] = known[parts[1]]
        elif mnemonic in ("jal", "jalr"):
            for register in CALLER_SAVED:
                known.pop(register, None)
        elif parts and not NO_DESTINATION.match(mnemonic):
            known.pop(parts[0], None)
    return found


def indirect_calls(function) -> int:
    return sum(1 for _, mnemonic, operands, resolved in function["code"]
               if mnemonic in ("jalr", "jr") and "<" not in operands and resolved is None
               and operands.strip() not in ("ra", "zero,0(ra)", "zero,ra,0"))
