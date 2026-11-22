"""Selected Rust open-frame ceiling within the existing linked-image guard."""
import re

from linked_code import native_instructions

OPEN_PROFILE = ("newt_app_image::reader_image::open::<impl "
                "newt_app_image::reader_image::Runtime>::open_selected_profile")
OPEN_FRAME_CEILING = 672
BRANCHES = {"beq", "bne", "blt", "bge", "bltu", "bgeu", "beqz", "bnez",
            "blez", "bgez", "bltz", "bgtz", "bgt", "ble", "bgtu", "bleu"}


def frame_bytes(name, body):
    """Conservative local reservation, shared by the linked-image measurements."""
    reserved = 0
    for opcode, operands, _ in native_instructions(body):
        if opcode in {'sw', 'sh', 'sb', 'sd', 'fsw', 'fsd'} or opcode in BRANCHES:
            continue
        if operands.split(',', 1)[0] not in {'sp', 'x2'}:
            continue
        adjust = re.fullmatch(r'sp,sp,(-?\d+)', operands) if opcode == 'addi' else None
        if adjust is None:
            raise RuntimeError(f'Unresolved selected Rust open SP adjustment: {name}')
        reserved += max(0, -int(adjust[1]))
    return reserved


def validate_open_frames(functions):
    """Include compiler-split profile closures; a small wrapper cannot hide them.

    This conservative local-frame ceiling is not a whole-chain stack proof.
    Unknown SP writes fail closed rather than acquiring an assumed size.
    """
    if OPEN_PROFILE not in functions:
        raise RuntimeError("Missing selected Rust open profile frame")
    frames = {}
    for name, body in functions.items():
        if name != OPEN_PROFILE and not name.startswith(OPEN_PROFILE + "::"):
            continue
        frames[name] = frame_bytes(name, body)
    total = sum(frames.values())
    if total > OPEN_FRAME_CEILING:
        raise RuntimeError(f"Selected Rust open frames reserve {total} bytes, over {OPEN_FRAME_CEILING}")
    return {"frames": frames, "reserved_bytes": total, "ceiling_bytes": OPEN_FRAME_CEILING,
            "limits": "Local profile and compiler-closure frames only; native-inclusive chains and device watermark remain separate."}
