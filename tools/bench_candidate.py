"""Offline checks for a manifest-pinned bench app; no device access.

ESP image fields follow the pinned official esptool ESP32C3FirmwareImage format.
"""
import hashlib
import json
from pathlib import Path
import re
import struct

import x3_slots
from x3_slots import HEAD, SLOT_SIZE as CAPACITY, SLOTS

FLASH = 0x1000000
SECTOR = 4096
USABLE = CAPACITY - ((CAPACITY + 9) // 10 + SECTOR - 1) // SECTOR * SECTOR
# The one firmware build; it always embeds and answers its DEV build ID.
VARIANT = 'regional-dev-usb'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(data):
    def unique(pairs):
        require(len(dict(pairs)) == len(pairs), 'Duplicate JSON key')
        return dict(pairs)
    return json.loads(data, object_pairs_hook=unique)


def write_span(offset, length):
    require(type(offset) is int and offset in SLOTS, 'Writes must start at an app slot')
    require(type(length) is int and 0 < length <= USABLE, 'App exceeds usable slot')
    end = offset + (length + SECTOR - 1) // SECTOR * SECTOR
    require(end <= offset + USABLE, 'Erase sectors exceed the usable app slot')
    return end - offset


def validate_image(image, build):
    write_span(SLOTS[0], len(image))
    require(len(image) >= 24 and image[0] == 0xe9 and 0 < image[1] <= 16,
            'Invalid ESP app header')
    require(image[2:4] == b'\x02\x4f' and struct.unpack_from('<H', image, 12)[0] == 5,
            'Expected ESP32-C3 DIO/80MHz/16MB image')
    require(image[23] == 1, 'App must include its SHA-256 digest')
    at, checksum = 24, 0xef
    for _ in range(image[1]):
        require(at + 8 <= len(image), 'Truncated app segment header')
        length = struct.unpack_from('<I', image, at + 4)[0]
        at += 8
        require(length <= 0x1000000 and at + length <= len(image), 'Truncated app segment')
        for byte in image[at:at + length]:
            checksum ^= byte
        at += length
    checksum_at = (at // 16 + 1) * 16 - 1
    require(len(image) == checksum_at + 1 + 32, 'Unexpected app length or trailing bytes')
    require(image[checksum_at] == checksum, 'App checksum mismatch')
    require(hashlib.sha256(image[:-32]).digest() == image[-32:], 'App digest mismatch')
    require(build.encode() + b'\0' in image and (build + '-dirty').encode() not in image,
            'App build identity differs or is dirty')


def other_image(path, image_sha256):
    """install-other's pinned image of another firmware: one whole ESP32-C3
    app (header, 1 to 16 placed segments, checksum and appended SHA-256, as
    the bootloader and Newt's own switch check it), DIO/80MHz/16MB as the X3's
    own, inside the usable slot and without Newt's mark."""
    require(re.fullmatch('[0-9a-f]{64}', image_sha256) is not None, 'Invalid image SHA-256')
    image = Path(path).read_bytes()
    require(sha(image) == image_sha256, 'Image SHA-256 differs from its pin')
    write_span(SLOTS[0], len(image))
    require(image[2:4] == b'\x02\x4f' and image[23] == 1,
            'Expected a DIO/80MHz/16MB image with its SHA-256 digest')
    # Exact, unlike a chosen file (x3_slots.extra_data): install-other writes, and
    # restore-other later compares, the pinned file's own bytes.
    require(x3_slots.image_length(image) == len(image) and x3_slots.image_intact(image, CAPACITY),
            'Not one whole ESP32-C3 app image')
    require(x3_slots.classify(image[:HEAD]).kind == 'other',
            'A Newt image installs with install and its reviewed manifest')
    return image


def partition_layout(table):
    require(len(table) in (0xc00, SECTOR), 'Expected a generated partition table')
    entries = []
    for at in range(0, len(table), 32):
        magic, kind, subtype, offset, size, label, flags = struct.unpack_from('<HBBII16sI', table, at)
        if magic == 0xebeb:
            require(table[at + 16:at + 32] == hashlib.md5(table[:at]).digest(),
                    'Partition table checksum differs')
            require(table[at + 32:] == b'\xff' * (len(table) - at - 32), 'Partition table trailing data')
            break
        require(magic == 0x50aa and size > 0 and offset + size <= FLASH and flags == 0,
                'Invalid/unsupported partition table')
        entries.append((kind, subtype, offset, size, label.rstrip(b'\0')))
    else:
        raise ValueError('Missing partition table MD5')
    require(all(sum(e[:4] == (0, 0x10 + i, at, CAPACITY) and e[4] == b'app%d' % i for e in entries) == 1
                for i, at in enumerate(SLOTS)), 'Candidate must use the reviewed app slots')
    ota = [e for e in entries if e[:2] == (1, 0)]
    apps = sorted(e for e in entries if e[0] == 0)
    require(len(ota) == 1 and ota[0][2:4] == (0xe000, 2 * SECTOR) and
            [e[1] for e in apps] == [0x10, 0x11], 'Unsupported OTA layout')
    spans = sorted((e[2], e[2] + e[3]) for e in entries)
    require(all(a[1] <= b[0] for a, b in zip(spans, spans[1:])), 'Overlapping partitions')


class Candidate:
    def __init__(self, directory, manifest_sha256):
        directory = Path(directory)
        require(re.fullmatch('[0-9a-f]{64}', manifest_sha256) is not None, 'Invalid manifest SHA-256')
        raw = (directory / 'manifest.json').read_bytes()
        require(sha(raw) == manifest_sha256, 'Reviewed manifest identity differs')
        manifest = json_bytes(raw)
        files = manifest['files']
        require(isinstance(files, dict), 'Invalid candidate file list')
        contents = {}
        for name, digest in files.items():
            require(Path(name).name == name and name not in ('.', '..'), 'Unsafe candidate file name')
            contents[name] = (directory / name).read_bytes()
            require(sha(contents[name]) == digest, 'Candidate file hash differs: ' + name)
        revision = manifest['revision']
        require(re.fullmatch('[0-9a-f]{40}', revision) is not None, 'Invalid candidate revision')
        identities = [n for n in contents if n.startswith('firmware-identity-') and n.endswith('.json')]
        require(len(identities) == 1, 'Expected exactly one firmware identity receipt')
        identity = json_bytes(contents[identities[0]])
        require(identity['variant'] == VARIANT and identities[0] == f'firmware-identity-{VARIANT}.json' and
                identity['git_revision'] == revision and identity['dirty'] is False,
                'Candidate lacks a clean matching identity receipt')
        self.image, self.table = contents['firmware.bin'], contents['partitions.bin']
        require(identity['binary_sha256'] == sha(self.image), 'App differs from identity receipt')
        gate = json_bytes(contents['gate-receipt.json'])
        require(gate['revision'] == revision and type(gate['exit']) is int and gate['exit'] == 0 and
                gate['log_sha256'] == sha(contents['gate.log']), 'Missing/failing candidate gate receipt')
        report = json_bytes(contents[f'code-health-{VARIANT}.json'])
        require(report['flash_bytes'] == manifest['flash'][VARIANT], 'Resource report differs: ' + VARIANT)
        self.build = identity.get('build_id', revision[:12] + '-regional-experiment')
        require(self.build in (revision[:12], revision[:12] + '-regional-experiment'),
                'Candidate build identity differs from its revision')
        self.manifest_sha256 = manifest_sha256
        validate_image(self.image, self.build)
        partition_layout(self.table)
        self.table = self.table.ljust(SECTOR, b'\xff')
