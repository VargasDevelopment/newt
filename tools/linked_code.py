"""Read a linked image's disassembly: functions, calls and references.
Shared by the image verifiers (verify-nvs-guard.py, crash_output_guard.py)."""
import re


def parse_functions(disassembly: str) -> dict[str, list[str]]:
    """Keep instructions grouped by actual linked function, including tail jumps."""
    functions: dict[str, list[str]] = {}
    caller = None
    for line in disassembly.splitlines():
        label = re.match(r"^[0-9a-fA-F]+ <(.+)>:$", line)
        if label:
            caller = label[1]
            functions.setdefault(caller, [])
        elif caller is not None:
            functions[caller].append(line)
    return functions


def ordered_calls(instructions: list[str], preserve_offsets: bool = False) -> list[str]:
    targets = []
    for line in instructions:
        # RISC-V long calls/tail calls are auipc plus jalr/jr with
        # objdump's resolved # target. An unresolved indirect jump is omitted.
        target = re.search(r"\b(?:jalr|jal|jr|j)\s+[^<\n]*<(.+)>", line)
        if target:
            targets.append(target[1] if preserve_offsets else target[1].split("+0x", 1)[0])
    return targets


def native_instructions(lines):
    for line in lines:
        match = re.match(r"^\s*[0-9a-fA-F]+:\s+[0-9a-fA-F]+\s+(\S+)\s*(.*)", line)
        if match:
            opcode, operands = match.groups()
            yield opcode, operands.split("#", 1)[0].strip(), line


def references(lines) -> set[str]:
    """Every function an instruction names: calls, tail jumps (jr) and address
    loads, which ordered_calls does not all see."""
    return {name.split("+0x", 1)[0] for line in lines for name in re.findall(r"<([^>]+)>", line)}
