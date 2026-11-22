# SPDX-License-Identifier: MIT
"""Proposed type-preserving semantic controls, not compile/run verified.
MIT; see LICENSE.
"""
_FRAME = 'core/src/frame.rs'
CONTROLS = (
    ('prefix shared column rotation', _FRAME,
     'let source_column = WIDTH - 1 - row;', 'let source_column = row;'),
    ('prefix gathered bit order', _FRAME,
     'value &= !(0x80 >> bit);', 'value &= !(1 << bit);'),
    ('prefix incomplete row retained', _FRAME,
     '.chunks_mut(NATIVE_ROW_BYTES)', '.chunks_exact_mut(NATIVE_ROW_BYTES)'),
    ('prefix untouched suffix', _FRAME,
     'if orientation == Orientation::LandscapeCcw {\n        for (dst, &src)',
     'native[bytes..].fill(0);\n    if orientation == Orientation::LandscapeCcw {\n        for (dst, &src)'),
    ('prefix invalid request atomicity', _FRAME,
     'if bytes > FRAME_BYTES {\n        return false;',
     'if bytes > FRAME_BYTES {\n        native[0] ^= 1;\n        return false;'),
)
