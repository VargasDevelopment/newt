"""The one X3 app-slot identity rule: which slot a tool may write.

Slot identity is read from the device, never assumed: the two OTA selection
records and each slot's first HEAD bytes (image header, ESP-IDF app
descriptor and Newt's fixed mark). Every tool write and boot switch goes
through `decide`. When exactly one slot holds another firmware (any valid
app without Newt's mark: stock, CrossPoint, CrossInk or another), Newt's
slot is the other one and the other firmware is never written. Without one
(Newt+Newt, Newt+Empty after an SD install), the slot that boots must hold a
selected, confirmed Newt and tools write only the other slot (docs/082
section 1). rust/core/src/slot_installer applies the same rule on the device
and installer/src/slots.ts on the install page; tests/x3_slots_test.py's
vectors hold all three (tests/installer_test.py, tests/slot_vectors_test.py).

Selection follows the ESP-IDF bootloader: a record counts when its sequence
is set, its CRC matches and its state is not invalid or aborted; the highest
sequence boots slot (seq - 1) % 2, unless it is still pending verification,
which the rollback bootloader aborts in favor of the other record.
"""
from __future__ import annotations
from dataclasses import dataclass
import hashlib
import struct
import zlib

SECTOR = 4096
OTADATA, OTADATA_SIZE = 0xe000, 2 * SECTOR
SLOTS = (0x10000, 0x780000)  # app0 (ota_0) and app1 (ota_1), same on both units
SLOT_SIZE = 0x770000
HEAD = 0x200  # image header, first segment header, app descriptor, Newt mark

APP_MAGIC = 0xe9
DESC_AT, DESC_MAGIC = 0x20, 0xabcd5432
# Newt's .rodata_custom_desc follows the 256-byte descriptor at a fixed
# offset (include/newt/app_slot_identity.h).
NEWT_MARK_AT = DESC_AT + 0x100
NEWT_MARK = b'NEWT-X3-APP\0' + struct.pack('<I', 1)
# Newt's identity block follows the mark: version char[24], build char[16],
# each NUL-padded (NewtAppIdentity). Reported only; no decision trusts it.
NEWT_IDENTITY_AT, NEWT_VERSION_BYTES, NEWT_BUILD_BYTES = NEWT_MARK_AT + len(NEWT_MARK), 24, 16
ESP32C3_CHIP_ID = 5
# What people call another firmware, by its descriptor's project
# (firmwareName, include/newt/app_slot_identity.h). Stock X3 firmware
# reports arduino-lib-builder and stays unknown. Evidence: docs/077.
FIRMWARE_NAMES = {'CrossInk': 'CrossInk', 'crosspoint-reader': 'CrossPoint'}
UNKNOWN_FIRMWARE = 'the other firmware'

STATE_NEW, STATE_PENDING, STATE_VALID, STATE_INVALID, STATE_ABORTED = 0, 1, 2, 3, 4
STATE_UNDEFINED = 0xffffffff
STATE_NAMES = {STATE_NEW: 'new', STATE_PENDING: 'pending-verify', STATE_VALID: 'valid',
               STATE_INVALID: 'invalid', STATE_ABORTED: 'aborted', STATE_UNDEFINED: 'undefined'}


class Refused(ValueError):
    """The device bytes do not establish a safe slot identity; nothing is written."""


class SequenceExhausted(Refused):
    """No OTA sequence below 0xffffffff selects the slot (#241); nothing is written."""


def refuse(condition, message):
    if not condition:
        raise Refused(message)


def seq_crc(seq):
    return zlib.crc32(struct.pack('<I', seq), 0xffffffff)


@dataclass(frozen=True)
class Selection:
    seq: int
    state: int
    usable: bool


def selections(ota):
    refuse(len(ota) == OTADATA_SIZE, 'Incomplete OTA selection read')
    result = []
    for at in (0, SECTOR):
        seq, state, crc = (struct.unpack_from('<I', ota, at)[0], *struct.unpack_from('<II', ota, at + 24))
        usable = (seq not in (0, 0xffffffff) and crc == seq_crc(seq) and
                  state not in (STATE_INVALID, STATE_ABORTED))
        result.append(Selection(seq, state, usable))
    return result


def boot_record(ota):
    """Index (0/1) of the selection record the next boot follows.

    The bootloader takes the highest usable sequence (a tie goes to the first
    record). One still pending verification was never confirmed by its app:
    the bootloader aborts it and follows the other record, which must be
    usable and settled; otherwise no safe choice exists.
    """
    records = selections(ota)
    usable = [i for i, record in enumerate(records) if record.usable]
    refuse(usable, 'No valid OTA selection record; cannot tell which slot boots')
    followed = max(usable, key=lambda i: records[i].seq)
    if records[followed].state != STATE_PENDING:
        return followed
    fallback = 1 - followed
    refuse(records[fallback].usable and records[fallback].state != STATE_PENDING,
           'A pending OTA selection has no confirmed fallback; cannot tell which slot boots')
    return fallback


def boot_slot(ota):
    return (selections(ota)[boot_record(ota)].seq - 1) % 2


@dataclass(frozen=True)
class Image:
    kind: str  # newt, other (another firmware), empty or invalid
    name: str = ''  # another firmware's name for people; empty for any other kind
    project: str = ''
    version: str = ''
    idf: str = ''
    elf_sha256: str = ''
    newt_version: str = ''  # Newt's identity block; empty when absent or unterminated
    newt_build: str = ''


def text(field):
    return field.split(b'\0', 1)[0].decode('ascii', 'replace')


def classify(head):
    """Newt by its fixed mark; any other valid app is another firmware."""
    refuse(len(head) >= HEAD, 'Incomplete slot header read')
    if head[:HEAD] == b'\xff' * HEAD:
        return Image('empty')
    if head[0] != APP_MAGIC or struct.unpack_from('<I', head, DESC_AT)[0] != DESC_MAGIC:
        return Image('invalid')
    desc = head[DESC_AT:DESC_AT + 0x100]
    fields = dict(version=text(desc[16:48]), project=text(desc[48:80]), idf=text(desc[112:144]),
                  elf_sha256=desc[144:176].hex())
    if head[NEWT_MARK_AT:NEWT_MARK_AT + len(NEWT_MARK)] != NEWT_MARK:
        return Image('other', name=FIRMWARE_NAMES.get(fields['project'], UNKNOWN_FIRMWARE), **fields)
    # As readNewtIdentity: both fields must end in NUL. Older Newt images
    # carry other bytes here.
    version = head[NEWT_IDENTITY_AT:NEWT_IDENTITY_AT + NEWT_VERSION_BYTES]
    build = head[NEWT_IDENTITY_AT + NEWT_VERSION_BYTES:NEWT_IDENTITY_AT + NEWT_VERSION_BYTES + NEWT_BUILD_BYTES]
    if version[-1] == 0 and build[-1] == 0:
        fields.update(newt_version=text(version), newt_build=text(build))
    return Image('newt', **fields)


MMU_PAGE = 0x10000


def segment_placed(load, data_at, length):
    """segmentPlaced: as the bootloader's verify_segment_header, a length
    that is a multiple of 4, and a segment the cache maps (DROM or IROM) at
    the same offset in its 64 KiB MMU page as its data. Both slots start on a
    page, so the offset in the slot counts."""
    mapped = 0x3c000000 <= load < 0x3c800000 or 0x42000000 <= load < 0x42800000
    return length % 4 == 0 and (not mapped or load % MMU_PAGE == data_at % MMU_PAGE)


def _walk(data, slot_bytes):
    """(segments end, digest offset, XOR checksum, digest appended) of an
    ESP32-C3 image with 1 to 16 segments inside the slot, else None. The
    digest offset ends the checksum's block. Reads past `data` fail, as the
    firmware's failed flash reads do."""
    def read(at, count):
        return data[at:at + count] if at + count <= len(data) else None

    header = read(0, 24) if slot_bytes >= 24 else None
    if (header is None or header[0] != APP_MAGIC or not 0 < header[1] <= 16 or
            struct.unpack_from('<H', header, 12)[0] != ESP32C3_CHIP_ID or header[23] > 1):
        return None
    at, checksum = 24, 0xef
    for _ in range(header[1]):
        entry = read(at, 8) if at + 8 <= slot_bytes else None
        if entry is None:
            return None
        at += 8
        load, length = struct.unpack_from('<II', entry)
        if not segment_placed(load, at, length):
            return None
        segment = read(at, length) if at + length <= slot_bytes else None
        if segment is None:
            return None
        for byte in segment:
            checksum ^= byte
        at += length
    digest_at, appended = (at // 16 + 1) * 16, header[23] == 1
    return None if digest_at + 32 * appended > slot_bytes else (at, digest_at, checksum, appended)


def image_intact(data, slot_bytes):
    """appImageIntact (include/newt/app_slot_identity.h) on a slot's bytes:
    the structure _walk checks, the XOR checksum as the last byte of its
    16-byte block and, when appended, the SHA-256 of every byte before the
    digest."""
    walked = _walk(data, slot_bytes)
    if walked is None or walked[1] + 32 * walked[3] > len(data):
        return False
    at, digest_at, checksum, appended = walked
    return data[digest_at - 1] == checksum and (
        not appended or hashlib.sha256(data[:digest_at]).digest() == data[digest_at:digest_at + 32])


def image_length(data):
    """The bytes the image at the start of `data` (a slot or a file) occupies,
    through its appended SHA-256 when it has one, or None when its structure
    is not complete inside `data`."""
    walked = _walk(data, len(data))
    return None if walked is None else walked[1] + 32 * walked[3]


def extra_data(data):
    """Whether a chosen firmware file holds bytes other than erased padding
    (0xff) after the image at its start. Only the image is installed, so
    padding is dropped; anything else means the file is not the image its
    digest describes (#368). The install page (checkFile) and the card install
    (check_update_image) apply this rule; a Newt release pair is exact."""
    length = image_length(data)
    return length is not None and data[length:].strip(b'\xff') != b''


@dataclass(frozen=True)
class Layout:
    other: int | None  # slot index holding another firmware; None when neither slot does
    newt: int  # the only slot a tool may write: Newt's beside another firmware, else the one that does not boot
    boot: int  # slot index the bootloader selects
    images: tuple

    @property
    def newt_offset(self):
        return SLOTS[self.newt]

    @property
    def other_offset(self):
        return None if self.other is None else SLOTS[self.other]

    def report(self, ota):
        return {'boot': f'app{self.boot}', 'other': None if self.other is None else f'app{self.other}',
                'newt': f'app{self.newt}',
                'selection': [{'seq': r.seq, 'state': STATE_NAMES.get(r.state, hex(r.state)), 'usable': r.usable}
                              for r in selections(ota)],
                'slots': [{'slot': f'app{i}', 'offset': hex(SLOTS[i]), **vars(image)}
                          for i, image in enumerate(self.images)]}


def decide(ota, heads, repair=False):
    """The layout, or Refused. `repair` allows only rewriting an unreadable
    slot beside the running, selected and confirmed Newt (docs/082 section 1)."""
    refuse(len(heads) == 2, 'Expected both app slot headers')
    images = tuple(classify(head) for head in heads)
    other = [i for i, image in enumerate(images) if image.kind == 'other']
    if other:
        # Exactly one other firmware, kept: Newt's slot is the other one.
        refuse(len(other) == 1, 'Both slots hold another firmware; neither is Newt\'s')
        refuse(all(image.kind != 'invalid' for image in images), 'A slot holds an unreadable image')
        layout = Layout(other[0], 1 - other[0], boot_slot(ota), images)
    else:
        layout = Layout(None, *running_newt(ota, images), images)
    if layout.images[layout.newt].kind == 'invalid':
        refuse(repair, f'app{layout.newt} holds an unreadable image; '
                       'pass --repair-invalid to rewrite only that slot')
    else:
        refuse(not repair, f'app{layout.newt} is not damaged; --repair-invalid has nothing to repair')
    return layout


def running_newt(ota, images):
    """(the slot to write, the boot slot) without another firmware.

    The slot that boots must hold Newt, be what the highest record selects
    (no newer selection still pending above it) and be confirmed
    (valid, or undefined without rollback). A new record could still roll
    back to the other slot, so that fallback is never overwritten.
    """
    boot = boot_slot(ota)
    refuse(images[boot].kind == 'newt',
           f'Neither slot holds another firmware, and app{boot}, the slot that boots, does not hold Newt')
    records = selections(ota)
    current = records[boot_record(ota)]
    refuse(current.seq == max(record.seq for record in records if record.usable),
           'A newer selection is still pending; let the X3 boot once (slots does) and retry')
    refuse(current.state in (STATE_VALID, STATE_UNDEFINED),
           f'The selection of app{boot} is not confirmed ({STATE_NAMES.get(current.state, hex(current.state))}); '
           'let Newt boot and confirm it, then retry')
    return 1 - boot, boot


def require_newt_write(layout, offset, length):
    """The only write into an app slot: the layout's writable slot. Never
    the other firmware's, and without one never the Newt that boots."""
    refuse(offset == layout.newt_offset and offset != layout.other_offset,
           'Writes go only to the slot that does not hold the other firmware' if layout.other is not None else
           f'Writes go only to app{layout.newt}, the slot that does not boot')
    refuse(type(length) is int and 0 < length <= SLOT_SIZE, 'Write exceeds the app slot')


def selection_record(ota, slot, state=STATE_NEW):
    """(offset, sector) of a record selecting `slot` in `state`, as ESP-IDF
    writes one: the smallest higher sequence for that slot, in the record the
    next boot does not follow. That record is older, or a pending one the
    bootloader would abort, so the record the device falls back to is never
    touched. 0xffffffff marks an erased record, so a selection that needs it
    is refused rather than written."""
    refuse(slot in (0, 1), 'Unknown slot')
    current = boot_record(ota)
    above = selections(ota)[current].seq
    seq = above + 1
    while (seq - 1) % 2 != slot:
        seq += 1
    if seq >= 0xffffffff:
        raise SequenceExhausted(f'No OTA sequence above {above:#x} selects app{slot}')
    entry = struct.pack('<I', seq) + b'\xff' * 20 + struct.pack('<II', state, seq_crc(seq))
    return OTADATA + (1 - current) * SECTOR, entry.ljust(SECTOR, b'\xff')


def applied(ota, write):
    """The selection records after one (offset, sector) write."""
    at = write[0] - OTADATA
    return ota[:at] + write[1] + ota[at + len(write[1]):]


def selection_write(layout, ota, slot):
    """Boot-only: the record that makes the next boot select `slot`, or None
    when it already does."""
    return None if layout.boot == slot else selection_record(ota, slot)


def install_selection(layout, ota):
    """(fallback, fresh): the selection writes around an install into
    layout.newt, in order. `fresh` is always a new record for the written
    slot, so its first boot is pending and returns to the other record unless
    the image confirms itself.

    The written slot boots only beside another firmware (bench: Newt app1,
    main: Newt app0), where the install rewrites the running Newt in place.
    Its record would then roll back to the new bytes, so `fallback` first
    selects the other firmware with a settled record, before any app byte
    changes. Otherwise
    `fallback` is None and the followed record, the running image's, stays
    the one a failed first boot returns to.
    """
    fallback = None
    if layout.boot == layout.newt:
        fallback = selection_record(ota, layout.other, STATE_VALID)
        ota = applied(ota, fallback)
    return fallback, selection_record(ota, layout.newt)


def fallback_slot(ota):
    """The slot the next boot returns to when the image it starts never
    confirms: the other record if the followed one is new or pending, else
    the followed slot itself."""
    records = selections(ota)
    followed = boot_record(ota)
    if records[followed].state not in (STATE_NEW, STATE_PENDING):
        return (records[followed].seq - 1) % 2
    other = records[1 - followed]
    refuse(other.usable and other.state not in (STATE_NEW, STATE_PENDING),
           'A new selection has no settled record to return to')
    return (other.seq - 1) % 2
