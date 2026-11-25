"""Regenerate the host's uppercase tables from the FatFs object linked by Newt.

Run with --write to update; without it, check the committed tables. The CLI
uses the generated constants only and needs no SDK, archiver or ELF parser.
"""
import argparse
import hashlib
from pathlib import Path
import struct
import subprocess

from host_objects import compiler_environment
from wifi_link_image import elf_sections

ROOT = Path(__file__).resolve().parents[1]
LIBRARY = ROOT / '.toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3/lib/libfatfs.a'
ARCHIVER = ROOT / '.toolchain/platformio/packages/toolchain-riscv32-esp/bin/riscv32-esp-elf-ar'
OBJECT_SHA256 = '6adc8079feb698a259a62c8a4f2a9f2b2bd9b52f65fe10f591f6cd8da21f1031'
BEGIN, END = '# BEGIN GENERATED TABLES\n', '# END GENERATED TABLES\n'


def pinned_tables():
    raw = subprocess.run([str(ARCHIVER), 'p', str(LIBRARY), 'ffunicode.c.obj'],
                         check=True, capture_output=True, env=compiler_environment(ROOT)).stdout
    # A provider change also needs a review of ff_wtoupper's decoding algorithm.
    if hashlib.sha256(raw).hexdigest() != OBJECT_SHA256:
        raise ValueError('FatFs Unicode provider changed; review its mapping before regenerating')
    sections = elf_sections(raw)
    tables = []
    for name in ('cvt1', 'cvt2'):
        matches = [s for label, s in sections.items() if label.startswith('.rodata.' + name + '.')]
        if len(matches) != 1:
            raise ValueError('Pinned FatFs uppercase table missing')
        section = matches[0]
        data = raw[section['offset']:section['offset'] + section['size']]
        tables.append(struct.unpack('<' + str(len(data) // 2) + 'H', data))
    return tuple(tables)


def generated(tables):
    text = BEGIN + '# Source: ESP-IDF v5.5 components/fatfs/src/ffunicode.c (ff_wtoupper).\n'
    text += '# Object: esp32c3/lib/libfatfs.a(ffunicode.c.obj)\n'
    text += '# Object SHA-256: ' + OBJECT_SHA256 + '\n'
    for name, table in zip(('CVT1', 'CVT2'), tables, strict=True):
        text += name + ' = (\n'
        for index in range(0, len(table), 8):
            text += '    ' + ', '.join(f'0x{value:04x}' for value in table[index:index + 8]) + ',\n'
        text += ')\n'
    return text + END


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    path = ROOT / 'tools/fatfs_case.py'
    source = path.read_text()
    before, rest = source.split(BEGIN)
    _, after = rest.split(END)
    expected = before + generated(pinned_tables()) + after
    if args.write:
        path.write_text(expected)
    elif source != expected:
        raise ValueError('Host FatFs tables differ from pinned SDK; regenerate with --write')
    print('PASS: host FatFs tables match the pinned firmware object')


if __name__ == '__main__':
    main()
