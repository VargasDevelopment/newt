"""Read actual C++ observations from the canonical, bounded binary fixture.

Capture/conversion is an explicit Root operation outside the test path. Native
host tests need only native_sha.cpp plus their existing product ABI/Expat objects.
"""
from pathlib import Path
import shutil
import struct
import zlib

KINDS = ('forwarding', 'publication', 'empty_styles', 'passes', 'map')
MATRIX = [(kind, i) for kind, ids in zip(KINDS, (range(22), range(22, 34), range(4), range(10), range(7))) for i in ids]
CAP = 262144
FILENAME = 'expected-outputs.bin.zlib'


def load(packet, complete_requests):
    packed = (Path(packet) / FILENAME).read_bytes()
    if len(packed) > 32 * 1024 * 1024:
        raise ValueError('producer fixture exceeds file cap')
    raw = zlib.decompress(packed)
    if len(raw) > 32 * 1024 * 1024:
        raise ValueError('producer fixture exceeds file cap')
    at = 0

    def take(n):
        nonlocal at
        if n < 0 or at + n > len(raw):
            raise ValueError('truncated producer fixture')
        value = raw[at:at+n]
        at += n
        return value

    def words(n):
        return list(struct.unpack('<' + 'I' * n, take(n * 4)))

    def blob(cap):
        n, = words(1)
        if n > cap:
            raise ValueError('producer fixture exceeds record cap')
        return take(n)

    if take(4) != b'NPF1' or words(1) != [55]:
        raise ValueError('producer fixture schema/count')
    requests, outputs = [], []
    for kind, ident in MATRIX:
        if words(2) != [KINDS.index(kind), ident]:
            raise ValueError('producer fixture matrix')
        requests.append(dict(kind=kind, id=ident, input_hex=blob(16384).hex()))
        small = kind in KINDS[:3]
        out = dict(fields=words(12)) if small else dict(state=words(16), starts=words(32))
        out['data_hex'] = blob(4096 if small else CAP).hex()
        n, = words(1)
        if n > (8192 if small else CAP):
            raise ValueError('producer fixture trace cap')
        out['trace'] = words(n)
        if small:
            if out['fields'][10:] != [len(out['data_hex']) // 2, n]:
                raise ValueError('producer fixture fields/counts')
        elif out['state'][15] != n:
            raise ValueError('producer fixture state/trace count')
        if kind == 'publication':
            out['probes'], = words(1)
        if kind == 'map':
            out['facts'] = words(2)
        outputs.append(out)
    if at != len(raw) or requests != complete_requests:
        raise ValueError('producer fixture trailing bytes or changed complete requests')
    return outputs


def stage(workspace, packet=None):
    """Copy the one fixture beside the unchanged snapshotted Rust workspace."""
    source = Path(packet) if packet is not None else Path(__file__).resolve().parent
    destination = Path(workspace) / 'tests/producer_forwarding' / FILENAME
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source / FILENAME, destination)
    linebreak = Path(workspace) / 'tests/producer_stream/linebreak.rs'
    linebreak.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source.parent / 'producer_stream/linebreak.rs', linebreak)
    return destination
