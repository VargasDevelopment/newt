"""Proposed additions to the existing pinned-oracle corpus; not integrated yet.

Pass the existing prefs/hx fixture builders. Protocol:
preferences_store MASK SCRIPT SLOT0 SLOT1 OP SEQUENCE ORIENTATION BAR CADENCE
MASK adds 256 for exact-capacity NVS copying and 512 for a changed short reread.
"""


def preferences_cases(prefs, hx):
    cases = []

    def add(mask, script, first, second, operation, sequence=999, orientation=0, bar=1, cadence=5):
        cases.append(' '.join(map(str, ['preferences_store', mask, script, first, second,
                                        operation, sequence, orientation, bar, cadence])))

    valid = prefs(sequence=9)
    damaged = bytearray(prefs(sequence=10)); damaged[-1] ^= 1
    legacy = prefs(sequence=9, version=1, cadence=0)
    future = prefs(sequence=10, version=3)
    variants = [
        '-', '_', hx(valid), hx(prefs(sequence=10, orientation=0)),
        hx(prefs(sequence=9, orientation=0)), hx(prefs(sequence=0x80000009)),
        hx(prefs(sequence=0xffffffff)), hx(prefs(sequence=0)), hx(legacy),
        hx(damaged), hx(future), hx(future[:8]), hx(future[:23]),
        hx(valid[:7]), hx(valid[:8]), hx(valid[:23]), hx(valid + b'\0'),
    ]
    for exact in [0, 256]:
        for first in variants:
            for second in variants:
                for operation in ['load', 'save']:
                    add(exact, 'g', first, second, operation)

    for exact in [0, 256]:
        for failure in [0, 1, 2, 4, 8, 16, 32, 64, 128, 512]:
            for script in ['g', 'a', 'bg', 'bb', 'ba', 't']:
                for operation in ['load', 'save']:
                    for peer in ['-', hx(legacy), hx(future[:12])]:
                        add(exact | failure, script, hx(valid), peer, operation)

    # Same decoded settings must not upgrade a valid v1 record or wear NVS.
    for script in ['g', 'a', 'bg', 'bb']:
        add(256, script, hx(legacy), '-', 'save', orientation=1)
        add(256 | 128, script, hx(legacy), '-', 'save', orientation=1)

    # A changed short reread is a storage failure. A stable short newer header
    # is unsupported, including when the other slot is valid.
    for size in [8, 12, 23]:
        for swapped in [False, True]:
            pair = [hx(valid), hx(future[:size])]
            if swapped:
                pair.reverse()
            for operation in ['load', 'save']:
                add(256, 'g', *pair, operation)
                add(256 | 512, 'g', *pair, operation)

    # Invalid cadence is rejected before any writable open. Orientation only
    # takes 0/1 here because the Rust type excludes other values by construction;
    # malformed serialized orientations stay covered by preference codec tests.
    for cadence in [0, 2, 255]:
        add(0, 'g', '-', '-', 'save', cadence=cadence)
    return cases
