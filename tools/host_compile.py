#!/usr/bin/env python3
"""Compile one standalone gate host test in its own process.

The gate runs this as a phase, so a parallel gate can buffer everything the
compile prints, including compiler diagnostics. It is compile_host itself:
same flags, object cache and output path (.build/<name>).
"""
import argparse
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from host_common import compile_host


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('main', type=Path)
    parser.add_argument('name')
    args = parser.parse_args()
    compile_host(args.main, args.name)


if __name__ == '__main__':
    main()
