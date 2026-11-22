"""Real compiler controls: scratch may not hide in expanded headers or inputs."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_scratch
import host_common
import host_jobs
import host_objects


class ScratchInputTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='compile-scratch-proof-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.source = self.root / 'unit.cpp'
        self.header = self.root / 'checkpoint-root.h'

    @contextmanager
    def gate(self):
        environment = dict(os.environ)
        environment.pop('TMPDIR', None)
        with patch.dict(os.environ, environment, clear=True), gate_scratch.environment(self.root):
            yield

    def rejected(self, flags=(), message='Per-run scratch path'):
        owner = host_objects.HostObjects(self.root, host_common.COMPILER)
        with patch.object(owner, 'compile', wraps=owner.compile) as compile_call:
            with self.assertRaisesRegex(RuntimeError, message):
                owner.object(self.source, ['-std=c++17', '-O1'], list(flags))
            compile_call.assert_not_called()
        self.assertEqual((owner.hits, owner.compiled), (0, 0))
        self.assertFalse(list(owner.directory.glob('*.json')))

    def test_checkpoint_header_contents_and_direct_define_fail_before_compile(self):
        for fresh in ('0', '1'):
            for channel in ('header', 'define', 'expanded'):
                with self.subTest(cache=fresh, channel=channel), self.gate(), \
                     patch.dict(os.environ, {'NEWT_HOST_OBJECT_CACHE': fresh}):
                    value = os.environ.get('TMPDIR') + '/data'
                    self.source.write_text('#include "checkpoint-root.h"\nconst char* root=NEWT_READING_STATE_ROOT;\n')
                    flags = []
                    if channel == 'header':
                        self.header.write_text('#define NEWT_READING_STATE_ROOT ' + json.dumps(value) + '\n')
                    elif channel == 'expanded':
                        self.header.write_text('#define ROOT_PARENT ' + json.dumps(value[:-5]) + '\n'
                                               '#define NEWT_READING_STATE_ROOT ROOT_PARENT "/data"\n')
                    else:
                        self.header.write_text('// supplied by the driver\n')
                        flags = ['-DNEWT_READING_STATE_ROOT=' + json.dumps(value)]
                    self.rejected(flags, 'preprocessed host input')

    def test_canonical_dependency_target_cannot_hide_behind_stable_include_name(self):
        with self.gate():
            target = Path(os.environ.get('TMPDIR')) / 'hidden.h'
            target.write_text('inline constexpr int value=7;\n')
            self.header.symlink_to(target)
            self.source.write_text('#include "checkpoint-root.h"\nint result() { return value; }\n')
            # Preprocessor line markers name the stable link, not its target.
            self.rejected(message='host dependency')

    def test_preexisting_owner_checks_the_active_gate_on_every_lookup(self):
        owner = host_objects.HostObjects(self.root, host_common.COMPILER)
        for _ in range(2):
            with self.gate():
                self.source.write_text('const char* root=' + json.dumps(os.environ.get('TMPDIR')) + ';\n')
                with patch.object(owner, 'compile', wraps=owner.compile) as compile_call:
                    with self.assertRaisesRegex(RuntimeError, 'preprocessed host input'):
                        owner.object(self.source, ['-std=c++17', '-O1'], [])
                    compile_call.assert_not_called()
                self.assertEqual((owner.hits, owner.compiled), (0, 0))

    def test_unused_define_is_still_a_compile_input(self):
        with self.gate():
            self.source.write_text('int value() { return 7; }\n')
            self.rejected(['-DUNUSED=' + json.dumps(os.environ.get('TMPDIR'))], 'compile argument')

    def test_gate_check_precedes_an_existing_cache_hit(self):
        with tempfile.TemporaryDirectory(prefix='warm-scratch-') as scratch:
            self.source.write_text('const char* root=' + json.dumps(scratch + '/data') + ';\n')
            environment = {**os.environ, 'TMPDIR': scratch, 'NEWT_HOST_OBJECT_CACHE': '1'}
            environment.pop(host_jobs.NATIVE_ROOTS, None)
            with patch.dict(os.environ, environment, clear=True):
                standalone = host_objects.HostObjects(self.root, host_common.COMPILER)
                standalone.object(self.source, ['-std=c++17', '-O1'], [])
                self.assertEqual(standalone.compiled, 1)
                with gate_scratch.environment(self.root):
                    owner = host_objects.HostObjects(self.root, host_common.COMPILER)
                    with patch.object(owner, 'compile', wraps=owner.compile) as compile_call:
                        with self.assertRaisesRegex(RuntimeError, 'preprocessed host input'):
                            owner.object(self.source, ['-std=c++17', '-O1'], [])
                        compile_call.assert_not_called()
                    # Removal control: the identical persisted object is a hit.
                    with patch.object(host_objects, 'check_scratch_inputs'):
                        owner.object(self.source, ['-std=c++17', '-O1'], [])
                    self.assertEqual((owner.hits, owner.compiled), (1, 0))

    def test_checkout_inside_caller_tmpdir_is_allowed_but_native_roots_are_not(self):
        with patch.dict(os.environ, {'TMPDIR': str(self.root.parent)}), gate_scratch.environment(self.root):
            self.source.write_text('int value() { return 7; }\n')
            owner = host_objects.HostObjects(self.root, host_common.COMPILER)
            owner.object(self.source, ['-std=c++17', '-O1'], [])
            self.assertEqual(owner.compiled, 1)
            self.source.write_text('const char* root=' + json.dumps(os.environ.get(host_jobs.NATIVE_ROOTS)) + ';\n')
            with self.assertRaisesRegex(RuntimeError, 'preprocessed host input'):
                owner.object(self.source, ['-std=c++17', '-O1'], [])
            self.assertEqual(owner.compiled, 1)


if __name__ == '__main__':
    unittest.main()
