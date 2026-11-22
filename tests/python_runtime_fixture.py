"""Small mutable Python installations for resource identity fixtures."""
from unittest.mock import patch

import python_runtime


def install_runtime_fixture(fixture):
    roots = {}
    for role in ("driver", "procedure"):
        root = fixture.root / ".toolchain" / (role + "-runtime")
        for name, content in (("bin/python", b"runtime executable"),
                              ("lib/python/json/__init__.py", b"stdlib source"),
                              ("lib/python/json/__pycache__/__init__.pyc", b"stdlib bytecode"),
                              ("lib/python/lib-dynload/_json.so", b"stdlib extension"),
                              ("Python", b"shared Python runtime")):
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        roots[role] = root
    fixture.runtime_roots = roots

    def description(executable=None):
        root = roots["driver" if executable is not None else "procedure"]
        return {"executable": str(executable or root / "bin/python"), "base_prefix": str(root),
                "stdlib": str(root / "lib/python"), "platstdlib": str(root / "lib/python"),
                "version": "fixture Python", "cache_tag": "fixture", "optimize": 0}

    fixture.runtime_description = description
    fixture.runtime_description_patch = patch.object(python_runtime, "runtime_description", side_effect=description)
    fixture.runtime_description_patch.start()
    fixture.addCleanup(fixture.runtime_description_patch.stop)
