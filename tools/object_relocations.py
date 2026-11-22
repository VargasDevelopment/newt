"""Tell a function pointer in linked data from bytes that only equal one (tools/verify-wifi-link.py).

A data word is a link-time address only where its object file has a
relocation; elsewhere it is bytes the compiler wrote, such as a font's pixels,
however much they look like an address. Plain objects are read, as are archive
members built under the project's .build output root; all other archives
(including the SDK in .toolchain) remain conservative. Missing or ambiguous
member evidence stops the check.
"""
import re
from pathlib import Path


def relocated_offsets(readelf: str) -> dict:
    """Each section's relocated offsets, from `readelf -rW` of one object."""
    sections, current = {}, None
    for line in readelf.splitlines():
        header = re.match(r"^Relocation section '\.rela?(\.[^']+)'", line)
        if header:
            current = sections.setdefault(header[1], set())
        elif current is not None and (entry := re.match(r"^([0-9a-f]{8})\s", line)):
            current.add(int(entry[1], 16))
    return sections


def member_text(text: str, member: str) -> str:
    """Select exactly one actual readelf archive member, never a fresh fallback."""
    blocks = []
    for match in re.finditer(r"^File: (.+)\(([^()]*)\)\s*$", text, re.M):
        blocks.append((match.start(), match.end(), match[2]))
    found = [text[end:blocks[index + 1][0] if index + 1 < len(blocks) else len(text)]
             for index, (_, end, name) in enumerate(blocks) if name == member]
    if len(found) != 1:
        raise RuntimeError(f"Expected one relocation member {member}, found {len(found)}")
    return found[0]


def object_metadata(path, read, cache, project_root):
    if path not in cache:
        if path.endswith(")") and "(" in path:
            archive, member = path[:-1].rsplit("(", 1)
            root = Path(project_root).resolve()
            if not (root / archive).resolve().is_relative_to(root / ".build"):
                return None
            key = ("archive", archive)
            if key not in cache:
                cache[key] = read(archive)
            text = member_text(cache[key], member)
        elif "(" in path:
            return None
        else:
            text = read(path)
        flags = {match[1]: match[2] for match in re.finditer(
            r"^\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+[0-9a-f]+\s+[0-9a-f]+\s+[0-9a-f]+\s+[0-9a-f]+\s+([A-Z]+)\s", text, re.M)}
        cache[path] = {"relocations": relocated_offsets(text), "flags": flags}
    return cache[path]


def chance_words(image, read, project_root=Path.cwd()) -> set:
    """The addresses of data words in Newt's own objects that equal a function's
    start where their object has no relocation. `read(path)` is readelf -rW's output."""
    starts = {node["address"] for node in image.nodes if node["kind"] == "code"}
    paths = {(entry["name"], entry["address"]): entry["path"] for entry in image.inputs}
    objects, chance = {}, set()
    for node in image.nodes:
        path = paths.get((node["name"], node["address"]), "(")
        if node["kind"] != "data":
            continue
        for at, value in image.words(node):
            if value not in starts:
                continue
            metadata = object_metadata(path, read, objects, project_root)
            if metadata is not None and at - node["address"] not in metadata["relocations"].get(node["name"], set()):
                chance.add(at)
    return chance


def literal_string_ranges(image, read, project_root):
    """Compiler-typed, read-only merged strings with no pointer relocations.

    Clip pre-merge map sizes at the next input's start: a stale string size
    must never cover the following pointer table. Named objects are separately
    excluded by the caller, so their addresses still undergo the original check.
    """
    cache, ranges = {}, []
    starts = sorted({entry["address"] for entry in image.inputs if entry["size"]})
    from bisect import bisect_right
    for entry in image.inputs:
        if not entry["name"].startswith(".rodata.str") or not entry["size"]:
            continue
        metadata = object_metadata(entry["path"], read, cache, project_root)
        if metadata is None:
            continue
        flags = set(metadata["flags"].get(entry["name"], ""))
        if not {"A", "M", "S"} <= flags or flags & {"W", "X"} or metadata["relocations"].get(entry["name"]):
            continue
        at = entry["address"]
        index = bisect_right(starts, at)
        end = min(at + entry["size"], starts[index] if index < len(starts) else at + entry["size"])
        ranges.append((at, end))
    return ranges
