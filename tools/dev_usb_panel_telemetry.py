"""Bounded literal telemetry only; no transport, book text or inferred waveforms."""
import base64
import re
import struct
import time

MAX_PRESS_TELEMETRY = 2048  # Recognized literals only; ordinary logs stay discarded.
REFRESH_TAG = re.compile(rb'(?<![A-Za-z0-9_])(?:8279_DIRECT_GRAY_DRF|8279_ISLAND_POF)(?![A-Za-z0-9_])')
DISPLAY_TAG = re.compile(rb'\[EPD\] displayBuffer mode=[012] off=[01](?=$|[ \t\r])')
# Ring schema 1, append-only IDs matching rust/firmware/dev_usb/panel.rs.
PANEL_MODES = ('fast', 'full', 'window-fast', 'gray-base-half', 'gray-overlay', 'gray-direct',
               'gray-direct-region', 'binary-island-fast', 'binary-regions-fast', 'reset', 'gray-base')
# IDs stay for old captures; no current call site, Frosted Home deleted 2026-10-26.
RETIRED_PANEL_MODES = ('gray-direct-region', 'binary-island-fast')
PANEL_REASONS = (
    'working-first-open', 'working-first-open-inverse', 'working-opening', 'working-opening-inverse',
    'working-saving', 'working-saving-inverse', 'working-applying', 'working-applying-inverse',
    'working-loading', 'working-loading-inverse', 'working-unknown', 'leave-overlay-inverse',
    'reader', 'ui', 'zone-focus', 'zone-cleanup', 'home-cleanup', 'home-inverse-rebase',
    'home-overlay-base', 'home', 'home-battery', 'home-focus', 'home-frosted', 'home-fallback',
    'home-stage-focus', 'home-stage', 'sleep-scene', 'sleep-fallback', 'boot-or-wake', 'resume-picture')
# IDs stay for old captures; no current call site, Frosted Home deleted 2026-10-26.
RETIRED_PANEL_REASONS = ('home-battery', 'home-focus', 'home-frosted')


def decode_panel_ring(body):
    match = re.fullmatch(r'PANEL version=1 capacity=31 next=(0|[1-9][0-9]{0,9}) count=(0|[1-9][0-9]?) data=([A-Za-z0-9+/=]+|-)', body)
    if not match:
        raise ValueError('Malformed panel ring')
    next_sequence, count = int(match[1]), int(match[2])
    if next_sequence > 0xffffffff or count > 31:
        raise ValueError('Panel ring bounds')
    raw = b'' if match[3] == '-' else base64.b64decode(match[3], validate=True)
    if len(raw) != count * 8 or (raw and base64.b64encode(raw).decode() != match[3]):
        raise ValueError('Panel ring encoding')
    entries = []
    for i, (started, duration, mode, reason_outcome) in enumerate(struct.iter_unpack('<IHBB', raw)):
        reason = reason_outcome & 63
        if mode & 127 >= len(PANEL_MODES) or reason >= len(PANEL_REASONS) or (mode & 128 and duration != 65535):
            raise ValueError('Unknown panel ring ID or duration')
        entries.append({'sequence': (next_sequence - count + i) & 0xffffffff,
            'ms': started, 'duration_ms': duration, 'duration_saturated': bool(mode & 128),
            'mode': PANEL_MODES[mode & 127], 'reason': PANEL_REASONS[reason],
            'outcome': ('accepted', 'rejected', 'fallback', 'pending')[reason_outcome >> 6]})
    return {'schema': 1, 'capacity': 31, 'next_sequence': next_sequence, 'entries': entries}


def collect_press_telemetry(line, active_press, press_audit):
    # Legacy latency evidence only. Panel diagnostics come from the boot ring.
    for match in sorted((match for pattern in (REFRESH_TAG, DISPLAY_TAG) for match in pattern.finditer(line)),
                        key=lambda value: value.start()):
        if len(press_audit) >= MAX_PRESS_TELEMETRY:
            raise ValueError('PRESS telemetry audit limit reached')
        boot, session, identifier = active_press
        press_audit.append({'boot': boot, 'session': session, 'request_id': identifier,
                           'tag': match[0].decode('ascii'), 'monotonic': time.monotonic()})
