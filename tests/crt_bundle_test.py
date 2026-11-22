"""tools/crt_bundle.py: the bundle update mode's HTTPS checks servers against
(docs/088-ota-wifi-install.md). The pinned CA list becomes ESP-IDF 5.5's
bundle layout, every root's subject and key whole and sorted by subject, and
any other list or a damaged certificate is refused."""
import importlib.util
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("crt_bundle", ROOT / "tools/crt_bundle.py")
crt_bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(crt_bundle)


def entries(data: bytes) -> list[tuple[bytes, bytes]]:
    """Read a bundle back as esp_crt_bundle does: the offset table, then each entry in turn."""
    count = struct.unpack_from("<I", data, 0)[0] // 4
    assert 0 < count <= crt_bundle.MAX_CERTS, "the offset table names 1 to MAX_CERTS entries"
    offsets = struct.unpack_from(f"<{count}I", data, 0)
    found, at = [], 4 * count
    for offset in offsets:
        assert offset == at, "each entry follows the last with no gap"
        name, key = struct.unpack_from("<HH", data, at)
        found.append((data[at + 4:at + 4 + name], data[at + 4 + name:at + 4 + name + key]))
        at += 4 + name + key
    assert at == len(data), "the last entry ends the bundle"
    return found


def main():
    pem = crt_bundle.pinned_pem()
    certificates = crt_bundle.certificates(pem)
    read = entries(crt_bundle.bundle(pem))
    assert len(read) == len(certificates), "every root is kept"
    assert [name for name, _ in read] == sorted(name for name, _ in read), "sorted by subject for the binary search"
    for name, key in read:
        assert crt_bundle.element(name, 0) == (0x30, 2 if name[1] < 0x80 else 2 + (name[1] & 0x7f), len(name)), \
            "a subject is one whole DER Name"
        assert key[0] == 0x30 and crt_bundle.element(key, 0)[2] == len(key), "a key is one whole SubjectPublicKeyInfo"
    # Each root's parts are its own: a root issues itself, so its subject is
    # also the issuer field just before its validity.
    der = certificates[0]
    subject, key = crt_bundle.subject_and_key(der)
    assert der.count(subject) >= 2 and der.count(key) == 1, "subject and key come from the certificate"

    # Refusals: another list, a damaged certificate, an empty list.
    original = crt_bundle.SOURCE_SHA256
    crt_bundle.SOURCE_SHA256 = "0" * 64
    try:
        crt_bundle.pinned_pem()
        raise AssertionError("another CA list is refused")
    except RuntimeError:
        pass
    finally:
        crt_bundle.SOURCE_SHA256 = original
    for broken in (der[:-1], der[:40], b"\x30\x03\x02\x01\x01"):
        try:
            crt_bundle.subject_and_key(broken)
            raise AssertionError("a damaged certificate is refused")
        except ValueError:
            pass
    try:
        crt_bundle.bundle("")
        raise AssertionError("an empty list is refused")
    except ValueError:
        pass
    print("PASS crt bundle")


if __name__ == "__main__":
    main()
