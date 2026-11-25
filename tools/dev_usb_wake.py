"""Decode the opt-in, awake-only timeline without changing shared receipts."""
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
