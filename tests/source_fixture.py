"""Mutate verbatim native sources for controls, failing closed when an anchor drifts."""
from pathlib import Path
import shutil


def replace_once(source, anchor, replacement):
    if source.count(anchor) != 1:
        raise ValueError(f"Expected one firmware fixture anchor: {anchor!r}")
    return source.replace(anchor, replacement, 1)


def patched_firmware(directory, file, anchor, replacement):
    """A copy of rust/firmware/native with one line replaced: its headers include
    each other from their own directory, so a lone changed header would be bypassed."""
    shutil.rmtree(directory, ignore_errors=True)
    copy = Path(shutil.copytree(Path(__file__).resolve().parents[1] / 'rust/firmware/native', directory))
    (copy / file).write_text(replace_once((copy / file).read_text(), anchor, replacement))
    return copy
