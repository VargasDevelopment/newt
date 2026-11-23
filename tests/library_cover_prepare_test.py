"""Authored inputs only. --portable-only tests contracts and four locked UI PNGs.

The synthetic helper supplies fixed scanlines for EPUB/cache contract tests;
it provides no image decoding evidence. Default execution requires Mac ImageIO.
"""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile
import zlib
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('prepare_covers', ROOT / 'tools/prepare-library-covers.py')
COVERS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COVERS)
PORTABLE_ONLY = False
PORTABLE_CASES = (
    'test_readonly_sources_bytes_identity_and_mtime_are_preserved',
    'test_sidecar_header_and_crc_bind_exact_source_and_payload',
    'test_nonrecursive_admitted_names_symlink_source_and_output_rejection',
    'test_book_and_output_directories_must_be_separate',
    'test_entity_xml_large_xml_ambiguous_cover_and_pathescape_fallbacks',
    'test_zip_metadata_count_expansion_encryption_symlink_and_corruption_bounds',
    'test_source_size_is_bounded',
    'test_timestamp_rounds_down_without_float_boundary_rounding',
    'test_corrupt_deflate_is_one_fallback_and_later_book_still_prepares',
    'test_helper_failures_and_wrong_payload_remove_old_cache',
    'test_authored_png_corpus_has_locked_payloads_and_rejects_unknown_bytes',
    'test_authored_fixtures_prepare_all_four_or_fail',
    'test_general_decoder_explicitly_rejects_linux',
    'test_required_mac_gate_checks_only_imageio_for_exact_source',
)


def fixture_tools():
    sys.path.insert(0, str(ROOT / 'tools'))
    modules = []
    for name in ('library-cover-fixture', 'library-redesign-preview'):
        spec = importlib.util.spec_from_file_location(name, ROOT / 'tools' / (name + '.py'))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        modules.append(module)
    return modules


def png(width=72, height=94, top=0, bottom=255):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    pixels = b''.join(b'\0' + bytes([top if y < height // 2 else bottom]) * width
                      for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>2I5B', width, height, 8, 0, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))


def epub(path, image=None, properties=True, extra=None, metadata=None, href='cover.png',
         media='image/png', compression=zipfile.ZIP_DEFLATED):
    mark = 'properties="cover-image"' if properties else ''
    cover_meta = '' if properties else '<meta name="cover" content="front"/>'
    if metadata is None:
        metadata = (f'<package xmlns="http://www.idpf.org/2007/opf"><metadata>{cover_meta}</metadata>'
                    f'<manifest><item id="front" href="{href}" media-type="{media}" {mark}/></manifest></package>')
    with zipfile.ZipFile(path, 'w', compression=compression) as archive:
        archive.writestr('META-INF/container.xml',
                         '<container><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></container>')
        archive.writestr('OPS/package.opf', metadata)
        if image is not None:
            archive.writestr('OPS/cover.png', image)
        for name, value in (extra or {}).items():
            archive.writestr(name, value)


class CoverPreparationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.shared = tempfile.TemporaryDirectory(prefix='newt-authored-cover-test-')
        if PORTABLE_ONLY:
            # Explicit protocol double: no PNG/JPEG decoding or parity claim.
            stub = Path(cls.shared.name) / 'synthetic-bitmap.py'
            stub.write_text('import sys\nfrom pathlib import Path\nPath(sys.argv[2]).write_bytes(bytes(3384))\n')
            cls.helper = [sys.executable, '-I', '-B', str(stub)]
            return
        cls.helper = Path(cls.shared.name) / 'image-helper'
        COVERS.compile_helper(cls.helper)
        # System ImageIO encodes an authored PNG as JPEG; no fixture/download dependency.
        source = Path(cls.shared.name) / 'authored.png'
        source.write_bytes(png())
        script = Path(cls.shared.name) / 'authored-jpeg.swift'
        script.write_text('''import Foundation
import ImageIO
let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL, nil)!
let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
let output = CGImageDestinationCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[2]) as CFURL, "public.jpeg" as CFString, 1, nil)!
CGImageDestinationAddImage(output, image, nil)
precondition(CGImageDestinationFinalize(output))
''')
        cls.jpeg_path = Path(cls.shared.name) / 'authored.jpg'
        env = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(Path(cls.shared.name) / 'clang-cache'),
                   SWIFT_MODULECACHE_PATH=str(Path(cls.shared.name) / 'swift-cache'))
        subprocess.run(['/usr/bin/swift', str(script), str(source), str(cls.jpeg_path)],
                       check=True, env=env, timeout=120)

    @classmethod
    def tearDownClass(cls):
        cls.shared.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='newt-authored-cover-test-')
        self.root = Path(self.temp.name)
        self.books, self.output, self.work = (self.root / name for name in ('books', 'covers', 'work'))
        self.books.mkdir()
        self.work.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def run_prepare(self):
        return COVERS.prepare(self.books, self.output, self.helper, self.work)

    def test_automatic_png_jpeg_and_legacy_opf_exact_contract(self):
        for name, image, properties, media in (
                ('PNG.epub', png(), True, 'image/png'),
                ('Legacy.EPUB', png(), False, 'image/png'),
                ('JPEG.epub', self.jpeg_path.read_bytes(), True, 'image/jpeg')):
            epub(self.books / name, image, properties=properties, media=media)
            os.utime(self.books / name, ns=(1_800_000_001_123_456_789, 1_800_000_001_123_456_789))
        self.assertEqual(self.run_prepare(), (3, 0))
        for source in self.books.iterdir():
            raw = (self.output / (source.name + '.nlc')).read_bytes()
            self.assertEqual(len(raw), 3412)
            magic, width, height, size, timestamp, crc = COVERS.HEADER.unpack(raw[:28])
            self.assertEqual((magic, width, height, size, timestamp),
                             (b'NLC2', 144, 188, source.stat().st_size, 1_800_000_000))
            self.assertEqual(crc, zlib.crc32(raw[28:]))
            self.assertGreater(sum(b.bit_count() for b in raw[28:]), 10000)
            self.assertLess(sum(b.bit_count() for b in raw[28:]), 15000)

    def test_orientation_and_white_letterboxing(self):
        epub(self.books / 'Portrait.epub', png(width=36, height=94))
        self.assertEqual(self.run_prepare(), (1, 0))
        payload = (self.output / 'Portrait.epub.nlc').read_bytes()[28:]
        self.assertEqual(payload[0:4], bytes(4))
        self.assertEqual(payload[8], 255)  # black top half remains at top
        self.assertEqual(payload[150 * 18 + 8], 0)

    def test_readonly_sources_bytes_identity_and_mtime_are_preserved(self):
        source = self.books / 'Untouched.epub'
        epub(source, png())
        source.chmod(0o444)
        before = source.stat()
        digest = hashlib.sha256(source.read_bytes()).digest()
        self.assertEqual(self.run_prepare(), (1, 0))
        after = source.stat()
        self.assertEqual((before.st_ino, before.st_size, before.st_mtime_ns, before.st_mode),
                         (after.st_ino, after.st_size, after.st_mtime_ns, after.st_mode))
        self.assertEqual(digest, hashlib.sha256(source.read_bytes()).digest())
        self.assertEqual(self.run_prepare(), (1, 0))

    def test_sidecar_header_and_crc_bind_exact_source_and_payload(self):
        source = self.books / 'Contract.epub'
        epub(source, png())
        os.utime(source, ns=(1_800_000_001_123_456_789, 1_800_000_001_123_456_789))
        self.assertEqual(self.run_prepare(), (1, 0))
        raw = (self.output / 'Contract.epub.nlc').read_bytes()
        self.assertEqual(len(raw), COVERS.HEADER.size + COVERS.PAYLOAD_BYTES)
        self.assertEqual(COVERS.HEADER.unpack(raw[:COVERS.HEADER.size]),
                         (b'NLC2', 144, 188, source.stat().st_size, 1_800_000_000,
                          zlib.crc32(raw[COVERS.HEADER.size:])))

    def test_missing_corrupt_and_oversized_cover_are_fallbacks_and_remove_old_cache(self):
        self.output.mkdir()
        for name, image in (('Missing', None), ('Corrupt', b'bad png'), ('Large', bytes(COVERS.MAX_MEMBER + 1))):
            epub(self.books / (name + '.epub'), image)
            (self.output / (name + '.epub.nlc')).write_bytes(b'NLC2stale')
        (self.output / 'unrelated.txt').write_text('keep')
        self.assertEqual(self.run_prepare(), (0, 3))
        self.assertEqual(list(self.output.iterdir()), [self.output / 'unrelated.txt'])

    def test_nonrecursive_admitted_names_symlink_source_and_output_rejection(self):
        nested = self.books / 'nested'
        nested.mkdir()
        epub(nested / 'Nested.epub', png())
        for name in ('.Hidden.epub', 'x' * 91 + '.epub', 'bad\x7f.epub', 'wrong.txt'):
            epub(self.books / name, png())
        outside = self.root / 'outside.epub'
        epub(outside, png())
        (self.books / 'Link.epub').symlink_to(outside)
        epub(self.books / 'Valid.epub', png())
        self.output.mkdir()
        sentinel = self.root / 'sentinel'
        sentinel.write_text('keep')
        (self.output / 'Valid.epub.nlc').symlink_to(sentinel)
        self.assertEqual(self.run_prepare(), (0, 1))
        self.assertEqual(sentinel.read_text(), 'keep')
        self.assertTrue((self.output / 'Valid.epub.nlc').is_symlink())

    def test_book_and_output_directories_must_be_separate(self):
        with self.assertRaises(ValueError):
            COVERS.prepare(self.books, self.books / 'cache', self.helper, self.work)
        link = self.root / 'link'
        link.symlink_to(self.books, target_is_directory=True)
        with self.assertRaises(ValueError):
            COVERS.prepare(link, self.output, self.helper, self.work)

    def test_entity_xml_large_xml_ambiguous_cover_and_pathescape_fallbacks(self):
        epub(self.books / 'Entity.epub', png(), metadata='<!DOCTYPE x [<!ENTITY a "bad">]><x>&a;</x>')
        epub(self.books / 'XML.epub', png(), metadata='<x>' + 'a' * COVERS.MAX_XML + '</x>')
        epub(self.books / 'Escape.epub', png(), href='../../outside.png')
        epub(self.books / 'Absolute.epub', png(), href='/cover.png')
        epub(self.books / 'URL.epub', png(), href='https://example.invalid/cover.png')
        epub(self.books / 'Duplicate.epub', png(), metadata='<package><item href="cover.png" '
             'media-type="image/png" properties="cover-image"/><item href="cover.png" '
             'media-type="image/png" properties="cover-image"/></package>')
        self.assertEqual(self.run_prepare(), (0, 6))

    def test_zip_metadata_count_expansion_encryption_symlink_and_corruption_bounds(self):
        epub(self.books / 'Count.epub', png(), extra={f'x/{n}': b'' for n in range(1023)})
        epub(self.books / 'Path.epub', png(), extra={'../escape': b'a'})
        epub(self.books / 'Unsupported.epub', png(), compression=zipfile.ZIP_BZIP2)
        symlink = self.books / 'Symlink.epub'
        epub(symlink, png())
        with zipfile.ZipFile(symlink, 'a') as archive:
            info = zipfile.ZipInfo('link')
            info.external_attr = (0o120777 << 16)
            archive.writestr(info, b'cover.png')
        source = self.books / 'Encrypted.epub'
        epub(source, png())
        raw = bytearray(source.read_bytes())
        at = raw.index(b'PK\x01\x02')
        raw[at + 8] |= 1
        source.write_bytes(raw)
        (self.books / 'NotZip.epub').write_bytes(b'not a zip')
        self.assertEqual(self.run_prepare(), (0, 6))

    def test_source_size_is_bounded(self):
        with (self.books / 'Huge.epub').open('wb') as stream:
            stream.truncate(COVERS.MAX_BOOK + 1)
        self.assertEqual(self.run_prepare(), (0, 1))

    def test_image_dimensions_are_bounded(self):
        image = bytearray(png())
        image[16:20] = struct.pack('>I', 100000)
        image[29:33] = struct.pack('>I', zlib.crc32(image[12:29]))
        epub(self.books / 'Dimensions.epub', bytes(image))
        self.assertEqual(self.run_prepare(), (0, 1))

    def test_existing_cache_replaced_after_changed_source(self):
        source = self.books / 'Change.epub'
        epub(source, png(top=0, bottom=0))
        self.assertEqual(self.run_prepare(), (1, 0))
        old = (self.output / 'Change.epub.nlc').read_bytes()
        epub(source, png(top=255, bottom=255))
        self.assertEqual(self.run_prepare(), (1, 0))
        new = (self.output / 'Change.epub.nlc').read_bytes()
        self.assertNotEqual(old, new)
        self.assertEqual(new[28:], bytes(3384))

    def test_timestamp_rounds_down_without_float_boundary_rounding(self):
        source = self.books / 'Time.epub'
        epub(source, png())
        os.utime(source, ns=(1_800_000_001_999_999_999, 1_800_000_001_999_999_999))
        self.assertEqual(self.run_prepare(), (1, 0))
        raw = (self.output / 'Time.epub.nlc').read_bytes()
        self.assertEqual(COVERS.HEADER.unpack(raw[:28])[4], 1_800_000_000)
        self.assertFalse(COVERS.admitted_name('invalid\udcff.epub'))

    def test_corrupt_deflate_is_one_fallback_and_later_book_still_prepares(self):
        source = self.books / 'Broken.epub'
        epub(source, png())
        with zipfile.ZipFile(source) as archive:
            offset = archive.getinfo('OPS/cover.png').header_offset
        raw = bytearray(source.read_bytes())
        name, extra = struct.unpack_from('<HH', raw, offset + 26)
        start = offset + 30 + name + extra
        raw[start:start + 4] = b'\xff\xff\xff\xff'
        source.write_bytes(raw)
        epub(self.books / 'Valid.epub', png())
        self.assertEqual(self.run_prepare(), (1, 1))

    def test_helper_failures_and_wrong_payload_remove_old_cache(self):
        epub(self.books / 'Helper.epub', png())
        self.output.mkdir()
        destination = self.output / 'Helper.epub.nlc'
        for failure in (subprocess.CalledProcessError(1, ['synthetic-helper']), None):
            destination.write_bytes(b'NLC2old-cache')
            def run(command, **kwargs):
                if failure:
                    raise failure
                Path(command[-1]).write_bytes(bytes(COVERS.PAYLOAD_BYTES - 1))
            with patch.object(COVERS.subprocess, 'run', side_effect=run):
                self.assertEqual(self.run_prepare(), (0, 1))
            self.assertFalse(destination.exists())

    def test_authored_png_corpus_has_locked_payloads_and_rejects_unknown_bytes(self):
        fixture, art = fixture_tools()
        self.assertEqual(len(fixture.CORPUS), 4)
        for style in range(4):
            image = (ROOT / f'tests/fixtures/library-cover-images/style-{style}.png').read_bytes()
            payload = fixture.bitmap(image)
            self.assertEqual(len(payload), COVERS.PAYLOAD_BYTES)
            self.assertEqual(hashlib.sha256(payload).hexdigest(),
                             fixture.CORPUS[hashlib.sha256(image).hexdigest()])
            for unknown in (image[:-1], image + b'extra', png(), b'jpeg', bytes(4097)):
                with self.assertRaisesRegex(ValueError, 'four exact authored'):
                    fixture.bitmap(unknown)

    def test_authored_fixtures_prepare_all_four_or_fail(self):
        _, art = fixture_tools()
        art.authored_books(self.books)
        art.prepare_authored_covers(self.books, self.output)
        self.assertEqual(len(list(self.output.glob('*.nlc'))), 4)
        epub(self.books / 'Unknown.epub', png())
        with self.assertRaisesRegex(RuntimeError, 'four exact authored'):
            art.prepare_authored_covers(self.books, self.output)

    def test_general_decoder_explicitly_rejects_linux(self):
        with patch.object(COVERS.platform, 'system', return_value='Linux'), \
             patch.object(COVERS.subprocess, 'run') as compiler:
            with self.assertRaisesRegex(RuntimeError, 'requires macOS ImageIO'):
                COVERS.compile_helper(self.work / 'helper')
            compiler.assert_not_called()

    def test_four_authored_payloads_match_actual_imageio(self):
        fixture, _ = fixture_tools()
        image, bitmap = self.work / 'authored.png', self.work / 'native-bitmap'
        for style in range(4):
            source = (ROOT / f'tests/fixtures/library-cover-images/style-{style}.png').read_bytes()
            self.assertEqual(fixture.cover_art(style), source)
            image.write_bytes(source)
            subprocess.run([str(self.helper), str(image), str(bitmap)], check=True)
            self.assertEqual(bitmap.read_bytes(), fixture.bitmap(image.read_bytes()))

    @patch('sys.stdout', new_callable=io.StringIO)
    def test_required_mac_gate_checks_only_imageio_for_exact_source(self, stdout):
        sys.path.insert(0, str(ROOT / 'tools'))
        spec = importlib.util.spec_from_file_location('mac_cover_gate', ROOT / 'tools/check-mac-covers.py')
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        report = self.root / '.build/mac-covers/report.json'
        report.parent.mkdir(parents=True)
        report.write_text('{"passed":true}')
        expected = 'a' * 40
        with patch.object(gate.platform, 'system', return_value='Linux'), \
             patch.object(gate, 'ROOT', self.root.resolve()), \
             patch.object(sys, 'argv', ['check-mac-covers.py', '--expected-commit', expected]), \
             patch.object(gate.subprocess, 'run') as run:
            with self.assertRaises(SystemExit) as error:
                gate.main()
            self.assertEqual(str(error.exception), 'The host cover helper gate needs macOS ImageIO')
            run.assert_not_called()
        self.assertFalse(report.exists())
        with patch.object(gate.platform, 'system', return_value='Darwin'), \
             patch.object(gate, 'committed_source', side_effect=[{'sha': 'a'}, {'sha': 'b'}]), \
             patch.object(gate.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0)):
            with self.assertRaisesRegex(RuntimeError, 'source changed'):
                gate.run_gate(self.root.resolve(), expected)
        self.assertFalse(report.exists())
        source = {'sha': expected}
        root = self.root.resolve()
        with patch.object(gate.platform, 'system', return_value='Darwin'), \
             patch.object(gate.platform, 'platform', return_value='Darwin test'), \
             patch.object(gate, 'committed_source', return_value=source), \
             patch.object(gate.subprocess, 'run') as run:
            gate.run_gate(root, expected)
            run.assert_called_once_with(
                [sys.executable, '-B', str(root / 'tests/library_cover_prepare_test.py')],
                cwd=root, check=True)
        receipt = json.loads(report.read_text())
        self.assertEqual(receipt['phases'], ['mac-imageio-cover-tests'])
        self.assertEqual(receipt['source'], source)
        self.assertIn(f'PASS Mac ImageIO cover helper gate for {expected}\n', stdout.getvalue())
        failure = subprocess.CalledProcessError(1, 'library_cover_prepare_test.py')
        with patch.object(gate.platform, 'system', return_value='Darwin'), \
             patch.object(gate, 'committed_source', return_value=source), \
             patch.object(gate.subprocess, 'run', side_effect=failure):
            with self.assertRaises(subprocess.CalledProcessError):
                gate.run_gate(root, expected)
        self.assertFalse(report.exists())


if __name__ == '__main__':
    if '--portable-only' in sys.argv:
        sys.argv.remove('--portable-only')
        PORTABLE_ONLY = True
        print('PORTABLE: synthetic EPUB/cache contracts and four exact UI PNG fixtures; see '
              'docs/CI.md#explicit-cover-coverage-boundary for Mac ImageIO coverage', flush=True)
        unittest.main(defaultTest=['CoverPreparationTests.' + name for name in PORTABLE_CASES])
    else:
        unittest.main()
