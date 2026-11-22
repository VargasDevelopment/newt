#!/usr/bin/env python3
"""Encode the built-in app from apps/pocket-pause.json into the portable artifact
and the copy the firmware embeds; no device/storage access."""
import argparse
import hashlib
from pathlib import Path

from newt_package import validate_source, encode_app

ROOT = Path(__file__).resolve().parents[1]


def generated() -> bytes:
    return encode_app(validate_source((ROOT / "apps/pocket-pause.json").read_bytes()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    payload = generated()
    for path in (ROOT / "artifacts/pocket-pause.newt", ROOT / "rust/tiny-app/src/pocket-pause.app"):
        if args.check:
            if not path.is_file() or path.read_bytes() != payload:
                raise SystemExit(f"Generated app differs: {path.relative_to(ROOT)}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
    print("PASS: built-in app equals the portable artifact and the firmware copy; SHA-256 " + hashlib.sha256(payload).hexdigest())


if __name__ == "__main__":
    main()
