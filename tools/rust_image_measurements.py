"""The Rust open-profile frame, read from the linked ELF."""
import subprocess
from pathlib import Path

from linked_code import parse_functions
from rust_open_stack import OPEN_PROFILE, validate_open_frames

OPEN_KEY = 'rust/firmware/reader_image_open.rs::' + OPEN_PROFILE


def functions(root, elf):
    tool = Path(root) / '.toolchain/platformio/packages/toolchain-riscv32-esp/bin/riscv32-esp-elf-objdump'
    return parse_functions(subprocess.run([str(tool), '-d', '-C', str(elf)], check=True,
                                         capture_output=True, text=True).stdout)


def open_frame(root, elf):
    measured = validate_open_frames(functions(root, elf))
    return {OPEN_KEY: {'bytes': measured['reserved_bytes'], 'kind': 'static'}}
