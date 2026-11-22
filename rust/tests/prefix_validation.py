"""Malformed frame replies must not count as behavioral mutation witnesses."""
from frame_cases import FRAME_BYTES

def validate(reply):
    fields = reply.split(' ')
    if len(fields) != 2 or fields[0] not in ('0', '1') or len(fields[1]) != 2 * FRAME_BYTES:
        raise ValueError('invalid prefix result/extent')
    try:
        pixels = bytes.fromhex(fields[1])
    except ValueError as error:
        raise ValueError('invalid prefix hex') from error
    if pixels.hex() != fields[1]:
        raise ValueError('noncanonical prefix hex')


def batch_lines(raw, count):
    if not isinstance(raw, bytes) or not raw.endswith(b'\n') or b'\r' in raw:
        raise ValueError('complete raw LF prefix response required')
    try:
        lines = raw[:-1].decode('ascii').split('\n')
    except UnicodeDecodeError as error:
        raise ValueError('prefix response must be ASCII') from error
    if len(lines) != count:
        raise ValueError('prefix response row count differs')
    return lines
