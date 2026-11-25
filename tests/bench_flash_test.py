#!/usr/bin/env python3
"""No-device installer regressions and guard removals, all state scratch-injected."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--tool-root', type=Path, default=ROOT / 'tools')
parser.add_argument('--single')
parser.add_argument('--negative-controls', action='store_true')
args = parser.parse_args()
sys.path.insert(0, str(args.tool_root))
sys.path.append(str(ROOT / 'tools'))
import bench_candidate as candidate
import bench_device as transport
import bench_flash as tool
import bench_lease as safety
from bench_flash_fixtures import APP0, APP1, BENCH_INSTALL, BUILD, Fake, IDENTITY, MAC, REVISION, RULE, make_candidate, select
import x3_slots

REAL_SHARED_STATE_ROOT = safety.shared_state_root

spec = importlib.util.spec_from_file_location('bench_bridge', args.tool_root / 'dev-usb.py')
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


from bench_candidate_cases import CandidateCases
from bench_session_cases import SessionCases
from bench_cancel_cases import CancelCases
from bench_other_cases import OtherFirmwareCases


class BenchHarness(unittest.TestCase):
    """Scratch owner rule, shared state, candidate and fake unit; also used by
    tests/bench_flash_layouts_test.py."""
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='bench-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.shared = self.root / 'common-git/newt-bench'
        self.rule = self.root / 'owner.rules'
        self.rule.write_text(RULE)
        self.rule.chmod(0o444)
        original_fstat = os.fstat
        def owner_stat(fd):
            result = original_fstat(fd)
            try:
                info = self.rule.stat()
            except FileNotFoundError:
                return result
            if (result.st_dev, result.st_ino) == (info.st_dev, info.st_ino):
                values = list(result)
                values[4] = 0  # The one injected owner rule, never an agent config.
                return os.stat_result(values)
            return result
        for guard in (patch.object(tool, 'host_boot_id', return_value='fake-host-boot'),
                      patch.object(safety, 'UDEV_RULE', self.rule),
                      patch.object(safety, 'shared_state_root', side_effect=lambda: self.shared),
                      patch.object(safety.os, 'fstat', side_effect=owner_stat)):
            guard.start()
            self.addCleanup(guard.stop)
        self.pin = make_candidate(self.root / 'candidate')
        self.image = candidate.Candidate(self.root / 'candidate', self.pin)
        self.device = Fake(self.image.table)
        self.session = self.root / 'session'
        self.output = io.StringIO()
        self.addCleanup(self.output.close)
        self.seen_identities = []

    @property
    def unit_dir(self):
        return self.shared / MAC.replace(':', '')

    def invoke(self, session=None, directory=None, pin=None, action='install', rule=None, repair=False):
        session = self.session if session is None else session
        directory = self.root / 'candidate' if directory is None else directory
        argv = ['bench_flash.py', action, str(directory), '--manifest-sha256', pin or self.pin,
                '--session', str(session)] + \
               (['--repair-invalid'] if repair else [])
        if action == 'slots':
            argv = ['bench_flash.py', action, '--session', str(session)]
        if rule is not None:
            argv += ['--identity-rule', str(rule)]
        def factory(identity):
            self.seen_identities.append(identity)
            return self.device
        with patch.object(tool, 'ROOT', self.root, create=True), patch.object(tool, 'Device', side_effect=factory):
            with patch.object(sys, 'argv', argv), contextlib.redirect_stdout(self.output), contextlib.redirect_stderr(self.output):
                return tool.main()

    def full_reads(self):
        return [read for read in self.device.reads if read[1] == candidate.FLASH]

    def refused_untouched(self, message, **options):
        before = bytes(self.device.memory)
        with self.assertRaisesRegex(ValueError, message):
            self.invoke(**options)
        self.assertEqual(self.device.writes, [])
        # A plain comparison: unittest's diff of a whole flash image takes hours.
        self.assertTrue(self.device.memory == before, 'Device memory changed')
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def bench_layout(self):
        heads = [bytes(self.device.memory[at:at + 0x200]) for at in (APP0, APP1)]
        return x3_slots.decide(bytes(self.device.memory[0xe000:0x10000]), heads)


class BenchTests(CandidateCases, OtherFirmwareCases, CancelCases, SessionCases, BenchHarness):
    def test_two_installs_read_once_and_hello(self):
        self.invoke()
        self.invoke()
        self.assertEqual(self.full_reads(), [(0, candidate.FLASH)])
        self.assertEqual(self.device.writes, BENCH_INSTALL * 2)
        self.assertEqual(self.device.hellos, [BUILD] * 2)
        self.assertFalse((self.unit_dir / 'STOP').exists())
        self.assertEqual(json.loads((self.unit_dir / 'write.json').read_text())['phase'], 'verified')

    def boundary_corruption(self, when):
        for edge in (APP1 - 1, APP1 + 4096):
            with self.subTest(when=when, edge=hex(edge)):
                self.shared = self.root / f'units-{when}-{edge}'
                self.device = Fake(self.image.table)
                setattr(self.device, 'flip_' + when, edge)
                with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
                    self.invoke(session=self.root / f'session-{when}-{edge}')
                self.assertEqual(self.device.writes, [] if when == 'before' else BENCH_INSTALL[:2])
                self.assertEqual(self.device.resets, 0)
                self.assertEqual(self.device.hellos, [])
                self.assertEqual((self.unit_dir / 'STOP').exists(), when == 'during')

    def test_before_boundary_bytes_refuse_write(self):
        self.boundary_corruption('before')

    def test_during_boundary_bytes_report_failure(self):
        self.boundary_corruption('during')

    def test_missing_backup_forces_full(self):
        self.invoke()
        (self.session / 'backup.bin').unlink()
        self.invoke()
        self.assertEqual(self.full_reads(), [(0, candidate.FLASH)] * 2)
        self.assertEqual(len(list(self.session.glob('session.json.orphan-*'))), 1)

    def test_missing_receipt_forces_full(self):
        self.invoke()
        (self.session / 'session.json').unlink()
        self.invoke()
        self.assertEqual(len(self.full_reads()), 2)
        self.assertEqual(len(list(self.session.glob('backup.bin.orphan-*'))), 1)

    def test_corrupt_backup_refuses_write(self):
        self.invoke()
        (self.session / 'backup.bin').write_bytes(b'broken')
        with self.assertRaisesRegex(ValueError, 'backup corrupt'):
            self.invoke()
        self.assertEqual(self.device.writes, BENCH_INSTALL)
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_expired_or_other_host_session_refused(self):
        self.invoke()
        receipt = json.loads((self.session / 'session.json').read_text())
        for changes in ({'created': 0}, {'host_boot': 'other-boot'}, {'mac': 'other-unit'}):
            (self.session / 'session.json').write_text(json.dumps(receipt | changes))
            with self.assertRaisesRegex(ValueError, 'Session identity/age'):
                self.invoke()
        self.assertEqual(self.device.writes, BENCH_INSTALL)

    def test_outside_drift_on_fast_install_refused(self):
        self.invoke()
        self.device.memory[-1] ^= 1
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke()
        self.assertEqual(self.device.writes, BENCH_INSTALL)
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_table_difference_refuses_write(self):
        self.device.memory[0x8000] ^= 1
        with self.assertRaisesRegex(ValueError, 'partition table differs'):
            self.invoke()
        self.assertEqual(self.device.writes, [])

    def test_unclear_slot_identity_refuses_write(self):
        crossink = bytes(self.device.memory[APP0:APP0 + 0x200])
        for name, change, message in (
                ('both CrossInk', lambda memory: memory.__setitem__(slice(APP1, APP1 + 0x200), crossink),
                 'Both slots hold another firmware'),
                ('no other firmware or booting Newt',
                 lambda memory: [memory.__setitem__(slice(APP0, APP0 + 0x200), memory[APP1:APP1 + 0x200]),
                                 memory.__setitem__(slice(APP1, APP1 + 0x200), b'\xff' * 0x200)],
                 'Neither slot holds another firmware, and app1, the slot that boots, does not hold Newt'),
                ('unknown image', lambda memory: memory.__setitem__(APP1, 0), 'unreadable image'),
                ('invalid selection', lambda memory: [memory.__setitem__(at + 28, memory[at + 28] ^ 1)
                                                      for at in (0xe000, 0xf000)], 'No valid OTA selection')):
            with self.subTest(name):
                self.shared = self.root / ('units-' + name.replace(' ', '-'))
                self.device = Fake(self.image.table)
                change(self.device.memory)
                self.refused_untouched(message, session=self.root / ('session-' + name.replace(' ', '-')))

    def test_main_layout_installs_newt_into_app0(self):
        self.device = Fake(self.image.table, layout='main')
        crossink = bytes(self.device.memory[APP1:])
        self.invoke()
        self.assertEqual(self.device.writes, [(APP0, 4096), (0xe000, 4096)])
        ota = bytes(self.device.memory[0xe000:0x10000])
        # One boot after the write; Newt's answer proves it confirmed the selection.
        self.assertEqual((x3_slots.boot_slot(ota), x3_slots.selections(ota)[0].state), (0, x3_slots.STATE_VALID))
        self.assertEqual(self.device.boots, [0])
        self.assertEqual(bytes(self.device.memory[APP1:]), crossink)
        self.assertEqual(self.device.hellos, [BUILD])

    def test_two_other_firmwares_refuse_write(self):
        # Neither slot is Newt's: the tools never choose which firmware to replace.
        self.device = Fake(self.image.table, layout='legacy')
        self.refused_untouched('Both slots hold another firmware')

    def test_new_selection_survives_its_first_boot(self):
        # Round trip 1's state: CrossInk confirmed seq 7, Newt installed in app1.
        self.invoke()
        for at, seq in ((0xe000, 7), (0xf000, 6)):
            select(self.device.memory, at, seq)
        self.device.boots.clear()
        self.invoke(action='boot-newt')
        # No second reset while the new selection is pending: Newt booted once,
        # answered on the same port and confirmed seq 8; CrossInk's record stays.
        ota = bytes(self.device.memory[0xe000:0x10000])
        self.assertEqual(self.device.boots, [1])
        self.assertEqual([(r.seq, r.state) for r in x3_slots.selections(ota)],
                         [(7, x3_slots.STATE_VALID), (8, x3_slots.STATE_VALID)])
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_boot_newt_selects_the_installed_candidate_only(self):
        self.invoke()
        for at, seq in ((0xe000, 7), (0xf000, 6)):  # CrossInk (app0) selected
            select(self.device.memory, at, seq)
        image = bytes(self.device.memory[APP1:APP1 + 4096])
        self.device.memory[APP1 + 0x30] ^= 1
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke(action='boot-newt')
        self.device.memory[APP1:APP1 + 4096] = image
        self.invoke(action='boot-newt')
        self.assertEqual(self.device.writes, BENCH_INSTALL + [(0xf000, 4096)])
        self.assertEqual(x3_slots.boot_slot(bytes(self.device.memory[0xe000:0x10000])), 1)
        self.invoke(action='boot-newt')  # already selected: nothing written
        self.assertEqual(len(self.device.writes), 4)

    def test_fast_session_partition_table_checked(self):
        self.invoke()
        table = bytearray(self.image.table)
        table[12:28] = b'other-nvs'.ljust(16, b'\0')
        table[144:160] = hashlib.md5(table[:128]).digest()
        directory = self.root / 'different-table'
        pin = make_candidate(directory, table=bytes(table))
        with self.assertRaisesRegex(ValueError, 'Session partition layout differs'):
            self.invoke(directory=directory, pin=pin)
        self.assertEqual(self.device.writes, BENCH_INSTALL)

    def test_session_span_refused_before_write(self):
        self.invoke()
        directory = self.root / 'larger'
        pin = make_candidate(directory, extra=4096)
        with self.assertRaisesRegex(ValueError, 'Image exceeds session erase span'):
            self.invoke(directory=directory, pin=pin)
        self.assertEqual(self.device.writes, BENCH_INSTALL)
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_stale_session_pending_before_device_or_backup(self):
        self.session.mkdir()
        (self.session / 'session.pending').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'Stale session.pending'):
            self.invoke()
        self.assertEqual(self.device.connects, 0)
        self.assertEqual(self.device.reads, [])
        self.assertFalse((self.unit_dir / 'STOP').exists())

    def test_written_range_failure_requires_lead_with_outside_proof(self):
        self.device.corrupt_payload = True
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke()
        record = json.loads((self.unit_dir / 'STOP').read_text())
        self.assertEqual(record['decision'], 'Lead')
        self.assertEqual(record['outside_md5'], 'intact')
        self.assertEqual(self.device.resets, 0)

    def test_new_session_after_partial_write_refused(self):
        self.device.fail_write = True
        with self.assertRaisesRegex(OSError, 'partial write'):
            self.invoke()
        stop = (self.unit_dir / 'STOP').read_bytes()
        record = json.loads(stop)
        self.assertEqual(record['backup_path'], str(self.session / 'backup.bin'))
        self.assertEqual(record['decision'], 'owner')
        self.assertEqual(record['session_dir'], str(self.session))
        self.assertTrue(Path(record['run_dir']).is_dir())
        self.assertEqual(self.device.writes, BENCH_INSTALL[:2])
        connects = self.device.connects
        self.device.fail_write = False
        with self.assertRaisesRegex(ValueError, 'Bench stopped') as refusal:
            self.invoke(session=self.root / 'session2')
        self.assertIn(str(self.unit_dir / 'STOP'), str(refusal.exception))
        self.assertEqual(self.device.connects, connects)
        self.assertEqual(len(self.full_reads()), 1)
        self.assertEqual((self.unit_dir / 'STOP').read_bytes(), stop)

    def test_session_cleanup_cannot_bypass_stop(self):
        self.device.fail_write = True
        with self.assertRaises(OSError):
            self.invoke()
        shutil.rmtree(self.session)
        connects = self.device.connects
        self.device.fail_write = False
        with self.assertRaisesRegex(ValueError, 'Bench stopped'):
            self.invoke(session=self.root / 'fresh-session')
        self.assertEqual(self.device.connects, connects)

    def test_interrupted_write_journal_cannot_bypass_stop(self):
        self.invoke()
        script = """
import os, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from bench_lease import lease_unit
with lease_unit(sys.argv[2], state_root=Path(sys.argv[3])) as unit:
    unit.set_context(Path(sys.argv[4]) / 'interrupted-run', Path(sys.argv[4]))
    unit.begin_write()
    os._exit(77)  # No exception handler, finally block or STOP publication.
"""
        result = subprocess.run([sys.executable, '-B', '-c', script, str(args.tool_root), MAC,
                                 str(self.shared), str(self.session)], capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 77, result.stderr)
        self.assertFalse((self.unit_dir / 'STOP').exists())
        with self.assertRaisesRegex(ValueError, 'unconfirmed shared write'):
            self.invoke(session=self.root / 'new-session')
        self.assertEqual(self.device.connects, 1)
        self.assertTrue((self.unit_dir / 'STOP').exists())

    def test_unconfirmed_outside_proof_controls_recovery_authority(self):
        self.unit_dir.mkdir(parents=True)
        (self.unit_dir / 'write.json').write_text(json.dumps({'phase': 'writing', 'outside_md5': 'intact',
            'run_dir': 'interrupted', 'session_dir': 'old', 'backup_path': 'last-good'}))
        with self.assertRaisesRegex(ValueError, 'unconfirmed shared write'):
            self.invoke()
        record = json.loads((self.unit_dir / 'STOP').read_text())
        self.assertEqual(record['decision'], 'Lead')
        self.assertEqual(self.device.connects, 0)

    def test_malformed_journal_fails_closed_without_device_access(self):
        for index, data in enumerate(('{partial', '{"phase":"verified"}')):
            with self.subTest(data=data):
                self.shared = self.root / f'corrupt-units-{index}'
                self.unit_dir.mkdir(parents=True)
                name = 'write.pending' if index == 0 else 'write.json'
                (self.unit_dir / name).write_text(data)
                with self.assertRaisesRegex(ValueError, 'unconfirmed shared write'):
                    self.invoke()
                record = json.loads((self.unit_dir / 'STOP').read_text())
                self.assertEqual(record['decision'], 'owner')
                self.assertEqual(self.device.connects, 0)

    def test_prewrite_failures_leave_no_stop(self):
        self.device.connect_error = OSError('pre-write connection failure')
        with self.assertRaisesRegex(OSError, 'pre-write connection'):
            self.invoke()
        self.assertFalse((self.unit_dir / 'STOP').exists())
        self.assertFalse((self.unit_dir / 'write.json').exists())
        self.device.connect_error = None
        self.device.flip_before = APP1 - 1
        with self.assertRaisesRegex(ValueError, 'MD5 mismatch'):
            self.invoke()
        self.assertFalse((self.unit_dir / 'STOP').exists())
        self.assertFalse((self.unit_dir / 'write.json').exists())
        self.assertEqual(self.device.writes, [])

    def test_worktree_without_config_works(self):
        self.invoke()
        self.assertEqual(self.seen_identities, [IDENTITY])

    def test_agent_written_config_ignored(self):
        path = self.root / '.build/bench/config.json'
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({'mac': 'aa:bb:cc:dd:ee:ff', 'port': '/dev/never-open'}))
        self.invoke()
        self.assertEqual(self.seen_identities, [IDENTITY])

    def test_missing_or_ambiguous_rule_before_device_calls(self):
        for contents in (None, '', RULE + RULE, RULE.rstrip() + ', ATTRS{serial}=="aa:bb:cc:dd:ee:ff"\n'):
            with self.subTest(contents=contents):
                if contents is None:
                    self.rule.unlink()
                else:
                    self.rule.write_text(contents)
                with self.assertRaises((ValueError, FileNotFoundError)):
                    self.invoke()
                self.assertEqual(self.device.connects, 0)
                self.assertEqual(self.seen_identities, [])

    def test_rule_owner_and_mode_required(self):
        other = self.root / 'agent.rules'
        other.write_text(RULE)
        with self.assertRaisesRegex(ValueError, 'root-owned'):
            safety.load_identity(other)
        self.rule.chmod(0o666)
        with self.assertRaisesRegex(ValueError, 'root-owned'):
            safety.load_identity(self.rule)

    def test_git_common_directory_result_is_canonical(self):
        # Exercise the real root calculation, not a fixed process-global temp path.
        for root in (self.root / 'worktree1', self.root / 'worktree2'):
            for common in (str(self.root / 'common-git'), os.path.relpath(self.root / 'common-git', root)):
                with patch.object(safety, 'ROOT', root):
                    with patch.object(subprocess, 'check_output', return_value=common + '\n'):
                        self.assertEqual(REAL_SHARED_STATE_ROOT(), self.shared)

    def test_git_environment_cannot_redirect_unit_state(self):
        def git_output(*arguments, **options):
            environment = options['env']
            self.assertFalse(any(name.startswith('GIT_') for name in environment))
            self.assertEqual(environment['NEWT_LEASE_TEST'], 'retained')
            return str(self.root / 'common-git') + '\n'
        with patch.dict(os.environ, {'GIT_DIR': '/not-the-project', 'GIT_COMMON_DIR': '/also-wrong',
                                    'GIT_WORK_TREE': '/another-tree', 'NEWT_LEASE_TEST': 'retained'}):
            with patch.object(subprocess, 'check_output', side_effect=git_output):
                self.assertEqual(REAL_SHARED_STATE_ROOT(), self.shared)

    def test_flash_and_usb_share_one_nonblocking_lease(self):
        def other_live_client():
            script = """
import importlib.util, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
sys.path.append(sys.argv[2])
import bench_lease
state_root, serial = Path(sys.argv[3]), sys.argv[4]
bench_lease.shared_state_root = lambda: state_root
bench_lease.port_serial = lambda port: serial
spec = importlib.util.spec_from_file_location('second_client', Path(sys.argv[1]) / 'dev-usb.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.approved_serial = lambda: object()
module.SerialClient = lambda *a, **kw: print('SERIAL_OPENED')
module.run_session = lambda *a: {}
sys.argv = ['dev-usb.py', 'live', '--port', '/dev/synthetic-alias', '--expect-build', 'fake-build']
try:
    module.main()
except ValueError as error:
    print(error, file=sys.stderr)
    raise SystemExit(2)
"""
            result = subprocess.run([sys.executable, '-B', '-c', script, str(args.tool_root),
                str(ROOT / 'tools'), str(self.shared), MAC], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn('bench is leased', result.stderr)
            self.assertNotIn('SERIAL_OPENED', result.stdout)
        self.device.on_write = other_live_client
        self.invoke()
        lock = self.unit_dir / 'lock'
        inode = lock.stat().st_ino
        with safety.lease_unit(MAC):
            pass
        self.assertEqual(lock.stat().st_ino, inode)

    def test_neighbor_journey_holds_usb_lease(self):
        spec = importlib.util.spec_from_file_location('ready_under_test', args.tool_root / 'reading-ready.py')
        ready = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(ready)
        spec = importlib.util.spec_from_file_location('ready_fixture', ROOT / 'tests/reading_latency_ready_test.py')
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)
        fixture.ready = ready
        case = fixture.ReadingLatencyReadyTest('test_installable_and_journey_need_no_approved_latency_baseline')
        result = unittest.TestResult()
        case.run(result)
        self.assertTrue(result.wasSuccessful(), result.errors + result.failures)

    def test_usb_stop_is_checked_before_serial_open(self):
        self.unit_dir.mkdir(parents=True)
        (self.unit_dir / 'STOP').write_text('{}')
        approved = (bridge.ROOT / '.toolchain/platformio/penv').resolve()
        serial = SimpleNamespace(__version__='3.5', __file__=str(approved/'lib/serial/__init__.py'))
        with patch.object(sys, 'prefix', str(approved)), patch.dict(sys.modules, serial=serial), \
             patch.object(safety, 'port_serial', return_value=MAC), patch.object(bridge, 'SerialClient') as client:
            with patch.object(sys, 'argv', ['dev-usb.py', 'live', '--port', IDENTITY['port'], '--expect-build', REVISION[:12]]):
                with self.assertRaisesRegex(ValueError, 'Bench stopped'):
                    bridge.main()
            client.assert_not_called()

    def test_receipt_failure_keeps_shared_stop(self):
        original = tool.save
        def save(path, value):
            if path.name == 'receipt.json':
                raise OSError('Receipt storage failed')
            return original(path, value)
        with patch.object(tool, 'save', save):
            with self.assertRaisesRegex(OSError, 'Receipt storage failed'):
                self.invoke()
        self.assertTrue((self.unit_dir / 'STOP').exists())
        self.assertEqual(self.device.writes, BENCH_INSTALL)

    def test_transport_refuses_writes_outside_newt_slot_and_selection(self):
        layout = self.bench_layout()
        with patch.dict(sys.modules, esptool=SimpleNamespace()):
            device = transport.Device(IDENTITY)
            with self.assertRaisesRegex(ValueError, "Never write the other firmware's slot"):
                device.write(layout, APP0, b'\xff' * 4096)
            for offset, length in ((0, 4096), (0x8000, 4096), (0x9000, 4096), (0xe000, 8192),
                                   (APP1 - 4096, 4096), (APP1 + 4096, 4096)):
                with self.assertRaisesRegex(ValueError, 'Newt\'s slot or one OTA selection sector'):
                    device.write(layout, offset, b'\xff' * length)

    def fake_esptool(self, actual_mac=MAC, security=False, size='16MB'):
        esp = SimpleNamespace(CHIP_NAME='ESP32-C3', read_mac=lambda: bytes.fromhex(actual_mac.replace(':', '')),
            get_security_info=lambda: {'parsed_flags': {'SECURE_BOOT_EN': security}, 'flash_crypt_cnt': 0},
            run_stub=lambda: esp, flash_set_parameters=lambda size: None)
        return SimpleNamespace(__version__='5.1.2', detect_chip=lambda *a, **kw: esp,
            cmds=SimpleNamespace(read_mac=lambda esp: None, get_security_info=lambda esp: None,
                                 detect_flash_size=lambda esp: size, attach_flash=lambda esp: None))

    def test_wrong_mac_value_refused(self):
        with patch.dict(sys.modules, esptool=self.fake_esptool(actual_mac='aa:bb:cc:dd:ee:ff')):
            with patch.object(transport, 'usb_identity'), self.assertRaisesRegex(ValueError, 'MAC differs'):
                transport.Device(IDENTITY).connect()

    def test_usb_guard_precedes_port_open(self):
        with patch.dict(sys.modules, esptool=self.fake_esptool()) as modules:
            with patch.object(modules['esptool'], 'detect_chip') as connect:
                with patch.object(transport, 'usb_identity', side_effect=ValueError('Wrong USB')):
                    with self.assertRaisesRegex(ValueError, 'Wrong USB'):
                        transport.Device(IDENTITY).connect()
                connect.assert_not_called()

    def test_usb_descriptor_checked(self):
        usb = self.root / 'usb'
        node = usb / 'interface/tty'
        node.mkdir(parents=True)
        (usb / 'idVendor').write_text('303a')
        (usb / 'idProduct').write_text('1001')
        resolve = Path.resolve
        def fake_resolve(path, **kwargs):
            if str(path) == IDENTITY['port']:
                return Path('/dev/ttyACM99')
            if str(path) == '/sys/class/tty/ttyACM99/device':
                return node
            return resolve(path, **kwargs)
        with patch.object(Path, 'resolve', fake_resolve):
            (usb / 'serial').write_text(MAC.upper())
            transport.usb_identity(IDENTITY)
            (usb / 'serial').write_text('aa:bb:cc:dd:ee:ff')
            with self.assertRaisesRegex(ValueError, 'USB descriptor'):
                transport.usb_identity(IDENTITY)

    def test_security_and_size_refused(self):
        for kw, error in (({'security': True}, 'Security enabled'), ({'size': '4MB'}, '16 MiB')):
            with patch.dict(sys.modules, esptool=self.fake_esptool(**kw)):
                with patch.object(transport, 'usb_identity'), self.assertRaisesRegex(ValueError, error):
                    transport.Device(IDENTITY).connect()

    def test_transport_disables_esptool_write_retries(self):
        esp = SimpleNamespace(WRITE_FLASH_ATTEMPTS=3)
        loader = SimpleNamespace(WRITE_BLOCK_ATTEMPTS=3)
        def write(peer, regions, **kwargs):
            self.assertEqual(peer.WRITE_FLASH_ATTEMPTS, 1)
            self.assertEqual(loader.WRITE_BLOCK_ATTEMPTS, 1)
            raise OSError('Lost connection')
        fake = SimpleNamespace(loader=loader, cmds=SimpleNamespace(write_flash=write))
        device = transport.Device(IDENTITY)
        device.esp = esp
        with patch.dict(sys.modules, esptool=fake), self.assertRaisesRegex(OSError, 'Lost connection'):
            device.write(self.bench_layout(), APP1, b'\xff' * 4096)



CONTROLS = [
    ('bench_candidate.py', "build.encode() + b'\\0' in image and", 'True and',
     'test_candidate_requires_the_one_build_identity'),
    ('bench_candidate.py', "(build + '-dirty').encode() not in image", 'True',
     'test_candidate_refuses_dirty_build_identity'),
    ('bench_candidate.py', "identity['binary_sha256'] == sha(self.image)", 'True',
     'test_candidate_requires_matching_image_receipt'),
    ('x3_slots.py', '    current = boot_record(ota)',
     '    current = max(range(2), key=lambda i: selections(ota)[i].seq)',
     'test_boot_other_keeps_confirmed_fallback_when_target_pending'),

    ('bench_flash.py', "        step('other-slot-md5', lambda: verify_md5(device, layout.other_offset, other))",
     '        pass', 'test_boot_other_refuses_changed_backup_identity_or_bytes'),
    ('bench_flash.py', "    boot_only = action in ('boot-newt', 'boot-other')",
     "    boot_only = action == 'boot-newt'", 'test_boot_other_preserves_both_apps_in_both_layouts'),

    ('bench_device.py', '        self.defer_cancellation()', '        pass',
     'test_successful_hold_cancel_keeps_port_and_lease'),
    ('bench_device.py', '            except KeyboardInterrupt as error:\n                self.cancelled = self.cancelled or error\n        # Restore',
     '            except KeyboardInterrupt:\n                raise\n        # Restore', 'test_reset_error_cancel_keeps_port_and_lease'),
    ('bench_device.py', '            HardReset(self.esp._port).reset()',
     '            self.esp.hard_reset()', 'test_reset_error_never_retries_or_reopens'),
    ('bench_device.py', 'CONFIRM_SECONDS = 30', 'CONFIRM_SECONDS = 29',
     'test_transport_close_holds_after_success_and_reset_error'),
    ('bench_flash.py', '    identity = load_identity(args.identity_rule)',
     '    identity = load_identity()', 'test_explicit_unit_rule_keeps_the_identity_checks'),
    ('bench_flash.py', '    identity = load_identity(args.identity_rule)',
     '    identity = ' + repr(IDENTITY), 'test_explicit_invalid_rule_never_falls_back'),
    ('bench_flash.py', "        step('confirm-window', device.confirm_window)",
     '        pass', 'test_other_selection_confirms_before_a_resetting_close'),
    ('bench_device.py', '            self.confirm_window()',
     '            pass', 'test_transport_close_holds_after_success_and_reset_error'),
    ('bench_lease.py', "if not name.startswith('GIT_')", 'if True', 'test_git_environment_cannot_redirect_unit_state'),
    ('bench_lease.py', "f'Bench stopped; inspect {self.stop} before any device access'",
     "'Bench stopped; inspect STOP before any device access'", 'test_new_session_after_partial_write_refused'),
    ('reading-ready.py', '            with usb.lease_port(args.port):',
     '            with __import__("contextlib").nullcontext():', 'test_neighbor_journey_holds_usb_lease'),
    ('bench_lease.py', "    return (ROOT / common).resolve() / 'newt-bench'",
     "    return ROOT / '.build/bench'", 'test_git_common_directory_result_is_canonical'),
    ('bench_lease.py', "                self.outside_intact = not pending.exists() and record.get('outside_md5') == 'intact'",
     '                self.outside_intact = False', 'test_unconfirmed_outside_proof_controls_recovery_authority'),
    ('bench_flash.py', "    require(not (session / 'session.pending').exists(), 'Stale session.pending; inspect before backup')\n"
     "    step('usb-mac-security-flash', device.connect)\n    ota, layout",
     "    step('usb-mac-security-flash', device.connect)\n    ota, layout", 'test_stale_session_pending_before_device_or_backup'),
    ('bench_lease.py', '            self.check_clear()', '            pass', 'test_new_session_after_partial_write_refused'),
    ('bench_lease.py', '            self.check_clear()', '            pass', 'test_session_cleanup_cannot_bypass_stop'),
    ('bench_lease.py', '            self.check_clear()', '            pass', 'test_interrupted_write_journal_cannot_bypass_stop'),
    ('bench_flash.py', '    identity = load_identity(args.identity_rule)', "    identity = json_bytes((ROOT / '.build/bench/config.json').read_bytes())", 'test_worktree_without_config_works'),
    ('bench_flash.py', '    identity = load_identity(args.identity_rule)', "    identity = json_bytes((ROOT / '.build/bench/config.json').read_bytes())", 'test_agent_written_config_ignored'),
    ('bench_flash.py', '    identity = load_identity(args.identity_rule)', '    identity = ' + repr(IDENTITY), 'test_missing_or_ambiguous_rule_before_device_calls'),
    ('bench_lease.py', 'require(info.st_uid == 0 and stat.S_ISREG(info.st_mode) and not info.st_mode & 0o022,', 'require(True,', 'test_rule_owner_and_mode_required'),
    ('dev-usb.py', '    with lease_port(args.port):', '    with contextlib.nullcontext():', 'test_flash_and_usb_share_one_nonblocking_lease'),
    ('bench_lease.py', '                if self.write_started:', '                if True:', 'test_prewrite_failures_leave_no_stop'),
    ('bench_flash.py', 'OTADATA_SIZE, start), (end, FLASH)):', 'OTADATA_SIZE, start), (end + 4096, FLASH)):', 'test_before_boundary_bytes_refuse_write'),
    ('bench_flash.py', 'OTADATA_SIZE, start), (end, FLASH)):', 'OTADATA_SIZE, start), (end + 4096, FLASH)):', 'test_during_boundary_bytes_report_failure'),
    ('bench_flash.py', '(OTADATA + OTADATA_SIZE, start),', '(OTADATA + OTADATA_SIZE, start - 1),', 'test_before_boundary_bytes_refuse_write'),
    ('bench_flash.py', "    step('before-outside-md5', outside)\n\n    def write(", '    pass\n\n    def write(',
     'test_before_boundary_bytes_refuse_write'),
    ('bench_flash.py', "        step('after-outside-md5', outside)", '        pass', 'test_during_boundary_bytes_report_failure'),
    ('bench_flash.py', 'and length <= span,', 'and True,', 'test_session_span_refused_before_write'),
    ('bench_flash.py', "require(table is None or backup[0x8000:0x9000] == table, 'Session partition layout differs')", "require(True, 'Session partition layout differs')", 'test_fast_session_partition_table_checked'),
    ('x3_slots.py', "        refuse(len(other) == 1, 'Both slots hold another firmware; neither is Newt\\'s')", '        pass', 'test_two_other_firmwares_refuse_write'),
    ('bench_flash.py', "        step('newt-slot-md5', lambda: verify_md5(device, layout.newt_offset, image))", '        pass', 'test_boot_newt_selects_the_installed_candidate_only'),
    ('bench_flash.py', "    ota, layout = step('slot-identity', lambda: read_layout(device, repair))", "    ota = device.read(OTADATA, OTADATA_SIZE); layout = x3_slots.Layout(0, 1, 1, (x3_slots.Image('other'), x3_slots.Image('newt')))", 'test_unclear_slot_identity_refuses_write'),
    ('bench_flash.py', "    step('reset', device.reset)\n    written", "    step('reset', device.reset)\n    device.close()\n    written", 'test_new_selection_survives_its_first_boot'),
    ('bench_device.py', 'self.esp.WRITE_FLASH_ATTEMPTS = 1', 'pass', 'test_transport_disables_esptool_write_retries'),
    ('bench_device.py', 'esptool.loader.WRITE_BLOCK_ATTEMPTS = 1', 'pass', 'test_transport_disables_esptool_write_retries'),
    ('bench_device.py', 'usb_identity(self.identity)  # Must pass before the port is opened.', 'pass', 'test_usb_guard_precedes_port_open'),
    ('bench_device.py', "require(':'.join(f'{n:02x}' for n in self.esp.read_mac()) == self.identity['mac'], 'MAC differs')", "require(True, 'MAC differs')", 'test_wrong_mac_value_refused'),
    ('bench_device.py', 'require(offset != layout.other_offset, "Never write the other firmware\'s slot")', 'pass', 'test_transport_refuses_writes_outside_newt_slot_and_selection'),
    ('bench_device.py', 'require(offset in (OTADATA, OTADATA + SECTOR) and len(image) == SECTOR,', 'require(True,', 'test_transport_refuses_writes_outside_newt_slot_and_selection'),
    ('bench_flash.py', 'if receipt_path.exists() and backup_path.exists():', 'if receipt_path.exists():', 'test_missing_backup_forces_full'),
]


def negative_controls(controls=CONTROLS, script=__file__):
    for filename, old, new, test in controls:
        with tempfile.TemporaryDirectory(prefix='bench-negative-') as scratch:
            directory = Path(scratch)
            for name in ('bench_candidate.py', 'bench_device.py', 'bench_flash.py', 'bench_lease.py', 'dev-usb.py', 'reading-ready.py', 'x3_slots.py'):
                shutil.copy2(args.tool_root / name, directory / name)
            path = directory / filename
            source = path.read_text()
            if source.count(old) != 1:
                raise RuntimeError('Negative control anchor drift: ' + test)
            # The removed lease needs a harmless context manager only in this mutant.
            if filename == 'dev-usb.py':
                source = 'import contextlib\n' + source
            path.write_text(source.replace(old, new))
            result = subprocess.run([sys.executable, '-B', script, '--tool-root', str(directory), '--single', test], capture_output=True, text=True)
            if result.returncode == 0 or ('FAIL:' not in result.stderr and 'ERROR:' not in result.stderr):
                raise RuntimeError('Guard removal was not caught: ' + test + '\n' + result.stderr)
            print('PASS guard-removal control: ' + test, flush=True)


def main(cases, controls, script):
    suite = unittest.defaultTestLoader.loadTestsFromName(args.single, cases) if args.single else \
        unittest.defaultTestLoader.loadTestsFromTestCase(cases)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():
        sys.exit(1)
    if args.negative_controls:
        negative_controls(controls, script)


if __name__ == '__main__':
    main(BenchTests, CONTROLS, __file__)
