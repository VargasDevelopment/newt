#!/usr/bin/env python3
"""Render the real firmware, built for the host, to PBM/PNG without connecting hardware."""
import argparse
from pathlib import Path

from host_common import ROOT, export_preview
from rust_preview import build, cli

ACTIONS = {"previous", "next", "activate", "back", "sleep", "wake"}


def action_list(value: str) -> str:
    parts = value.split(",")
    if any(part not in ACTIONS for part in parts) or len(parts) > 1024:
        raise argparse.ArgumentTypeError(
            "Use up to 1024 comma-separated previous,next,activate,back,sleep,wake actions")
    return value


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--screen", choices=("home", "device", "sleeping", "epub", "app", "library"),
                        default="home")
    parser.add_argument("--telemetry", choices=("unknown", "sample"), default="unknown",
                        help="sample is invented preview telemetry, never a hardware measurement")
    parser.add_argument("--actions", type=action_list)
    parser.add_argument("--reader-orientation", choices=("portrait", "landscape-ccw"),
                        help="Quick Start's book orientation; without it Quick Start is skipped, which keeps "
                             "books upright. System screens remain portrait")
    parser.add_argument("--reader-progress-bar", choices=("on", "off"), default="on",
                        help="selected slim bar in book reading only")
    parser.add_argument("--clock", metavar="HH:MM",
                        help="invented 24-hour clock text for Home and every system screen, never the hardware clock")
    parser.add_argument("--other-firmware", choices=("crosspoint", "crossink", "stock", "missing", "failing", "unsaved",
                                                     "unconfirmed"), default="crossink",
                        help="the other app slot for Settings' firmware switch; the preview ends at a restart into it")
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts",
                        help="directory for PBM and lossless PNG files")
    args = parser.parse_args()
    binary = cli(build())
    export_preview(binary, "preview", args.screen, args.telemetry, args.actions, args.output,
                   args.reader_orientation, args.reader_progress_bar, args.clock,
                   other_firmware=args.other_firmware)


if __name__ == "__main__":
    main()
