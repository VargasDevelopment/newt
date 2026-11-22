#!/usr/bin/env python3
"""List namespace names and physical live-entry counts from a plain NVS partition.

Offline only; accepts a partition image, never a device or full-flash dump.
ESP-IDF NVS pages are 4096 bytes: a 32-byte header, 32-byte two-bit entry
bitmap, and 126 32-byte entries. CRC32 uses the NVS seed (0xffffffff).
Counts include live entry headers (blob chunks/index separately), not payload
slots or erased entries. During page GC, copied headers may be counted twice;
this is a namespace inventory, not a logical key/value dump or NVS repair tool.
No ordinary key or value is decoded or emitted. Unsupported/encrypted or corrupt
page/entry metadata fails closed, with no partial namespace output. Variable
payload contents are not decoded. Spans must match type and declared size,
including erased entries; ambiguous metadata fails before any output. Physical
totals include payload slots, and empty_pages counts fully erased GC candidates.
"""
import argparse
from collections import Counter
from pathlib import Path
import struct
import sys
import zlib

PAGE = 4096
ENTRY = 32
ENTRIES = 126
PAGE_STATES = (0xfffffffe, 0xfffffffc, 0xfffffff8)  # active, full, freeing
TYPES = (1, 0x11, 2, 0x12, 4, 0x14, 8, 0x18, 0x21, 0x41, 0x42, 0x48)


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def inspect_partition(data):
    require(data and len(data) % PAGE == 0, 'Expected whole NVS partition pages')
    names, counts, runs = {}, Counter(), []
    totals = dict(pages=len(data) // PAGE, used=0, erased=0, free=0, empty_pages=0)
    for offset in range(0, len(data), PAGE):
        page = data[offset:offset + PAGE]
        if page == b'\xff' * PAGE:
            totals['free'] += ENTRIES
            totals['empty_pages'] += 1
            runs.append(ENTRIES)
            continue
        page_state, = struct.unpack_from('<I', page)
        require(page_state in PAGE_STATES and page[8] in (0xff, 0xfe), 'Unsupported NVS page state/version')
        require(zlib.crc32(page[4:28], 0xffffffff) == struct.unpack_from('<I', page, 28)[0],
                'NVS page CRC mismatch (corrupt or encrypted image)')
        bitmap = int.from_bytes(page[32:64], 'little')
        states = [(bitmap >> (2 * i)) & 3 for i in range(ENTRIES)]
        require(1 not in states, 'Invalid NVS entry bitmap')
        for label, state in (('used', 2), ('erased', 0), ('free', 3)):
            totals[label] += states.count(state)
        longest = current = 0
        for state in states:
            current = current + 1 if state == 3 else 0
            longest = max(longest, current)
        runs.append(longest if page_state == 0xfffffffe else 0)
        index = 0
        while index < ENTRIES:
            if states[index] == 3:
                require(page[64 + index * ENTRY:64 + (index + 1) * ENTRY] == b'\xff' * ENTRY,
                        'Uncommitted entry bytes make payload ownership ambiguous')
                index += 1
                continue
            entry = page[64 + index * ENTRY:64 + (index + 1) * ENTRY]
            ns, kind, span, _ = entry[:4]
            require(kind in TYPES and 1 <= span <= ENTRIES - index, 'Invalid NVS entry type/span')
            require(all(s == states[index] for s in states[index:index + span]), 'Incomplete NVS entry span')
            require(zlib.crc32(entry[:4] + entry[8:], 0xffffffff) == struct.unpack_from('<I', entry, 4)[0],
                    'NVS entry CRC mismatch (corrupt or encrypted image)')
            # Only size fields are interpreted; ordinary keys/values never reach output.
            # Blob indexes (0x48) are fixed headers; their size describes other chunks.
            expected = 1 + (struct.unpack_from('<H', entry, 24)[0] + 31) // 32 if kind in (0x21, 0x41, 0x42) else 1
            require(span == expected, 'NVS span differs from type/payload size')
            if states[index] == 0:
                index += span  # Erased payload is still payload, never a namespace header.
                continue
            if ns == 0:
                require(kind == 1 and span == 1 and 1 <= entry[24] <= 254, 'Invalid namespace declaration')
                raw = entry[8:24].split(b'\0', 1)[0]
                # Safe terminal output, including against control/escape bytes.
                require(0 < len(raw) <= 15 and b'\0' in entry[8:24] and
                        all(0x21 <= byte <= 0x7e for byte in raw), 'Invalid namespace name')
                name, identifier = raw.decode('ascii'), entry[24]
                require(identifier not in names or names[identifier] == name, 'Conflicting namespace identifiers')
                require(name not in names.values() or names.get(identifier) == name, 'Conflicting namespace names')
                names[identifier] = name
            else:
                require(ns != 255, 'Invalid namespace identifier')
                counts[ns] += 1
            index += span
    require(all(identifier in names for identifier in counts), 'Entry has no live namespace declaration')
    if totals['empty_pages']:
        runs.remove(ENTRIES)  # Keep one fully erased page reserved for GC.
    totals['append_run'] = max(runs, default=0)
    return sorted((name, counts[identifier]) for identifier, name in names.items()), totals


def inventory(data):
    return inspect_partition(data)[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path, help='Plain NVS partition image (not the full backup)')
    args = parser.parse_args()
    # Check before reading: device nodes, FIFOs and block devices are forbidden.
    require(args.image.is_file(), 'Expected a regular partition image file')
    result, totals = inspect_partition(args.image.read_bytes())
    for name, count in result:
        print(f'{name}\t{count}')
    print('TOTAL\t' + '\t'.join(f'{name}={value}' for name, value in totals.items()))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError):
        # Never print exception text containing image bytes, paths or values.
        print('FAIL: invalid, unsupported or unreadable NVS partition image', file=sys.stderr)
        sys.exit(1)
