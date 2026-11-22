# SPDX-License-Identifier: MIT
"""The preview's update and firmware-switch worlds, as fixture files.

The preview runs the real firmware over C fakes (fakes/). This writes what
those fakes read, under one NEWT_PREVIEW_DIR: the card's update offers, the
two app slots and otadata, the GitHub responses, and fixture.conf. The
firmware's own policy decides every page from them; nothing here names a
page. Images are small, valid ESP32-C3 app images, so the firmware's own
classification, digest and integrity checks pass or fail for real.

`write(directory, update=..., other_firmware=..., run=...)`: `run(keys)`
replays presses through the driver; fixtures that need a saved Wi-Fi network
use it so the firmware's own Save writes the card record and the NVS key.
"""
import hashlib
from pathlib import Path
import re
import struct
import zlib

ROOT = Path(__file__).resolve().parents[3]
RUNNING = re.search(r'#define NEWT_FIRMWARE_VERSION "([^"]+)"',
                    (ROOT / 'rust/firmware/native/firmware_version.h').read_text())[1]
NEWER, OLDER = '9.0.0', '0.0.9'
BUILD = '0123456789ab'
SLOT_BYTES, SECTOR = 0x770000, 4096
STATE_VALID, STATE_ABORTED = 2, 4

# Scan order; the first scan hears the first seven, each rescan one more.
NETWORKS = [(-71, 0, 'Lighthouse Reading Room'), (-48, 0, ''), (-58, 0, 'Harbour Cafe Guest'),
            (-52, 0, 'Lighthouse Reading Room'), (-66, 0, 'Maple Street Loft 5G Extended Ne'),
            (-80, 0, 'Bibliothèque Été'), (-88, 1, 'Tower 7'), (-63, 0, 'Garden Studio'),
            (-75, 0, 'Riverside Commons'), (-84, 0, 'Orchard Lane 2.4')]
# Buttons: 0 Back, 1 Confirm, 5 Down. From Home on a card that ran Newt:
# Settings, its Wi-Fi row, then the open Tower 7 (fifth listed network). The
# join saves it, the firmware checks GitHub, and Back restarts into Newt.
SAVE_OPEN_NETWORK = [5, 5, 1, *[5] * 7, 1, *[5] * 5, 1, 0]


def image(project, version=None, newt=None, intact=True):
    """One-segment ESP32-C3 app image: its descriptor (version, project) sits
    at the segment start, a Newt identity (version, build) at 288, then the
    checksum byte and the appended SHA-256 that app_image_intact checks."""
    data = bytearray(512)
    data[0:4] = struct.pack('<I', 0xabcd5432)
    data[16:16 + len(version or '')] = (version or '').encode()
    data[48:48 + len(project)] = project.encode()
    if newt:
        data[256:272] = b'NEWT-X3-APP\0\x01\0\0\0'
        data[272:272 + len(newt)] = newt.encode()
        data[296:308] = BUILD.encode()
    header = bytes([0xe9, 1, 2, 0x20]) + struct.pack('<I', 0x42000000) + bytes(4) + struct.pack('<H', 5) + bytes(9) + b'\1'
    body = header + struct.pack('<II', 0x3c000020, len(data)) + bytes(data)
    checksum = 0xef
    for byte in data:
        checksum ^= byte
    tail = bytearray((len(body) // 16 + 1) * 16 - len(body))
    tail[-1] = checksum
    body += bytes(tail)
    digest = hashlib.sha256(body).digest()
    return body + (digest if intact else bytes(32))


def newt_image(version):
    return image('newt', version, newt=version)


def manifest(version, payload):
    return (f'format=newt-update-1\nkind=newt\nversion={version}\nbuild={BUILD}\n'
            f'size={len(payload)}\nsha256={hashlib.sha256(payload).hexdigest()}\n').encode()


def otadata(*records):
    """Two otadata sectors; each record is (seq, state) or None (erased)."""
    data = bytearray(b'\xff' * 2 * SECTOR)
    for index, record in enumerate(records):
        if record:
            seq, state = record
            data[index * SECTOR:index * SECTOR + 32] = (struct.pack('<I', seq) + b'\xff' * 20
                + struct.pack('<II', state, zlib.crc32(struct.pack("<I", seq), 0xffffffff)))
    return bytes(data)


OTHERS = {'crossink': image('CrossInk', '1.2.0'), 'crosspoint': image('crosspoint-reader', '2.0.0'),
          'stock': image('x3-reader', 'V6.5.5'), 'missing': None, 'failing': image('CrossInk', '1.2.0', intact=False),
          'unsaved': image('CrossInk', '1.2.0'), 'unconfirmed': image('CrossInk', '1.2.0')}
# What else each other-firmware choice changes: a reader save that cannot be
# confirmed, or an otadata read that fails after the slot checks pass.
OTHER_CONF = {'unsaved': {'store.drop': 'card-sync:/newt-sd/Newt/State/nb'}, 'unconfirmed': {'slot.fail': 'otadata-read'}}

CARD_FILES = {'crosspoint-2.0.0-x3.bin': image('crosspoint-reader', '2.0.0'),
              'reader-6.5.bin': image('x3-reader', 'V6.5.5'), 'notes.bin': b'not firmware\n' * 64}
MORE_FILES = {'crosspoint-2.0.0-x3.bin': CARD_FILES['crosspoint-2.0.0-x3.bin'], 'newt-old.bin': newt_image(OLDER),
              'reader-6.5.bin': CARD_FILES['reader-6.5.bin'], 'zz-extra.bin': image('x3-reader', 'V7.0.0')}
LATEST = 'https://api.github.com/repos/VargasDevelopment/newt-releases/releases/latest'


def release(version):
    """GitHub's latest-release JSON and its two assets."""
    payload = newt_image(version)
    assets = {'https://example.invalid/newt.manifest': manifest(version, payload),
              'https://example.invalid/newt.bin': payload}
    listing = ','.join(f'{{"name":"{url.rsplit("/", 1)[1]}","browser_download_url":"{url}","size":{len(body)}}}'
                       for url, body in assets.items())
    return {LATEST: f'{{"tag_name":"v{version}","assets":[{listing}]}}'.encode(), **assets}


def update_world(name):
    """(card offer version | None, card firmware files, app1 image or 'other',
    conf, http responses, needs a saved network) for one --update name."""
    world = dict(offer=NEWER, files={}, app1='other', conf={}, http={}, saved=False, status={})
    offers = {'newer': NEWER, 'same': RUNNING, 'older': OLDER}
    if name in offers:
        world['offer'] = offers[name]
        # Replacing another firmware asks first, so a reinstall's own question
        # needs an empty other slot.
        if name == 'same':
            world['app1'] = None
    elif name in ('firmware', 'firmware-other-slot', 'firmware-empty-slot'):
        world['files'] = CARD_FILES
        world['app1'] = {'firmware': newt_image(RUNNING), 'firmware-other-slot': 'other',
                         'firmware-empty-slot': None}[name]
    elif name == 'firmware-more':
        world['files'] = MORE_FILES
    elif name == 'firmware-unconfirmed':
        world.update(offer=None, files={'crosspoint-2.0.0-x3.bin': CARD_FILES['crosspoint-2.0.0-x3.bin']},
                     conf={'slot.fail': 'otadata-read'})
    elif name == 'other-slot':
        pass
    elif name == 'empty-slot':
        world['app1'] = None
    elif name == 'rolled-back':
        world['otadata'] = otadata((1, STATE_VALID), (2, STATE_ABORTED))
    elif name == 'unreadable':
        world['manifest'] = b'format=newt-update-1\nkind=newt\nversion=broken\n'
    elif name == 'unsaved':
        world['conf'] = {'store.drop': 'card-sync:/newt-sd/Newt/State/nb'}
    elif name == 'updated':
        world.update(offer=None, conf={'image.state': '1'})
    elif name == 'none':
        world['offer'] = None
    elif name == 'no-card':
        world.update(offer=None, conf={'card.absent': '1'})
    elif name in ('fail-write', 'fail-unsettled', 'fail-unconfirmed'):
        world['conf'] = {'fail-write': {'slot.fail': 'app1-write'}, 'fail-unsettled': {'slot.lose': 'app1'},
                         'fail-unconfirmed': {'slot.fail': 'otadata-reread'}}[name]
    elif name in ('wifi-none', 'wifi-failed'):
        world['conf'] = {'wifi.scan': name[5:]}
    elif name == 'wifi-scanning':
        world['conf'] = {'freeze': 'wifi.scan'}
    elif name == 'wifi-not-saved':
        world['conf'] = {'store.refuse': 'nvs-set:newt_wifi'}
    elif name == 'wifi-unconfirmed':
        # A first save makes the record file; the next save rewrites it in
        # place and loses that sync, so the new bytes may or may not last.
        world.update(saved=True, conf={'store.drop': 'card-sync:/newt-sd/Newt/State/newt-net.nrs'})
    elif name == 'writing':
        world['conf'] = {'freeze': 'slot.write'}
    elif name in ('wifi-saved', 'wifi-joining', 'wifi-wrong-password', 'wifi-not-found'):
        world.update(saved=True, conf={'wifi-saved': {}, 'wifi-joining': {'freeze': 'wifi.join'},
                                       'wifi-wrong-password': {'wifi.join': 'wrong-password'},
                                       'wifi-not-found': {'wifi.join': 'not-found'}}[name])
    elif name in ('check-latest', 'check-no-release', 'check-ready', 'check-downloading'):
        world.update(offer=None, saved=True, http=release(RUNNING if name == 'check-latest' else NEWER))
        if name == 'check-no-release':
            world.update(http={}, status={LATEST: 404})
        if name == 'check-downloading':
            world['conf'] = {'freeze': 'http.read', 'http.freeze_bytes': '256'}
    else:
        raise ValueError(f'unknown update fixture: {name}')
    return world


def http_name(url):
    return re.sub(r'[^A-Za-z0-9.-]', '_', url)


def write(directory, update=None, other_firmware='crossink', run=None):
    """Fill `directory` (NEWT_PREVIEW_DIR) for one preview world. Without an
    `update`, the card offers nothing and Wi-Fi hears the default networks."""
    directory = Path(directory)
    world = update_world(update) if update else dict(offer=None, files={}, app1='other', conf={},
                                                     http={}, saved=False, status={})
    if other_firmware not in OTHERS:
        raise ValueError(f'unknown other firmware: {other_firmware}')
    conf = {'wifi.networks': ';'.join(f'{rssi},{open_},{name}' for rssi, open_, name in NETWORKS),
            'wifi.first': '7', **OTHER_CONF.get(other_firmware, {})}
    partitions = directory / 'partitions'
    partitions.mkdir(parents=True, exist_ok=True)
    (partitions / 'app0.bin').write_bytes(newt_image(RUNNING))
    app1 = OTHERS[other_firmware] if world['app1'] == 'other' else world['app1']
    if app1:
        (partitions / 'app1.bin').write_bytes(app1)
    (partitions / 'otadata.bin').write_bytes(world.get('otadata') or otadata((1, STATE_VALID)))
    folder = directory / 'card/Newt/Update'
    folder.mkdir(parents=True, exist_ok=True)
    if world['offer']:
        payload = newt_image(world['offer'])
        (folder / 'newt.bin').write_bytes(payload)
        (folder / 'newt.manifest').write_bytes(world.get('manifest') or manifest(world['offer'], payload))
    for name, data in world['files'].items():
        (folder / name).write_bytes(data)
    (directory / 'card/Newt/State').mkdir(parents=True, exist_ok=True)
    if world['saved']:
        if run is None:
            raise ValueError(f'{update} needs a saved network: pass run')
        (directory / 'fixture.conf').write_text(''.join(f'{k}={v}\n' for k, v in conf.items()))
        run(SAVE_OPEN_NETWORK)
    responses = directory / 'http'
    responses.mkdir(exist_ok=True)
    for url, body in world['http'].items():
        (responses / http_name(url)).write_bytes(b'200\nDate: Fri, 20 Nov 2026 12:00:00 GMT\n\n' + body)
    for url, status in world['status'].items():
        (responses / http_name(url)).write_bytes(f'{status}\n\n'.encode())
    conf.update(world['conf'])
    (directory / 'fixture.conf').write_text(''.join(f'{k}={v}\n' for k, v in conf.items()))
