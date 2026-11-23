"""The smallest PEP 517 backend for tools/cz_newt: standard library only, so
`pip install --no-index --no-deps tools/cz_newt` needs no network and no
setuptools. Its wheel holds cz_newt.py and the `commitizen.plugin` entry point."""
import base64
import hashlib
from pathlib import Path
import zipfile

NAME, VERSION = "cz_newt", "1"
DIST_INFO = f"{NAME}-{VERSION}.dist-info"
# .cz.toml's `name`: the package's path from the repository root.
ENTRY_POINT = "./tools/cz_newt"


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    files = {
        "cz_newt.py": (Path(__file__).resolve().parent / "cz_newt.py").read_bytes(),
        f"{DIST_INFO}/METADATA": f"Metadata-Version: 2.1\nName: {NAME}\nVersion: {VERSION}\n".encode(),
        f"{DIST_INFO}/WHEEL": b"Wheel-Version: 1.0\nGenerator: cz_newt_backend\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        f"{DIST_INFO}/entry_points.txt": f"[commitizen.plugin]\n{ENTRY_POINT} = cz_newt:NewtCz\n".encode(),
    }
    record = []
    name = f"{NAME}-{VERSION}-py3-none-any.whl"
    with zipfile.ZipFile(Path(wheel_directory) / name, "w") as wheel:
        for path, data in files.items():
            wheel.writestr(path, data)
            digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
            record.append(f"{path},sha256={digest},{len(data)}")
        wheel.writestr(f"{DIST_INFO}/RECORD", "\n".join([*record, f"{DIST_INFO}/RECORD,,"]) + "\n")
    return name
