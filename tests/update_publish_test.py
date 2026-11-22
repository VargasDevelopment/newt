#!/usr/bin/env python3
"""Publisher outputs through the shipped Rust parser."""
from functools import reduce
import hashlib
import json
from operator import xor
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tests'))
from bench_candidate import USABLE, VARIANT, sha
import bench_flash_fixtures as fixtures
from rust_app_archive import verify_toolchain
from x3_slots import NEWT_MARK
import update_publish
from update_publish import newt_manifest
from daily_firmware import FEATURES, IMAGE as DAILY_IMAGE

TOOL = ROOT / 'tools/update_publish.py'
REVISION = '0123456789abcdef0123456789abcdef01234567'
BUILD = REVISION[:12]
VERSION = '0.2.0-alpha.1'
ELF = '33' * 32


def app(project, version, after=b'', pad=0, elf='11' * 32):
    """A one-segment ESP32-C3 image: app descriptor, `after`, filler, then `pad` zero bytes."""
    header = bytearray(24)
    header[:4] = b'\xe9\x01\x02\x4f'
    struct.pack_into('<H', header, 12, 5)
    header[23] = 1
    payload = fixtures.descriptor(project, version, elf) + after + bytes(range(256))
    checksum = reduce(xor, payload, 0xef)  # zero padding leaves it unchanged
    payload += bytes(pad)
    payload += bytes(-len(payload) % 4)  # segment lengths are a multiple of 4, as esptool writes them
    data = bytes(header) + struct.pack('<II', 0x3c000020, len(payload)) + payload
    data += b'\0' * (15 - len(data) % 16) + bytes([checksum])
    return data + hashlib.sha256(data).digest()


def identity(version, build):
    return version.encode().ljust(24, b'\0') + build.encode().ljust(16, b'\0')


def newt_image(version=VERSION, build=BUILD, block=None, mark=NEWT_MARK, pad=0):
    """Mark and identity block as rust/firmware/native/app_descriptor.h places them, and the DEV build ID string."""
    block = identity(version, build) if block is None else block
    return app('arduino-lib-builder', '8cabf2c', mark + block + build.encode() + b'\0', pad)


def receipt(image, **changes):
    return {'binary_sha256': sha(image), 'build_id': BUILD, 'dirty': False, 'firmware_version': VERSION,
            'git_revision': REVISION, 'variant': VARIANT, 'image': DAILY_IMAGE,
            'features': list(FEATURES), **changes}


def resealed(image, at, value):
    """`image` with byte `at` set to `value` and its appended digest recomputed."""
    data = bytearray(image[:-32])
    data[at] = value
    return bytes(data) + hashlib.sha256(data).digest()


def lines(text):
    return ''.join(line + '\n' for line in text).encode()


IMAGE = newt_image()
NEWT_MANIFEST = lines(['format=newt-update-1', 'kind=newt', f'version={VERSION}', f'build={BUILD}',
                       f'size={len(IMAGE)}', f'sha256={sha(IMAGE)}'])
# The CrossInk manifest kind Newt read before NEWT-71: another firmware now
# goes on the card as its bare `.bin`, so the device refuses this kind.
CROSSINK = app('CrossInk', 'v1.6.1', elf=ELF)
CROSSINK_MANIFEST = lines(['format=newt-update-1', 'kind=crossink', 'version=v1.6.1', f'elf-sha256={ELF}',
                           f'size={len(CROSSINK)}', f'sha256={sha(CROSSINK)}'])


class Manifests(unittest.TestCase):
    def test_newt_manifest_is_exact(self):
        self.assertEqual(newt_manifest(IMAGE, receipt(IMAGE)), NEWT_MANIFEST)

    def test_only_the_full_rust_daily_image_can_publish(self):
        for image in (None, 'cpp', 'rust-app bench'):
            with self.subTest(image=image), self.assertRaisesRegex(ValueError, 'full Rust daily image'):
                newt_manifest(IMAGE, {**receipt(IMAGE), 'image': image})
        for feature in FEATURES:
            features = [item for item in FEATURES if item != feature]
            with self.subTest(feature=feature), self.assertRaisesRegex(ValueError, 'full Rust daily image'):
                newt_manifest(IMAGE, receipt(IMAGE, features=features))

    def test_newt_refusals(self):
        def case(image=IMAGE, **changes):
            return image, receipt(image, **changes)

        upper, other = '0123456789AB', 'abcdefabcdef'
        long_version = '1.2.3-alpha.123456789012'  # 24 characters: no NUL left in the field
        bad_digest = IMAGE[:-1] + bytes([IMAGE[-1] ^ 1])
        large = newt_image(pad=USABLE)
        for name, (image, record), message in (
                ('dirty', case(dirty=True), 'dirty'),
                ('dirty flag not a boolean', case(dirty='false'), 'dirty'),
                ('another variant', case(variant='regional'), 'one firmware build'),
                ('uppercase build ID',
                 case(newt_image(build=upper), build_id=upper, git_revision=upper + REVISION[12:]), '12 lowercase hex'),
                ('dirty build ID', case(build_id=BUILD + '-dirty'), '12 lowercase hex'),
                ('build ID not the revision', case(newt_image(build=other), build_id=other), 'Git revision'),
                ('image not the receipt', case(binary_sha256='00' * 32), 'differs from its identity receipt'),
                ('not SemVer: two parts', case(newt_image('0.2'), firmware_version='0.2'), 'SemVer'),
                ('not SemVer: leading zero', case(newt_image('1.02.3'), firmware_version='1.02.3'), 'SemVer'),
                ('not SemVer: pre-release zero', case(newt_image('1.2.3-01'), firmware_version='1.2.3-01'),
                 'SemVer'),
                ('version too long', case(newt_image(long_version), firmware_version=long_version), '23 characters'),
                ('image digest', case(bad_digest), 'digest mismatch'),
                ('larger than the usable slot', case(large), 'usable slot'),
                ('no Newt mark', case(newt_image(mark=bytes(16))), 'Newt mark'),
                ('identity version differs', case(newt_image(block=identity('0.1.0', BUILD))), 'identity block'),
                ('identity build empty', case(newt_image(block=identity(VERSION, ''))), 'identity block')):
            with self.subTest(name), self.assertRaisesRegex(ValueError, message):
                newt_manifest(image, record)

class Command(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='update-publish-')
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)

    def run_tool(self, *arguments):
        return subprocess.run([sys.executable, '-B', str(TOOL), *map(str, arguments)], capture_output=True, text=True)

    def build(self, image=IMAGE, record=None):
        (self.root / 'firmware.bin').write_bytes(image)
        (self.root / 'receipt.json').write_text(json.dumps(receipt(image) if record is None else record))
        return '--image', self.root / 'firmware.bin', '--receipt', self.root / 'receipt.json'

    def test_newt_round_trip(self):
        out = self.root / 'out'
        result = self.run_tool('newt', *self.build(), '--out', out)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((out / 'newt.bin').read_bytes(), IMAGE)
        self.assertEqual((out / 'newt.manifest').read_bytes(), NEWT_MANIFEST)
        self.assertEqual(result.stdout.splitlines()[-3:],
                         ['Copy newt.bin and newt.manifest to /Newt/Update/ on the card.',
                          f'newt.manifest sha256 {sha(NEWT_MANIFEST)}', f'newt.bin sha256 {sha(IMAGE)}'])

    def test_refusal_writes_nothing(self):
        out = self.root / 'out'
        result = self.run_tool('newt', *self.build(record=receipt(IMAGE, dirty=True)), '--out', out)
        self.assertEqual((result.returncode, result.stdout), (1, ''))
        self.assertIn('Publish refused: The build is dirty', result.stderr)
        self.assertFalse(out.exists())

    def test_never_overwrites(self):
        out = self.root / 'out'
        source = self.build()
        self.assertEqual(self.run_tool('newt', *source, '--out', out).returncode, 0)
        again = self.run_tool('newt', *self.build(newt_image(pad=16)), '--out', out)
        self.assertEqual(again.returncode, 1)
        self.assertIn('already holds newt.bin and newt.manifest', again.stderr)
        self.assertEqual(((out / 'newt.bin').read_bytes(), (out / 'newt.manifest').read_bytes()),
                         (IMAGE, NEWT_MANIFEST))
        # A leftover manifest alone stops the image too, so no pair is ever half new.
        partial = self.root / 'partial'
        partial.mkdir()
        (partial / 'newt.manifest').write_bytes(b'old\n')
        result = self.run_tool('newt', *source, '--out', partial)
        self.assertEqual(result.returncode, 1)
        self.assertEqual([path.name for path in partial.iterdir()], ['newt.manifest'])
        self.assertEqual((partial / 'newt.manifest').read_bytes(), b'old\n')

    def test_candidate(self):
        directory = self.root / 'candidate'
        image = newt_image(build=fixtures.BUILD)
        fixtures.make_candidate(directory, image=image)
        # Generic bench candidates remain valid; publishing requires the daily declaration.
        name = f'firmware-identity-{VARIANT}.json'
        record = json.loads((directory / name).read_text())
        (directory / name).write_text(json.dumps({**record, 'firmware_version': VERSION,
                                                  'image': DAILY_IMAGE, 'features': list(FEATURES)}))
        manifest = json.loads((directory / 'manifest.json').read_text())
        manifest['files'][name] = sha((directory / name).read_bytes())
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        reviewed = sha((directory / 'manifest.json').read_bytes())
        refused = self.run_tool('newt', '--candidate', directory, '--manifest-sha256', '0' * 64,
                                '--out', self.root / 'x')
        self.assertEqual(refused.returncode, 1)
        self.assertIn('Reviewed manifest identity differs', refused.stderr)
        result = self.run_tool('newt', '--candidate', directory, '--manifest-sha256', reviewed,
                               '--out', self.root / 'out')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'out/newt.bin').read_bytes(), image)
        self.assertIn(f'build={fixtures.BUILD}\n', (self.root / 'out/newt.manifest').read_text())

RUST_HARNESS = r"""
#[path = @MANIFEST@]
pub mod update_manifest;
use std::ffi::{CStr, CString};
use update_manifest::{UpdateKind, UpdateManifest, parse_update_manifest, semver2_valid};

fn text(bytes: &[u8]) -> &str {
    CStr::from_bytes_until_nul(bytes).unwrap().to_str().unwrap()
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("parse") => for path in args {
            let bytes = std::fs::read(path).unwrap();
            let mut manifest = UpdateManifest::default();
            if !parse_update_manifest(Some(&bytes), bytes.len(), &mut manifest) {
                println!("refused");
                continue;
            }
            let kind = match manifest.kind { UpdateKind::Newt => "newt", UpdateKind::Firmware => "firmware" };
            let digest: String = manifest.sha256.iter().map(|byte| format!("{byte:02x}")).collect();
            println!("{kind} {} {} {} {digest}", text(&manifest.version), text(&manifest.build), manifest.size);
        },
        Some("semver") => for version in args {
            println!("{}", u8::from(semver2_valid(Some(&CString::new(version).unwrap()))));
        },
        _ => std::process::exit(2),
    }
}
"""


class DeviceParser(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = tempfile.TemporaryDirectory(prefix='update-publish-device-')
        cls.addClassCleanup(cls.scratch.cleanup)
        cls.root = Path(cls.scratch.name)
        cls.rustc = verify_toolchain(ROOT) / 'rustc'
        cls.compile_rust('rust', ROOT / 'rust/core/src/update_manifest.rs')

    @classmethod
    def compile_rust(cls, name, source):
        harness = cls.root / f'{name}.rs'
        harness.write_text(RUST_HARNESS.replace('@MANIFEST@', json.dumps(str(source))))
        subprocess.run([str(cls.rustc), '--edition=2024', '-Dwarnings', '--crate-name', 'publish_parser',
                        str(harness), '-o', str(cls.root / name)], check=True)

    def device(self, *arguments, parser='rust'):
        return subprocess.run([str(self.root / parser), *map(str, arguments)], check=True,
                              capture_output=True, text=True).stdout.split('\n')[:-1]

    def save(self, name, data):
        path = self.root / name
        path.write_bytes(data)
        return path

    def published(self, kind, image, text):
        out = self.root / kind
        update_publish.publish(out, kind, image, text)
        return out / f'{kind}.manifest', out / f'{kind}.bin'

    def test_device_reads_what_publish_writes(self):
        newt = self.published('newt', IMAGE, newt_manifest(IMAGE, receipt(IMAGE)))
        self.assert_published('rust', newt[0])

    def assert_published(self, parser, manifest, *, image=IMAGE, version=VERSION):
        crossink = self.save('crossink.manifest', CROSSINK_MANIFEST)
        self.assertEqual(self.device('parse', manifest, crossink, parser=parser),
                         [f'newt {version} {BUILD} {len(image)} {sha(image)}', 'refused'])

    def test_rust_only_rejection_fails_the_publish_round_trip(self):
        source = ROOT / 'rust/core/src/update_manifest.rs'
        mutant = self.root / 'mutant'
        (mutant / 'update_manifest').mkdir(parents=True)
        anchor = 'b"newt-update-1"'
        self.assertEqual(source.read_text().count(anchor), 1)
        (mutant / source.name).write_text(source.read_text().replace(anchor, 'b"newt-update-2"'))
        (mutant / 'update_manifest/semver.rs').write_bytes((source.parent / 'update_manifest/semver.rs').read_bytes())
        self.compile_rust('rust-reject', mutant / source.name)
        manifest, _ = self.published('rust-reject-output', IMAGE, newt_manifest(IMAGE, receipt(IMAGE)))
        self.assert_published('rust', manifest)
        with self.assertRaises(AssertionError):
            self.assert_published('rust-reject', manifest)

    def test_each_published_version_reads_back(self):
        for version in ('0.1.0-alpha.15', '0.1.0-alpha.16', '0.1.0-alpha.17'):
            with self.subTest(version=version):
                image = newt_image(version=version)
                paths = self.published(version, image,
                                       newt_manifest(image, receipt(image, firmware_version=version)))
                self.assert_published('rust', paths[0], image=image, version=version)

    def test_device_refuses_mutated_manifests(self):
        text = NEWT_MANIFEST.decode()
        rows = text.splitlines(keepends=True)
        size = f'size={len(IMAGE)}\n'
        mutated = {
            'reordered': ''.join([rows[0], rows[2], rows[1], *rows[3:]]),
            'extra line': text + 'note=x\n',
            'crlf': text.replace('\n', '\r\n'),
            'uppercase hex': text.replace(sha(IMAGE), sha(IMAGE).upper()),
            'leading-zero size': text.replace(size, 'size=0' + size[5:]),
            'no final LF': text[:-1],
        }
        files = [self.save('mutated-' + name.replace(' ', '-'), body.encode()) for name, body in mutated.items()]
        self.assertEqual(self.device('parse', *files), ['refused'] * len(mutated))

    def test_same_semver_rule(self):
        versions = ['0.1.0', VERSION, '1.0.0-rc.1+build.5', '1.2.3-0a', '1.2.3+001', '10.20.30', '1.2.3-x-y',
                    '1.2', '1.2.3.4', '01.2.3', '1.2.3-01', '1.2.3-', '1.2.3+', '1.2.3-a..b', 'v1.2.3', '1.2.3_4']
        self.assertEqual(self.device('semver', *versions),
                         [str(int(update_publish.SEMVER2.fullmatch(v) is not None)) for v in versions])


if __name__ == '__main__':
    unittest.main(verbosity=2)
