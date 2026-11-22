# SPDX-License-Identifier: MIT
"""Retained-input license guard helper checked by rust/tests/landing_test.py."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_inputs(tool_root):
    provenance = json.loads((HERE / 'provenance.json').read_text())
    for name, expected in provenance['packet_sha256'].items():
        require(digest(HERE / name) == expected, 'frozen packet license changed: ' + name)
    for name, expected in provenance['retained_inputs_sha256'].items():
        require(digest(tool_root / name) == expected, 'retained input changed: ' + name)
