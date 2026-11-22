"""Authenticate the product SHA header used by retained native shims."""
import hashlib
from pathlib import Path

HEADER_SHA256 = 'cf55a4c9ba5e3e9ba5cd36f5b817a936c5d59992cd2a7b209a28c38747ab6aec'


def verify(root):
    # The unchanged alpha.16 native SHA provider for host shims, not a policy oracle.
    path = Path(root) / 'rust/tests/include/newt/host_sha256.h'
    if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != HEADER_SHA256:
        raise ValueError('Native SHA product header changed')
