"""Keep experimental SDK replacement inside a real project-local build root."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import code_health_resources as resources
from freeink_sdk_routes import FIRMWARE_BUILD as BUILD
import freeink_source_pin
import freeink_sdk_provenance as provenance
spec = importlib.util.spec_from_file_location("freeink_patch", ROOT / "tools/apply-freeink-patch.py")
sdk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sdk)


class DestinationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.build = self.root / '.build'
        self.vendor = self.root / 'vendor/freeink-sdk'
        self.vendor.mkdir(parents=True)
        self.marker = self.vendor / 'pinned-source'
        self.marker.write_text('preserve')
        self.constants = patch.multiple(sdk, ROOT=self.root, BUILD=self.build,
                                        DESTINATION=self.build / 'freeink-sdk')
        self.constants.start()
        self.addCleanup(self.constants.stop)

    def reject(self):
        with self.assertRaises(RuntimeError):
            sdk.require_build_destination()
        self.assertEqual(self.marker.read_text(), 'preserve')

    def test_build_symlink_to_vendor(self):
        self.build.symlink_to(self.vendor.parent, target_is_directory=True)
        self.reject()

    def test_build_symlink_to_external_directory(self):
        with tempfile.TemporaryDirectory() as external:
            self.build.symlink_to(external, target_is_directory=True)
            self.reject()

    def test_dangling_build_symlink(self):
        self.build.symlink_to(self.root / 'missing', target_is_directory=True)
        self.reject()

    def test_destination_symlink_to_vendor(self):
        self.build.mkdir()
        (self.build / 'freeink-sdk').symlink_to(self.vendor, target_is_directory=True)
        self.reject()

    def test_destination_outside_build(self):
        self.build.mkdir()
        with patch.object(sdk, 'DESTINATION', self.vendor):
            self.reject()

    def test_nondirectory_build(self):
        self.build.write_text('not a directory')
        self.reject()

    def test_real_project_build_accepts_absent_and_existing_copy(self):
        self.build.mkdir()
        sdk.require_build_destination()
        (self.build / 'freeink-sdk').mkdir()
        sdk.require_build_destination()
        self.assertEqual(self.marker.read_text(), 'preserve')


    def test_each_regional_variant_has_its_own_copy_beside_the_others(self):
        self.build.mkdir()
        copies = {variant: sdk.destination(variant) for variant in ('regional', 'regional-dev-usb')}
        self.assertEqual(copies, {'regional': self.build / 'freeink-sdk',
                                  'regional-dev-usb': self.build / 'freeink-sdk-dev-usb'})
        for variant, copy in copies.items():
            sdk.require_build_destination(copy)
        (copies['regional-dev-usb']).symlink_to(self.vendor, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            sdk.require_build_destination(copies['regional-dev-usb'])
        self.assertEqual(self.marker.read_text(), 'preserve')

    def test_the_one_reader_lane_has_its_composed_copy(self):
        self.build.mkdir()
        self.assertEqual(sdk.reader_destination(BUILD), self.root / '.build/freeink-reader-sdk-regional-dev-usb')
        self.assertEqual(resources.reader_patch_order(BUILD), ('regional', 'producer'))
        sdk.require_build_destination(sdk.reader_destination(BUILD))
        for retired in ('default', 'dev-usb', 'regional'):
            with self.subTest(retired=retired), self.assertRaisesRegex(ValueError, 'Unknown SDK variant'):
                sdk.reader_destination(retired)

    def test_producer_copy_uses_same_destination_boundary(self):
        self.build.mkdir()
        target = self.build / 'freeink-producer-sdk'
        sdk.require_build_destination(target)
        target.symlink_to(self.vendor, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            sdk.require_build_destination(target)
        self.assertEqual(self.marker.read_text(), 'preserve')

    def test_provenance_tree_rejects_link_and_unexpected_extra_bytes(self):
        self.build.mkdir()
        (self.build / 'one').write_text('one')
        original = sdk.tree_digest(self.build)
        (self.build / 'two').write_text('two')
        self.assertNotEqual(sdk.tree_digest(self.build), original)
        (self.build / 'linked').symlink_to(self.marker)
        with self.assertRaises(RuntimeError):
            sdk.tree_digest(self.build)


class ComposedReaderCopyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name).resolve()
        patches = cls.root / 'patches'
        patches.mkdir()
        for stem in ('freeink-producer-policy', 'freeink-regional-gray'):
            for suffix in ('.patch', '.json'):
                shutil.copy2(ROOT / 'patches' / (stem + suffix), patches / (stem + suffix))
        cls.constants = patch.multiple(sdk, ROOT=cls.root, BUILD=cls.root / '.build')
        cls.constants.start()
        for variant in resources.VARIANTS:
            sdk.main(['--reader-lane', variant])

    @classmethod
    def tearDownClass(cls):
        cls.constants.stop()
        cls.temp.cleanup()

    def target(self, variant):
        return self.root / resources.reader_sdk(variant)

    def provenance(self, variant):
        return self.root / resources.reader_sdk_provenance(variant)

    def assert_receipt_rejected(self, variant, mutate):
        path = self.provenance(variant)
        original = path.read_bytes()
        receipt = json.loads(original)
        mutate(receipt)
        try:
            path.write_text(json.dumps(receipt))
            with self.assertRaises(RuntimeError):
                sdk.validate_reader_copy(variant)
        finally:
            path.write_bytes(original)
        sdk.validate_reader_copy(variant)

    def test_the_copy_has_exact_order_results_inventory_and_license(self):
        producer = json.loads((self.root / 'patches/freeink-producer-policy.json').read_text())
        regional = json.loads((self.root / 'patches/freeink-regional-gray.json').read_text())
        for variant in resources.VARIANTS:
            with self.subTest(variant=variant):
                receipt = sdk.validate_reader_copy(variant)
                target = self.target(variant)
                self.assertEqual(tuple(item['name'] for item in receipt['patches']),
                                 resources.reader_patch_order(variant))
                self.assertEqual(receipt['license_sha256'], sdk.digest(target / 'LICENSE'))
                self.assertEqual(receipt['patched_tree_sha256'], sdk.tree_digest(target))
                self.assertEqual(receipt['source_structure_sha256'], receipt['patched_structure_sha256'])
                self.assertTrue((target / 'libs/assets/Icons/lucide').is_dir())
                self.assertEqual(list((target / 'libs/assets/Icons/lucide').iterdir()), [])
                for library in resources.SDK_LIBRARIES:
                    self.assertTrue((target / library).is_dir(), library)
                for path, hashes in producer['files'].items():
                    self.assertEqual(sdk.digest(target / path), hashes['patched_sha256'])
                for path, hashes in regional['files'].items():
                    self.assertEqual(sdk.digest(target / path), hashes['patched_sha256'])

    def test_receipt_rejects_dropped_patch_wrong_order_and_changed_provenance(self):
        variant = BUILD
        path = self.provenance(variant)
        original = path.read_bytes()
        receipt = json.loads(original)
        mutations = []
        dropped = json.loads(original); dropped['patches'].pop(); mutations.append(dropped)
        reversed_order = json.loads(original); reversed_order['patches'].reverse(); mutations.append(reversed_order)
        changed = json.loads(original); changed['patches'][0]['manifest_sha256'] = '0' * 64; mutations.append(changed)
        try:
            for mutation in mutations:
                with self.subTest(mutation=tuple(item['name'] for item in mutation['patches'])):
                    path.write_text(json.dumps(mutation))
                    with self.assertRaises(RuntimeError):
                        sdk.validate_reader_copy(variant)
        finally:
            path.write_bytes(original)
        sdk.validate_reader_copy(variant)

    def test_changed_tree_is_rejected_and_restored(self):
        variant = BUILD
        target = self.target(variant) / 'libs/book/FreeInkBook/include/BookStorage.h'
        original = target.read_bytes()
        try:
            target.write_bytes(original + b'changed')
            with self.assertRaises(RuntimeError):
                sdk.validate_reader_copy(variant)
        finally:
            target.write_bytes(original)
        sdk.validate_reader_copy(variant)

    def test_an_exact_copy_stays_in_place_and_any_drift_is_replaced(self):
        # Gate phases read one shared copy while another phase re-applies it.
        target = self.target(BUILD)
        drifts = {'none': lambda: None,
                  'bytes': lambda: (target / 'LICENSE').write_bytes(b'changed'),
                  'directory': lambda: (target / 'forged-empty-directory').mkdir(),
                  'provenance': lambda: self.provenance(BUILD).write_text('{}\n')}
        for drift, apply in drifts.items():
            with self.subTest(drift=drift):
                inode = target.stat().st_ino
                apply()
                sdk.main(['--reader-lane', BUILD])
                self.assertEqual(target.stat().st_ino == inode, drift == 'none')
                sdk.validate_reader_copy(BUILD)

    def test_empty_directory_and_executable_mode_drift_are_rejected(self):
        variant, target = BUILD, self.target(BUILD)
        extra = target / 'forged-empty-directory'
        try:
            extra.mkdir()
            with self.assertRaisesRegex(RuntimeError, 'structure'):
                sdk.validate_reader_copy(variant)
        finally:
            extra.rmdir()

        executable = target / 'libs/book/FreeInkBook/test/host/run.sh'
        original_mode = executable.stat().st_mode & 0o777
        try:
            base = original_mode & ~0o111
            for mask in (0o000, 0o001, 0o010, 0o011, 0o100, 0o101, 0o110, 0o111):
                with self.subTest(mask=oct(mask)):
                    executable.chmod(base | mask)
                    if mask & 0o100:
                        sdk.validate_reader_copy(variant)
                    else:
                        with self.assertRaisesRegex(RuntimeError, 'structure'):
                            sdk.validate_reader_copy(variant)
        finally:
            executable.chmod(original_mode)
        sdk.validate_reader_copy(variant)

    def test_source_identity_tree_and_intermediate_states_are_independently_derived(self):
        self.assert_receipt_rejected(BUILD, lambda receipt: receipt.update(source_commit='0' * 40))
        self.assert_receipt_rejected(BUILD, lambda receipt: receipt.update(source_origin='https://invalid.example/sdk'))

        def source_tree(receipt):
            receipt['source_tree_sha256'] = '1' * 64
            receipt['patches'][0]['input_tree_sha256'] = '1' * 64
        self.assert_receipt_rejected(BUILD, source_tree)

        def intermediate(receipt):
            receipt['patches'][0]['result_tree_sha256'] = '2' * 64
            receipt['patches'][1]['input_tree_sha256'] = '2' * 64
        self.assert_receipt_rejected(BUILD, intermediate)

    def test_manifest_and_simulated_checkout_cannot_move_the_independent_source_pin(self):
        manifest_path = self.root / 'patches/freeink-producer-policy.json'
        original, attacker = manifest_path.read_bytes(), '0' * 40
        manifest = json.loads(original)
        manifest['source_commit'] = attacker
        real_run = freeink_source_pin.subprocess.run

        def forged_head(command, *arguments, **options):
            if command[:3] == ['git', '-C', str(sdk.SOURCE)] and command[3:] == ['rev-parse', 'HEAD']:
                result = real_run(command, *arguments, **options)
                return type(result)(result.args, result.returncode, attacker + '\n', result.stderr)
            return real_run(command, *arguments, **options)

        try:
            manifest_path.write_text(json.dumps(manifest))
            with patch.object(freeink_source_pin.subprocess, 'run', side_effect=forged_head):
                with self.assertRaisesRegex(RuntimeError, 'reviewed pin'):
                    sdk.validate_reader_copy(BUILD)
        finally:
            manifest_path.write_bytes(original)
        sdk.validate_reader_copy(BUILD)

    def test_joint_tree_and_receipt_forgery_cannot_change_unchanged_file_license_or_inventory(self):
        variant = BUILD
        target, provenance = self.target(variant), self.provenance(variant)
        original_receipt = provenance.read_bytes()

        def forged_final(receipt):
            tree = sdk.tree_digest(target)
            receipt['patches'][-1]['result_tree_sha256'] = tree
            receipt['patched_tree_sha256'] = tree
            provenance.write_text(json.dumps(receipt))

        unchanged = target / 'README.md'
        original = unchanged.read_bytes()
        try:
            unchanged.write_bytes(original + b'forged unchanged bytes')
            forged_final(json.loads(original_receipt))
            with self.assertRaises(RuntimeError):
                sdk.validate_reader_copy(variant)
        finally:
            unchanged.write_bytes(original)
            provenance.write_bytes(original_receipt)

        license_path = target / 'LICENSE'
        original = license_path.read_bytes()
        try:
            license_path.write_bytes(original + b'forged license bytes')
            receipt = json.loads(original_receipt)
            receipt['license_sha256'] = sdk.digest(license_path)
            forged_final(receipt)
            with self.assertRaises(RuntimeError):
                sdk.validate_reader_copy(variant)
        finally:
            license_path.write_bytes(original)
            provenance.write_bytes(original_receipt)

        extra = target / 'forged-extra'
        try:
            extra.write_bytes(b'forged inventory member')
            forged_final(json.loads(original_receipt))
            with self.assertRaises(RuntimeError):
                sdk.validate_reader_copy(variant)
        finally:
            extra.unlink()
            provenance.write_bytes(original_receipt)
        sdk.validate_reader_copy(variant)

if __name__ == '__main__':
    unittest.main()
