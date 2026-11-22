"""Real provider SHA-256 vectors; optional existing OpenSSL API exercise on Mac.

No acquisition, firmware, fake digest success, private content or hardware.
An explicit Mac prefix exercises that installed library, not the CI library pin.
"""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import host_common
from host_objects import compiler_environment
from native_output import owned_directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--openssl-prefix", type=Path,
                        help="Exercise an already installed OpenSSL provider; never installs")
    args = parser.parse_args()
    flags = host_common.crypto_flags()
    openssl = sys.platform.startswith("linux")
    if args.openssl_prefix:
        prefix = args.openssl_prefix.resolve(strict=True)
        library = prefix / "lib/libcrypto.a"
        if not (prefix / "include/openssl/evp.h").is_file() or not library.is_file():
            parser.error("Existing OpenSSL headers/static library missing")
        flags = ["-DNEWT_HOST_SHA256_TEST_OPENSSL=1", "-I", str(prefix / "include"), str(library)]
        if sys.platform.startswith("linux"):
            flags += ["-pthread", "-ldl"]
        openssl = True
    build = owned_directory(ROOT, ROOT / ".build/host-sha256-test", create=True)
    environment = compiler_environment(ROOT)
    # Only the reviewed builtin/default provider is needed for SHA-256. The
    # unavailable-provider control below deliberately loads the builtin null one.
    environment["OPENSSL_CONF"] = "/dev/null"
    with tempfile.TemporaryDirectory(prefix="vectors-", dir=build) as name:
        binary = Path(name) / "vectors"
        subprocess.run([host_common.COMPILER, "-std=c++17", "-Wall", "-Wextra", "-Werror", "-pedantic",
                        *host_common.SANITIZERS, "-I", str(ROOT / "rust/tests/include"),
                        str(ROOT / "tests/host_sha256_test.cpp"), *flags, "-o", str(binary)],
                       check=True, cwd=ROOT, env=environment)
        subprocess.run([str(binary)], check=True, cwd=ROOT, env=environment)
        if openssl:
            subprocess.run([str(binary), "missing-provider"], check=True, cwd=ROOT, env=environment)
            # A fresh process must ignore both malformed ambient config and a
            # valid config that would activate only the null provider. The latter
            # would make real SHA initialization fail if config loading leaked in.
            for config in ("[intentionally unfinished section\n",
                           "openssl_conf = init\n[init]\nproviders = providers\n"
                           "[providers]\nnull = null_provider\n[null_provider]\nactivate = 1\n"):
                invalid = Path(name) / "ambient.cnf"
                invalid.write_text(config)
                ambient = {**environment, "OPENSSL_CONF": str(invalid)}
                subprocess.run([str(binary)], check=True, cwd=ROOT, env=ambient)


if __name__ == "__main__":
    main()
