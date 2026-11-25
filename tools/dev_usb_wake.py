"""Decode the opt-in, awake-only timeline without changing shared receipts."""
import json
from datetime import datetime, timezone
import re
import struct

STAGES = ('entry', 'serial', 'probe', 'display_ready', 'sd', 'application',
          'home_restore', 'reader_restore', 'input_ready', 'picture_candidate',
          'source_ready', 'page_ready', 'picture_ready', 'display_start',
          'display_return', 'page_complete')


def decode_wake_timing(body):
    if body == 'WAKE version=1 available=0':
        return {'version': 1, 'available': False}
    match = re.fullmatch(r'WAKE version=1 available=1 data=([0-9a-f]{192})', body)
    if match is None:
        raise ValueError('Malformed wake timeline')
    words = struct.unpack('<24I', bytes.fromhex(match[1]))
    schema, valid, flags, reset, cause, cpu, route, panel = words[16:]
    if schema != 1 or valid >> 16 or valid & 0x8001 != 0x8001 or flags & 3 != 1 or flags & ~0xff01:
        raise ValueError('Invalid or unfrozen wake timeline')
    if any(value != 0 for index, value in enumerate(words[:16]) if not valid & (1 << index)):
        raise ValueError('Unavailable wake stamp has a value')
    return {'version': schema, 'available': True, 'clock': 'boot-us32',
            'stamps': {name: words[index] if valid & (1 << index) else None
                       for index, name in enumerate(STAGES)},
            'since_entry_us': {name: (words[index] - words[0]) & 0xffffffff
                               if valid & (1 << index) else None for index, name in enumerate(STAGES)},
            'reset': reset, 'cause': cause, 'cpu_mhz': cpu,
            'page_kind': {1: 'picture', 2: 'ordinary'}.get(route & 255, 'unknown'),
            'requested_screen': (route >> 8) & 255, 'restored': bool(route & 0x10000),
            'panel': panel & 255, 'refresh_mode': panel >> 8, 'display_calls': flags >> 8}


# Leave about 95 seconds of margin below the 32-bit microsecond period.
MAX_WAKE_GAP_SECONDS = 70 * 60


def load_wake_before(path):
    """A missing/corrupt local witness never grants timing validity."""
    if path is None:
        return None
    try:
        with path.open('rb') as stream:
            data = stream.read(1024 * 1024 + 1)
        return json.loads(data) if len(data) <= 1024 * 1024 else None
    except (OSError, ValueError, RecursionError):
        return None


def witness_age(before, boot, device, now):
    """A different boot since a recent same-device DIAG bounds this boot's age."""
    try:
        previous = before['protocol']['boot']
        diag = before['diagnostics']
        if (before.get('simulation') is not False or 'error' in before
                or type(previous) is not int or not 0 <= previous <= 0xffffffff
                or previous == boot or diag.get('available') is not True
                or not isinstance(device, str) or not re.fullmatch('[0-9a-f]{16}', device)
                or diag.get('device') != device):
            return None
        observations = [row for row in before['protocol']['records']
                        if row.get('direction') == 'response' and row.get('kind') == 'DIAG'
                        and row.get('boot') == previous]
        observed = datetime.fromisoformat(observations[-1]['utc'])
        if observed.tzinfo is None:
            return None
        age = (now - observed).total_seconds()
        return age if 0 <= age < MAX_WAKE_GAP_SECONDS else None
    except (KeyError, TypeError, ValueError, IndexError, AttributeError):
        return None


def read_wake_timing(body, *, before, boot, device, now=None):
    decoded = decode_wake_timing(body)
    raw = {'body': body, 'clock': 'boot-us32'}
    if not decoded['available']:
        return {**decoded, 'valid': False, 'status': 'snapshot unavailable', 'raw': raw}
    age = witness_age(before, boot, device, now or datetime.now(timezone.utc))
    if age is None:
        return {'version': 1, 'available': True, 'valid': False,
                'status': 'freshness unverified, not valid', 'raw': raw}
    return {**decoded, 'valid': True, 'status': 'valid', 'raw': raw,
            'witness_gap_seconds': age}
