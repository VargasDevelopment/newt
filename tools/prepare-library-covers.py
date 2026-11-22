"""Prepare optional, regenerable NLC2 thumbnails without changing book bytes.

Automatic nonrecursive EPUB scan; explicit local --books/--output only. Use an
ignored host output such as .build/library-covers. This tool never writes a card.
NLC2: <4sHHQQI header (144x188, source size, FAT-compatible 2-second mtime,
payload CRC32) followed by 3384 bytes of MSB-first 1bpp, 1=black. The firmware's
automatic producer also writes header-only no-art records, whose last field
names its decoder route; readers show the placeholder for them and this tool
replaces them like any NLC2 cache. No private cover, book, or cache belongs in
Git. Unsupported/missing covers are fallbacks.
"""
import argparse
import os
from pathlib import Path
import posixpath
import platform
import stat
import struct
import subprocess
import tempfile
from urllib.parse import unquote
from xml.etree import ElementTree as ET
import zipfile
import zlib

MAX_BOOK = 64 * 1024 * 1024  # include/newt/file_book_source.h
MAX_MEMBER = 4 * 1024 * 1024  # include/newt/epub_admission.h
MAX_XML = 128 * 1024
HEADER = struct.Struct('<4sHHQQI')
PAYLOAD_BYTES = 3384


def admitted_name(name):
    try:
        length = len(name.encode('utf-8'))
    except UnicodeEncodeError:
        return False
    return (6 <= length <= 95 and not name.startswith('.')
            and name[-5:].lower() == '.epub' and '/' not in name and '\\' not in name
            and all(ord(c) >= 32 and not 127 <= ord(c) <= 159 for c in name))


def member_path(value, base=''):
    value = unquote(value, errors='strict')
    if not value or value.startswith('/') or any(c in value for c in '\\:\0?#'):
        raise ValueError('Unsafe EPUB path')
    result = posixpath.normpath(posixpath.join(base, value))
    if result in ('.', '..') or result.startswith('../') or len(result.encode()) > 511:
        raise ValueError('Unsafe EPUB path')
    return result


def preflight(source):
    """Bound ZipFile metadata allocation; reject ZIP64/multidisk and comments.

    Cover preparation is optional: conservatively rejecting comments avoids a
    second ambiguous end-record authority inside untrusted ZIP comment bytes.
    """
    source.seek(0, os.SEEK_END)
    size = source.tell()
    if not 22 <= size <= MAX_BOOK:
        raise ValueError('Unsupported source size')
    source.seek(size - 22)
    end = source.read(22)
    magic, disk, cd_disk, disk_count, count, directory_size, offset, comment = struct.unpack('<4s4H2IH', end)
    if (magic != b'PK\x05\x06' or disk or cd_disk or comment or disk_count != count
            or not 1 <= count <= 1024 or directory_size > 1024 * 1024
            or offset + directory_size != size - 22):
        raise ValueError('Unsupported ZIP directory')
    if size >= 42:
        source.seek(size - 42)
        if source.read(4) == b'PK\x06\x07':
            raise ValueError('ZIP64 unsupported')
    source.seek(offset)
    remaining = directory_size
    for _ in range(count):
        raw = source.read(46)
        if len(raw) != 46 or raw[:4] != b'PK\x01\x02':
            raise ValueError('Invalid ZIP directory')
        name, extra, note = struct.unpack_from('<3H', raw, 28)
        record = 46 + name + extra + note
        if not 1 <= name <= 511 or record > remaining:
            raise ValueError('Invalid ZIP member bounds')
        source.seek(record - 46, os.SEEK_CUR)
        remaining -= record
    if remaining:
        raise ValueError('Invalid ZIP directory size')


def read_member(archive, name, limit):
    info = archive.getinfo(name)
    if not 0 < info.file_size <= limit:
        raise ValueError('Member exceeds bound')
    with archive.open(info) as source:
        raw = source.read(limit + 1)
        if len(raw) != info.file_size or len(raw) > limit or source.read(1):
            raise ValueError('Invalid expanded member length')
    return raw


def xml_member(archive, name):
    raw = read_member(archive, name, MAX_XML)
    # ElementTree supports internal entities; disallow declarations entirely.
    if b'<!DOCTYPE' in raw.upper() or b'<!ENTITY' in raw.upper() or b'\0' in raw:
        raise ValueError('Unsupported XML declaration/encoding')
    return ET.fromstring(raw)


def cover_bytes(source):
    preflight(source)
    with zipfile.ZipFile(source) as archive:
        seen, total = set(), 0
        for info in archive.infolist():
            path = member_path(info.filename)
            mode = info.external_attr >> 16
            if (path != info.filename.rstrip('/') or path in seen or info.flag_bits & 0x41
                    or stat.S_ISLNK(mode) or info.compress_type not in (0, 8)
                    or info.file_size > MAX_MEMBER or info.compress_size > MAX_BOOK):
                raise ValueError('Unsupported ZIP member')
            seen.add(path)
            total += info.file_size
        if total > 64 * 1024 * 1024:
            raise ValueError('Expanded EPUB too large')
        container = xml_member(archive, 'META-INF/container.xml')
        roots = [n for n in container.iter() if n.tag.rsplit('}', 1)[-1] == 'rootfile']
        if len(roots) != 1:
            raise ValueError('Ambiguous package')
        package_path = member_path(roots[0].get('full-path', ''))
        package = xml_member(archive, package_path)
        items = [n for n in package.iter() if n.tag.rsplit('}', 1)[-1] == 'item']
        cover_ids = [n.get('content') for n in package.iter()
                     if n.tag.rsplit('}', 1)[-1] == 'meta' and n.get('name') == 'cover']
        choices = [n for n in items if 'cover-image' in n.get('properties', '').split()]
        if not choices:
            choices = [n for n in items if n.get('id') in cover_ids]
        if len(choices) != 1 or choices[0].get('media-type') not in ('image/jpeg', 'image/png'):
            raise ValueError('No unambiguous supported cover')
        name = member_path(choices[0].get('href', ''), posixpath.dirname(package_path))
        return read_member(archive, name, MAX_MEMBER)


def compile_helper(destination):
    if platform.system() != 'Darwin':
        raise RuntimeError('General cover decoding requires macOS ImageIO; run tools/check-mac-covers.py for required cover evidence')
    root = Path(__file__).resolve().parent
    env = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(destination.parent / 'clang-cache'),
               SWIFT_MODULECACHE_PATH=str(destination.parent / 'swift-cache'))
    subprocess.run(['/usr/bin/swiftc', str(root / 'library-cover-image.swift'), '-o', str(destination)],
                   check=True, env=env, timeout=120, stdout=subprocess.DEVNULL)


def remove_previous(destination):
    """Only delete a regular, identifiable NLC2 cache at this exact book name."""
    try:
        fd = os.open(destination, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as source:
            if (stat.S_ISREG(os.fstat(source.fileno()).st_mode)
                    and source.read(4) == b'NLC2'):
                destination.unlink()
    except (FileNotFoundError, OSError):
        pass


def prepare(books, output, helper, work):
    if books.is_symlink() or output.is_symlink() or not books.is_dir():
        raise ValueError('Expected local directories without symlinks')
    books, output = books.resolve(), output.resolve()
    if output == books or output.is_relative_to(books) or books.is_relative_to(output):
        raise ValueError('Book and cache directories must be separate')
    output.mkdir(parents=True, exist_ok=True)
    prepared = skipped = 0
    # Sources remain readonly; caches are generated one book at a time.
    with os.scandir(books) as entries:
        for entry in entries:
            if not admitted_name(entry.name) or not entry.is_file(follow_symlinks=False):
                continue
            destination = output / (entry.name + '.nlc')
            if destination.is_symlink():
                skipped += 1
                continue
            try:
                fd = os.open(entry.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(fd, 'rb') as source:
                    before = os.fstat(source.fileno())
                    if not stat.S_ISREG(before.st_mode):
                        raise ValueError('Expected regular source')
                    raw = cover_bytes(source)
                    image, bitmap = work / 'cover-image', work / 'cover-bitmap'
                    image.write_bytes(raw)
                    command = list(helper) if isinstance(helper, (list, tuple)) else [str(helper)]
                    subprocess.run([*command, str(image), str(bitmap)], check=True,
                                   timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    payload = bitmap.read_bytes()
                    after = os.fstat(source.fileno())
                    current = os.stat(entry.path, follow_symlinks=False)
                    if ((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) !=
                            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                            or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) !=
                            (current.st_dev, current.st_ino, current.st_size, current.st_mtime_ns)
                            or len(payload) != PAYLOAD_BYTES or before.st_mtime < 0):
                        raise ValueError('Source changed or invalid payload')
                header = HEADER.pack(b'NLC2', 144, 188, before.st_size,
                                     before.st_mtime_ns // 2_000_000_000 * 2, zlib.crc32(payload))
                fd, temporary = tempfile.mkstemp(prefix='.nlc-', dir=output)
                try:
                    with os.fdopen(fd, 'wb') as stream:
                        stream.write(header + payload)
                    os.replace(temporary, destination)
                finally:
                    Path(temporary).unlink(missing_ok=True)
                prepared += 1
            except (ValueError, OSError, KeyError, EOFError, struct.error, zlib.error,
                    ET.ParseError, zipfile.BadZipFile,
                    subprocess.SubprocessError, RuntimeError, NotImplementedError):
                remove_previous(destination)
                skipped += 1
    return prepared, skipped


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--books', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='newt-library-covers-') as directory:
        work = Path(directory)
        helper = work / 'image-helper'
        compile_helper(helper)
        prepared, skipped = prepare(args.books, args.output, helper, work)
    print(f'Prepared {prepared} bounded thumbnails; {skipped} cover fallbacks. Sources unchanged.')


if __name__ == '__main__':
    main()
