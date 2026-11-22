"""Parent-run only: compare already-built transports. Does not build/acquire."""
import subprocess
import sys
from corpus import corpus, rejection_corpus


def run(executable, data):
    return subprocess.run([executable], input=data.encode(), capture_output=True,
                          timeout=30, check=False)


def compare(cpp, rust):
    for case in corpus():
        left, right = (run(exe, case["input"]) for exe in (cpp, rust))
        if left.returncode or right.returncode or left.stdout != right.stdout:
            raise AssertionError((case["name"], left.returncode, right.returncode,
                                  left.stdout[:4000], right.stdout[:4000],
                                  left.stderr[:1000], right.stderr[:1000]))
        lines = right.stdout.decode("ascii").splitlines()
        for expected in case["expected_lines"]:
            if expected not in lines:
                raise AssertionError((case["name"], "independent expectation", expected))
    for data in rejection_corpus():
        for exe in (cpp, rust):
            result = run(exe, data)
            if result.returncode != 2:
                raise AssertionError(("strict rejection", exe, repr(data), result.returncode))


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: compare.py CPP_TRANSPORT RUST_TRANSPORT")
    compare(*sys.argv[1:])
