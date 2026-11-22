#!/usr/bin/env python3
"""Write the certificate bundle update mode's HTTPS checks servers against (docs/UPDATES.md, "Updates over Wi-Fi").

The pinned framework's mbedTLS is built with ESP-IDF's certificate bundle
(CONFIG_MBEDTLS_CERTIFICATE_BUNDLE, its full default list) but ships no bundle
data: esp_crt_bundle_attach reads _binary_x509_crt_bundle_start/_end, which
rust/firmware/native/update_https.cpp embeds from OUTPUT. The source is Mozilla's CA list
for TLS servers as the pinned toolchain's certifi package carries it
(MPL-2.0); this script refuses any other copy. The format is ESP-IDF 5.5's:
one little-endian u32 offset per certificate, then for each, sorted by
subject, a u16 subject length, a u16 key length, the subject Name's DER and
its SubjectPublicKeyInfo's DER. Only those two parts of each root are kept.
"""
import argparse
import base64
import hashlib
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from freeink_sdk_routes import FIRMWARE_BUILD
from native_output import write_bytes

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / ".toolchain/platformio/penv/lib/python3.12/site-packages/certifi/cacert.pem"
SOURCE_SHA256 = "9cc2a774b5198dcff14d9be1e66091f538975d867ce029a96bce15a55dfd730f"  # certifi 2026.7.22
OUTPUT = ROOT / ".build" / FIRMWARE_BUILD / "crt/x509_crt_bundle"
MAX_CERTS = 200  # CONFIG_MBEDTLS_CERT_BUNDLE_MAX_CERTS in the pinned sdkconfig


def element(der: bytes, at: int) -> tuple[int, int, int]:
    """One DER element at `at`: its tag, where its content starts, and where it ends."""
    if at + 2 > len(der):
        raise ValueError("DER element runs past its end")
    tag, length, start = der[at], der[at + 1], at + 2
    if length & 0x80:
        count = length & 0x7f
        if not 1 <= count <= 4 or start + count > len(der):
            raise ValueError("DER length is not definite")
        length = int.from_bytes(der[start:start + count], "big")
        start += count
    if start + length > len(der):
        raise ValueError("DER element runs past its end")
    return tag, start, start + length


def subject_and_key(der: bytes) -> tuple[bytes, bytes]:
    """A certificate's raw subject Name and SubjectPublicKeyInfo elements."""
    tag, start, end = element(der, 0)
    if tag != 0x30 or end != len(der):
        raise ValueError("not one certificate")
    tag, at, _ = element(der, start)  # tbsCertificate
    if tag != 0x30:
        raise ValueError("certificate without tbsCertificate")
    fields = []
    while len(fields) < 7:
        tag, _, after = element(der, at)
        fields.append((tag, at, after))
        at = after
    if fields[0][0] == 0xa0:  # [0] version, then serial, signature, issuer, validity, subject, key
        fields = fields[1:]
    serial, signature, issuer, validity, subject, key = fields[:6]
    if [serial[0], signature[0], issuer[0], validity[0], subject[0], key[0]] != [0x02, 0x30, 0x30, 0x30, 0x30, 0x30]:
        raise ValueError("unexpected tbsCertificate layout")
    return der[subject[1]:subject[2]], der[key[1]:key[2]]


def certificates(pem: str) -> list[bytes]:
    found, body = [], None
    for line in pem.splitlines():
        line = line.strip()
        if line == "-----BEGIN CERTIFICATE-----":
            body = []
        elif line == "-----END CERTIFICATE-----":
            if body is None:
                raise ValueError("END without BEGIN")
            found.append(base64.b64decode("".join(body), validate=True))
            body = None
        elif body is not None:
            body.append(line)
    if body is not None:
        raise ValueError("certificate without END")
    return found


def bundle(pem: str) -> bytes:
    entries = sorted(subject_and_key(der) for der in certificates(pem))
    if not 0 < len(entries) <= MAX_CERTS:
        raise ValueError(f"{len(entries)} certificates; the bundle holds 1 to {MAX_CERTS}")
    offsets, body, at = [], b"", 4 * len(entries)
    for subject, key in entries:
        if len(subject) > 0xffff or len(key) > 0xffff:
            raise ValueError("certificate part too long for the bundle")
        offsets.append(at)
        entry = struct.pack("<HH", len(subject), len(key)) + subject + key
        body += entry
        at += len(entry)
    return struct.pack(f"<{len(offsets)}I", *offsets) + body


def pinned_pem() -> str:
    data = SOURCE.read_bytes()
    if hashlib.sha256(data).hexdigest() != SOURCE_SHA256:
        raise RuntimeError(f"CA list differs from the one tools/crt_bundle.py pins: {SOURCE}")
    return data.decode("ascii")


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    write_bytes(ROOT, OUTPUT, bundle(pinned_pem()))


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Certificate bundle stopped: {error}", file=sys.stderr)
        sys.exit(1)
