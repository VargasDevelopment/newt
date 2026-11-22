"""Offline candidate identity/integrity cases for the injected BenchTests harness."""
import json

import bench_candidate as candidate
from bench_flash_fixtures import APP1, BUILD, REVISION, app_bytes, make_candidate


class CandidateCases:
    def test_candidate_requires_the_one_build_identity(self):
        image = candidate.Candidate(self.root / 'candidate', self.pin)
        self.assertEqual(image.build, BUILD)
        self.assertIn(BUILD.encode() + b'\0', image.image)
        # A release image (Arduino metadata, no Git ID) or the old plain DEV ID.
        for name, other in (('release', 'arduino-version'), ('wrong-revision', 'abcdefabcdef')):
            with self.subTest(image=name):
                directory = self.root / name
                pin = make_candidate(directory, image=app_bytes(other))
                with self.assertRaisesRegex(ValueError, 'App build identity differs or is dirty'):
                    candidate.Candidate(directory, pin)
        for variant in ('default', 'dev-usb', 'regional'):
            with self.subTest(variant=variant):
                directory = self.root / variant
                pin = make_candidate(directory, variant=variant)
                with self.assertRaisesRegex(ValueError, 'clean matching identity receipt'):
                    candidate.Candidate(directory, pin)

    def test_legacy_regional_candidate_uses_its_own_receipt(self):
        directory = self.root / 'legacy'
        pin = make_candidate(directory, legacy=True)
        image = candidate.Candidate(directory, pin)
        self.assertEqual(image.build, REVISION[:12] + '-regional-experiment')

    def test_candidate_refuses_dirty_build_identity(self):
        # Include the clean string too, so only the dirty guard rejects it.
        image = app_bytes(BUILD + '-dirty', project=BUILD)
        directory = self.root / 'dirty'
        pin = make_candidate(directory, image=image)
        with self.assertRaisesRegex(ValueError, 'App build identity differs or is dirty'):
            candidate.Candidate(directory, pin)

    def test_candidate_requires_matching_image_receipt(self):
        directory = self.root / 'replaced'
        make_candidate(directory)
        # A valid replacement app, re-pinned by the manifest, but the original
        # clean identity receipt still names the original app.
        image = app_bytes(BUILD, extra=1)
        (directory / 'firmware.bin').write_bytes(image)
        manifest = json.loads((directory / 'manifest.json').read_bytes())
        manifest['files']['firmware.bin'] = candidate.sha(image)
        raw = json.dumps(manifest).encode()
        (directory / 'manifest.json').write_bytes(raw)
        with self.assertRaisesRegex(ValueError, 'App differs from identity receipt'):
            candidate.Candidate(directory, candidate.sha(raw))

    def test_candidate_integrity_guards(self):
        path = self.root / 'candidate/firmware.bin'
        valid = self.image
        path.write_bytes(path.read_bytes() + b'bad')
        with self.assertRaisesRegex(ValueError, 'hash differs'):
            candidate.Candidate(path.parent, self.pin)
        with self.assertRaisesRegex(ValueError, 'manifest identity'):
            candidate.Candidate(path.parent, '0' * 64)
        candidate.partition_layout(self.image.table[:0xc00])
        for at in (0, 2, 12, 23, 32, len(valid.image) - 1):
            with self.subTest(offset=at):
                image = bytearray(valid.image)
                image[at] ^= 1
                with self.assertRaises(ValueError):
                    candidate.validate_image(bytes(image), valid.build)
        for length in (0, candidate.USABLE + 1):
            with self.assertRaises(ValueError):
                candidate.write_span(APP1, length)
