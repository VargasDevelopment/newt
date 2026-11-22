"""Synthetic firmware, USB identity and byte-addressed flash; never opens a port."""
import hashlib
import json
import struct
import zlib

import bench_candidate as candidate
import x3_slots

MAC = '02:12:23:34:45:56'
IDENTITY = {'mac': MAC, 'port': '/dev/serial/by-id/usb-Espressif_USB_JTAG_serial_debug_unit_' + MAC.upper() + '-if00'}
REVISION = 'a1' * 20
BUILD = REVISION[:12]  # the DEV build ID the image embeds and HELLO reports
RULE = 'SUBSYSTEM=="tty", ATTRS{idVendor}=="303a", ATTRS{serial}=="' + MAC.upper() + '", MODE="0600"\n'
APP0, APP1 = x3_slots.SLOTS
# One install on the bench layout (CrossInk app0, Newt app1 running) rewrites
# app1 in place: CrossInk's settled fallback record, the app, then Newt's
# fresh record (x3_slots.install_selection).
BENCH_INSTALL = [(0xe000, 4096), (APP1, 4096), (0xf000, 4096)]


def table_bytes():
    entries = [(1, 2, 0x9000, 0x5000, b'nvs'), (1, 0, 0xe000, 0x2000, b'otadata'),
               (0, 0x10, 0x10000, 0x770000, b'app0'), (0, 0x11, APP1, 0x770000, b'app1')]
    data = b''.join(struct.pack('<HBBII16sI', 0x50aa, kind, sub, at, size, name, 0)
                    for kind, sub, at, size, name in entries)
    data += b'\xeb\xeb' + b'\xff' * 14 + hashlib.md5(data).digest()
    return data.ljust(4096, b'\xff')


def descriptor(project, version, elf):
    data = bytearray(256)
    struct.pack_into('<I', data, 0, x3_slots.DESC_MAGIC)
    data[16:16 + len(version)] = version.encode()
    data[48:48 + len(project)] = project.encode()
    data[144:176] = bytes.fromhex(elf)
    return bytes(data)


def app_bytes(build, extra=0, project='arduino-lib-builder', newt=True, elf='11' * 32):
    """One segment: the app descriptor, Newt's mark when `newt`, then the build."""
    header = bytearray(24)
    header[:4] = b'\xe9\x01\x02\x4f'
    struct.pack_into('<H', header, 12, 5)
    header[23] = 1
    mark = x3_slots.NEWT_MARK if newt else b'\0' * len(x3_slots.NEWT_MARK)
    payload = descriptor(project, build, elf) + mark + build.encode() + b'\0' + bytes(range(255)) + b'x' * extra
    payload += b'\0' * (-len(payload) % 4)  # segment lengths are a multiple of 4, as esptool writes them
    data = bytes(header) + struct.pack('<II', 0x3c000020, len(payload)) + payload
    checksum = 0xef
    for byte in payload:
        checksum ^= byte
    data += b'\0' * (15 - len(data) % 16) + bytes([checksum])
    return data + hashlib.sha256(data).digest()


def make_candidate(directory, extra=0, table=None, variant=candidate.VARIANT, image=None, legacy=False):
    """The one DEV build; `variant` names a retired identity receipt for refusal cases."""
    directory.mkdir()
    build = BUILD + '-regional-experiment' if legacy else BUILD
    data = {'firmware.bin': app_bytes(build, extra) if image is None else image,
            'partitions.bin': table if table is not None else table_bytes(), 'gate.log': b'PASS\n'}
    data[f'firmware-identity-{variant}.json'] = json.dumps({'git_revision': REVISION, 'dirty': False,
        'variant': variant, 'binary_sha256': candidate.sha(data['firmware.bin'])}).encode()
    if not legacy:
        receipt = json.loads(data[f'firmware-identity-{variant}.json'])
        receipt['build_id'] = build
        data[f'firmware-identity-{variant}.json'] = json.dumps(receipt).encode()
    data['gate-receipt.json'] = json.dumps({'revision': REVISION, 'exit': 0,
        'log_sha256': candidate.sha(data['gate.log'])}).encode()
    data[f'code-health-{candidate.VARIANT}.json'] = json.dumps({'flash_bytes': 1234}).encode()
    for name, raw in data.items():
        (directory / name).write_bytes(raw)
    manifest = {'revision': REVISION, 'files': {name: candidate.sha(raw) for name, raw in data.items()},
                'flash': {candidate.VARIANT: 1234}}
    (directory / 'manifest.json').write_text(json.dumps(manifest))
    return candidate.sha((directory / 'manifest.json').read_bytes())


CROSSINK = app_bytes('v1.4.0.1', project='CrossInk', newt=False, elf='33' * 32)


def select(memory, at, seq, state=x3_slots.STATE_VALID):
    struct.pack_into('<I', memory, at, seq)
    struct.pack_into('<II', memory, at + 24, state, zlib.crc32(struct.pack('<I', seq), 0xffffffff))


class Fake:
    """bench: CrossInk in app0, Newt in app1 and selected. main: CrossInk in
    app1 and selected, an older Newt in app0. legacy: main with an older app
    that is not Newt in app0, so two slots hold another firmware."""

    def __init__(self, table, layout='bench'):
        self.memory = bytearray(b'\xff') * candidate.FLASH
        self.memory[0x8000:0x9000] = table
        crossink, other = (APP0, APP1) if layout == 'bench' else (APP1, APP0)
        self.memory[crossink:crossink + len(CROSSINK)] = CROSSINK
        image = app_bytes('legacy', newt=False, elf='22' * 32) if layout == 'legacy' else app_bytes('older-newt')
        self.memory[other:other + len(image)] = image
        for at, seq in ((0xe000, 5), (0xf000, 6)):  # seq 6 boots app1
            select(self.memory, at, seq)
        self.reads, self.writes, self.resets, self.hellos = [], [], 0, []
        self.connects = 0
        self.fail_write = self.corrupt_payload = False
        self.connect_error = self.on_write = self.flip_before = self.flip_during = None
        self.flipped = False
        self.port_open = False
        self.boots = []
        self.confirms = True  # False: the image that boots never confirms (it cannot paint)

    def connect(self):
        self.connects += 1
        if self.connect_error:
            raise self.connect_error
        self.port_open = True  # Download mode: the second-stage bootloader does not run.

    def read(self, offset, length):
        self.reads.append((offset, length))
        return bytes(self.memory[offset:offset + length])

    def md5(self, offset, length):
        # Corrupt a real boundary byte only after the original backup was confirmed.
        if self.flip_before is not None and length != candidate.FLASH and not self.flipped:
            self.memory[self.flip_before] ^= 1
            self.flipped = True
        return hashlib.md5(self.memory[offset:offset + length]).hexdigest()

    def write(self, layout, offset, payload):
        self.writes.append((offset, len(payload)))
        if self.on_write:
            self.on_write()
        app = offset >= x3_slots.SLOTS[0]  # the faults below model an app write, not a selection record
        if self.fail_write and app:
            self.memory[offset:offset + 16] = payload[:16]
            raise OSError('Injected partial write failure')
        self.memory[offset:offset + len(payload)] = payload
        if not app:
            return
        if self.corrupt_payload:
            self.memory[offset] ^= 1
        if self.flip_during is not None:
            self.memory[self.flip_during] ^= 1

    def write_spare(self, boot, offset, payload):
        self.write(None, offset, payload)

    def reset(self):
        self.resets += 1
        self.boot()

    def boot(self):
        """The ESP-IDF rollback bootloader, independent of x3_slots: follow
        the highest usable sequence; abort it if still pending verification
        (its app never confirmed) and follow the other; a new one boots as
        pending until the app confirms it."""
        records = x3_slots.selections(bytes(self.memory[x3_slots.OTADATA:x3_slots.OTADATA + x3_slots.OTADATA_SIZE]))
        usable = [i for i, record in enumerate(records) if record.usable]
        active = max(usable, key=lambda i: records[i].seq)
        if records[active].state == x3_slots.STATE_PENDING:
            self.set_state(active, x3_slots.STATE_ABORTED)
            usable.remove(active)
            active = max(usable, key=lambda i: records[i].seq)
        if records[active].state == x3_slots.STATE_NEW:
            self.set_state(active, x3_slots.STATE_PENDING)
        self.running = active
        self.boots.append((records[active].seq - 1) % 2)

    def set_state(self, record, state):
        struct.pack_into('<I', self.memory, x3_slots.OTADATA + record * x3_slots.SECTOR + 24, state)

    def hello(self, build):
        self.hellos.append(build)
        if not self.port_open:
            self.boot()  # Opening a port resets the X3.
            self.port_open = True
        slot = self.boots[-1]
        if x3_slots.classify(bytes(self.memory[x3_slots.SLOTS[slot]:][:x3_slots.HEAD])).kind != 'newt' or \
                not self.confirms:
            raise ValueError('Protocol response timed out')
        # Arduino confirms a pending selection before setup(), so before the bridge answers.
        self.set_state(self.running, x3_slots.STATE_VALID)
        return {'hello': build}

    def confirm_window(self):
        # Virtual time: initArduino confirms without a DEV bridge.
        self.set_state(self.running, x3_slots.STATE_VALID)

    def close(self):
        self.port_open = False
