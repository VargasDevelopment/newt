"""Real compiler mutation controls for reusable host objects; no device inputs."""
from concurrent.futures import ThreadPoolExecutor
import json
from contextlib import redirect_stderr, redirect_stdout
import io
import os
import shlex
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import fcntl
import host_objects
from host_objects import HostObjects, compiler_environment
import host_common

COMPILER = host_common.COMPILER
FLAGS = ['-std=c++17', '-O1', '-Wall', '-Wextra', '-Werror', '-pedantic']
# A launcher with the real compiler's version that rewrites its own bytes during
# a link: any invocation that neither compiles, preprocesses nor reports a version.
RELINKING = ('#!/bin/sh\ncase " $* " in\n  *" -c "*|*" -E "*|*" --version "*) ;;\n'
             '  *) printf "# rewritten during a link\\n" >> "$0" ;;\nesac\nexec ' + shlex.quote(COMPILER) + ' "$@"\n')


class ObjectTests(unittest.TestCase):
    def setUp(self):
        mode = patch.dict(os.environ, {'NEWT_HOST_OBJECT_CACHE': '1'})
        mode.start()
        self.addCleanup(mode.stop)
        self.temporary = tempfile.TemporaryDirectory(prefix='newt host object ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.header = self.root / 'value.h'
        self.header.write_text('#define VALUE 7\n')
        self.source = self.root / 'value.cpp'
        self.source.write_text('#include "value.h"\nint value() { return VALUE; }\n')
        self.main = self.root / 'main.cpp'
        self.main.write_text('#include <cstdio>\nint value(); int main() { std::printf("%d\\n",value()); }\n')
        self.include = ['-I', str(self.root)]
        self.cache = HostObjects(self.root, COMPILER)

    def object(self, flags=None, includes=None, cache=None):
        owner = cache or self.cache
        result = owner.object(self.source, flags or FLAGS, self.include if includes is None else includes)
        owner.finish()
        return result

    def observed(self, obj):
        binary = self.root / 'observed'
        subprocess.run([COMPILER, str(self.main), str(obj), '-o', str(binary)], check=True,
                       env=compiler_environment(self.root), cwd=self.root)
        return int(subprocess.run([str(binary)], check=True, capture_output=True, text=True).stdout)

    def test_unchanged_object_is_reused_but_executable_is_still_linked_and_run(self):
        first = self.object()
        self.assertEqual(self.observed(first), 7)
        self.assertEqual(first, self.object())
        self.assertEqual((self.cache.compiled, self.cache.hits), (1, 1))
        self.assertEqual(self.observed(first), 7)

    def test_direct_and_transitive_header_bytes_invalidate(self):
        first = self.object()
        self.header.write_text('#include "nested.h"\n')
        nested = self.root / 'nested.h'
        nested.write_text('#define VALUE 11\n')
        second = self.object()
        self.assertNotEqual(first, second)
        self.assertEqual(self.observed(second), 11)
        nested.write_text('#define VALUE 13\n')
        third = self.object()
        self.assertNotEqual(second, third)
        self.assertEqual(self.observed(third), 13)
        nested.write_text('#define VALUE 13\n// exact dependency bytes changed\n')
        self.assertNotEqual(third, self.object())

    def test_source_and_flags_invalidate(self):
        first = self.object()
        self.source.write_text('#include "value.h"\nint value() { return VALUE+1; }\n')
        second = self.object()
        self.assertNotEqual(first, second)
        self.assertEqual(self.observed(second), 8)
        third = self.object(flags=[*FLAGS, '-O2'])
        self.assertNotEqual(second, third)
        self.source.write_text('#include "value.h"\n#ifndef SCALE\n#define SCALE 1\n#endif\n'
                               'int value() { return VALUE*SCALE; }\n')
        normal = self.object()
        macro = self.object(includes=[*self.include, '-DSCALE=2'])
        self.assertNotEqual(normal, macro)
        self.assertEqual(self.observed(macro), 14)

    def test_include_order_and_shim_bytes_invalidate(self):
        self.source.write_text('#include <value.h>\nint value() { return VALUE; }\n')
        shim = self.root / 'shim'
        shim.mkdir()
        shadow = shim / 'value.h'
        shadow.write_text('#define VALUE 23\n')
        first = self.object()
        second = self.object(includes=['-I', str(shim), *self.include])
        self.assertNotEqual(first, second)
        self.assertEqual(self.observed(second), 23)
        shadow.write_text('#define VALUE 29\n')
        third = self.object(includes=['-I', str(shim), *self.include])
        self.assertNotEqual(second, third)
        self.assertEqual(self.observed(third), 29)

    def test_preprocessor_detects_new_conditional_dependency(self):
        self.source.write_text('#if __has_include("optional.h")\n#include "optional.h"\n'
                               '#else\n#define VALUE 7\n#endif\nint value() { return VALUE; }\n')
        first = self.object()
        (self.root / 'optional.h').write_text('#define VALUE 31\n')
        second = self.object()
        self.assertNotEqual(first, second)
        self.assertEqual(self.observed(second), 31)

    def test_same_version_changed_compiler_launcher_invalidate(self):
        directory = self.root / 'compiler'
        directory.mkdir()
        launcher = directory / 'clang++'
        launcher.write_text('#!/bin/sh\nexec ' + shlex.quote(COMPILER) + ' "$@"\n')
        launcher.chmod(0o700)
        old = HostObjects(self.root, launcher)
        first = self.object(cache=old)
        launcher.write_text('#!/bin/sh\n# changed launcher, same --version\nexec ' + shlex.quote(COMPILER) + ' "$@"\n')
        new = HostObjects(self.root, launcher)
        self.assertEqual(old.identity['version'], new.identity['version'])
        self.assertNotEqual(first, self.object(cache=new))
        with self.assertRaisesRegex(RuntimeError, 'compiler changed'):
            old.finish()

    def test_corrupt_object_or_receipt_cannot_reuse(self):
        first = self.object()
        first.write_bytes(b'replaced compiler output')
        self.assertEqual(first, self.object())
        self.assertEqual(self.observed(first), 7)
        receipt = first.with_suffix('.json')
        record = json.loads(receipt.read_text())
        record['identity']['schema'] = 999
        receipt.write_text(json.dumps(record))
        self.object()
        receipt.write_bytes(b'not json')
        self.object()
        self.assertEqual((self.cache.compiled, self.cache.hits), (4, 0))

    def test_ambient_include_environment_cannot_change_compilation(self):
        self.source.write_text('#include <value.h>\nint value() { return VALUE; }\n')
        bad = self.root / 'ambient'
        bad.mkdir()
        (bad / 'value.h').write_text('#error ambient include path entered compilation\n')
        with patch.dict(os.environ, {'CPATH': str(bad), 'CPLUS_INCLUDE_PATH': str(bad),
                                     'SDKROOT': '/does/not/exist', 'DEVELOPER_DIR': '/does/not/exist'}):
            self.assertEqual(self.observed(self.object()), 7)

    def test_ubsan_errors_are_fatal_despite_ambient_recovery_override(self):
        source = self.root / 'undefined.cpp'
        source.write_text('#include <climits>\nint main() { volatile int value = INT_MAX; '
                          'value += 1; return 0; }\n')
        binary = self.root / 'undefined'
        with patch.dict(os.environ, {'UBSAN_OPTIONS': 'halt_on_error=0'}):
            environment = compiler_environment(self.root)
        self.assertEqual(environment['UBSAN_OPTIONS'], 'halt_on_error=1')
        subprocess.run([COMPILER, '-std=c++17', *host_common.SANITIZERS,
                        str(source), '-o', str(binary)], check=True, env=environment)
        failed = subprocess.run([str(binary)], capture_output=True, text=True, env=environment)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('runtime error: signed integer overflow', failed.stderr)

    @unittest.skipUnless(sys.platform == 'linux', 'Locked Linux Clang function sanitizer')
    def test_newt_function_type_sanitizer_remains_fatal(self):
        source = self.root / 'callback.cpp'
        source.write_text('struct Item { int value; };\n'
                          'void callback(Item *item) { item->value += 1; }\n'
                          'int main() { Item item{0}; '
                          'void (*volatile erased)(void *) = '
                          'reinterpret_cast<void (*)(void *)>(callback); '
                          'erased(&item); return 0; }\n')
        binary = self.root / 'callback'
        environment = compiler_environment(self.root)
        subprocess.run([COMPILER, '-std=c++17', *host_common.SANITIZERS,
                        str(source), '-o', str(binary)], check=True, env=environment)
        failed = subprocess.run([str(binary)], capture_output=True, text=True, env=environment)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('incorrect function type', failed.stderr)

    def test_cache_aliases_are_rejected(self):
        first = self.object()
        first.unlink()
        first.symlink_to(self.source)
        with self.assertRaisesRegex(RuntimeError, 'unaliased'):
            self.object()

    def test_unsupported_external_compiler_inputs_fail_before_dispatch(self):
        for flags, includes in (([*FLAGS, '-fprofile-instr-use=unbound.profdata'], self.include),
                                (FLAGS, ['-include-pch', 'unbound.pch']),
                                (FLAGS, ['@unbound-response']), (FLAGS, ['-I', '@unbound-response']),
                                (FLAGS, ['-I'])):
            with self.subTest(flags=flags, includes=includes):
                with patch('host_objects.subprocess.run') as child:
                    with self.assertRaises(RuntimeError):
                        self.object(flags=flags, includes=includes)
                    child.assert_not_called()

    def test_disabled_reuse_compiles_even_when_exact_object_exists(self):
        first = self.object()
        with patch.dict(os.environ, {'NEWT_HOST_OBJECT_CACHE': '0'}):
            fresh = HostObjects(self.root, COMPILER)
            self.assertEqual(first, self.object(cache=fresh))
            self.assertEqual((fresh.compiled, fresh.hits), (1, 0))
        self.assertEqual(self.observed(first), 7)

    def test_fresh_fixture_bypasses_shared_objects(self):
        (self.root / 'fresh').mkdir()
        self.main.write_text('int main() { return 0; }\n')
        with patch.object(host_common, 'ROOT', self.root), \
             patch.object(host_common, 'BUILD', self.root / '.build'), \
             patch.object(host_common, 'HostObjects', side_effect=AssertionError('shared cache entered fresh fixture')):
            binary = host_common.compile_host(self.main, 'fresh-witness', fresh_directory=self.root / 'fresh')
        # With HostObjects unavailable, its sources prove they bypass the shared cache.
        subprocess.run([str(binary)], check=True)

    def test_fixture_builds_reuse_shared_objects_until_inputs_change(self):
        fixture = self.root / 'fixture-build'
        fixture.mkdir()
        probe = fixture / 'probe.cpp'
        probe.write_text('int value(); int main() { return value() == 7 ? 0 : 1; }\n')
        binary = fixture / 'fixture'
        shared = self.root / 'twice.cpp'
        shared.write_text('#include "value.h"\nint twice() { return 2 * VALUE; }\n')
        command = [COMPILER, *FLAGS, *self.include, str(self.source), str(shared), str(probe), '-o', str(binary)]
        store = self.root / '.build/host-objects'

        def build():
            report = io.StringIO()
            with patch.object(host_common, 'ROOT', self.root), redirect_stderr(report):
                host_common.run_build(command, check=True, cwd=self.root)
            subprocess.run([str(binary)], check=True)
            return report.getvalue().strip()

        def stored():
            return sorted(path.name for path in store.glob('*.o'))
        line = 'HOST objects fixture: reused {}, compiled {}, fixture-local fresh 1; fresh link'
        self.assertEqual(build(), line.format(0, 2))
        first = stored()
        self.assertEqual(len(first), 2)
        self.assertEqual(build(), line.format(2, 0))
        # A probe written beside its fixture compiles fresh and is never stored.
        probe.write_text('int value(); int main() { return value() == 7 ? 0 : 2; }\n')
        self.assertEqual(build(), line.format(2, 0))
        self.assertEqual(stored(), first)
        # It compiles under the shared objects' explicit environment, never the caller's.
        ambient = self.root / 'ambient'
        ambient.mkdir()
        (ambient / 'ambient.h').write_text('#pragma once\n')
        probe.write_text('#if __has_include("ambient.h")\n#error ambient compiler environment\n#endif\n'
                         'int value(); int main() { return value() == 7 ? 0 : 2; }\n')
        with patch.dict(os.environ, {'CPLUS_INCLUDE_PATH': str(ambient)}):
            self.assertEqual(build(), line.format(2, 0))
        # A changed shared source recompiles only its own unit.
        shared.write_text('#include "value.h"\nint twice() { return VALUE + VALUE; }\n')
        self.assertEqual(build(), line.format(1, 1))
        # A changed header recompiles its unit: a stale value would still pass.
        self.header.write_text('#define VALUE 8\n')
        with self.assertRaises(subprocess.CalledProcessError) as failure:
            build()
        self.assertEqual(failure.exception.returncode, 2)
        self.header.write_text('#define VALUE 7\n')
        refused = patch('host_common.HostObjects', side_effect=AssertionError('fixture entered shared cache'))
        with refused, patch.dict(os.environ, {'NEWT_HOST_OBJECT_CACHE': '0'}):
            self.assertEqual(build(), '')
        # An argument role the key does not bind keeps the build fresh.
        command.insert(1, '-Wno-unused-parameter')
        with refused:
            self.assertEqual(build(), '')

    def relinked_builds(self, common):
        """Run compile_host's cached build and a shared fixture build of
        `common` (host_common, or a control variant) through a launcher that
        rewrites itself during the link; return what each raised, or None."""
        for directory in ('fixture',):
            (self.root / directory).mkdir()
        host = self.root / 'host.cpp'
        host.write_text('int main() { return 0; }\n')
        probe = self.root / 'fixture/probe.cpp'
        probe.write_text('int value(); int main() { return value() == 7 ? 0 : 1; }\n')
        launcher = self.root / 'compiler/clang++'
        launcher.parent.mkdir()
        fixture = [str(launcher), *FLAGS, *self.include, str(self.source), str(probe),
                   '-o', str(probe.with_suffix(''))]
        builds = {'compile_host': lambda: common.compile_host(host, 'relinked'),
                  'run_build': lambda: common.run_build(fixture, check=True, cwd=self.root)}
        raised = {}
        for name, build in builds.items():
            launcher.write_text(RELINKING)
            launcher.chmod(0o700)
            with patch.object(common, 'ROOT', self.root), patch.object(common, 'BUILD', self.root / '.build'), \
                 patch.object(common, 'C_COMPILER', str(launcher)), patch.object(common, 'COMPILER', str(launcher)), \
                 redirect_stderr(io.StringIO()):
                try:
                    build()
                    raised[name] = None
                except RuntimeError as error:
                    raised[name] = str(error)
            # The link, and only the link, ran through the launcher and rewrote it.
            self.assertEqual(launcher.read_text(), RELINKING + '# rewritten during a link\n')
        return raised

    def test_a_compiler_changed_during_the_link_fails_the_build(self):
        changed = 'Host compiler changed during object compilation'
        self.assertEqual(self.relinked_builds(host_common), {'compile_host': changed, 'run_build': changed})

    def test_identifying_the_compiler_before_the_link_fails_the_relink_control(self):
        # Negative control: host_common with the concluding identity check before
        # the link, as in 87fd603. Both builds then report success.
        source = Path(host_common.__file__).read_text()
        link = '    host_jobs.run([*link, *map(str, authored), "-o", str(output)], check=True, cwd=ROOT,'
        checked = '        if shared:\n            session.finish()\n        return linked\n'
        for old, new in ((checked, '        return linked\n'),
                         ('        if shared:\n            objects = [',
                          '        if shared:\n            session.finish()\n            objects = ['),
                         ('    session.finish()\n    print(f"HOST objects {name}', '    print(f"HOST objects {name}'),
                         (link, '    session.finish()\n' + link)):
            self.assertEqual(source.count(old), 1, old)
            source = source.replace(old, new)
        control = types.ModuleType('host_common_identified_before_link')
        control.__file__ = host_common.__file__
        exec(compile(source, host_common.__file__, 'exec'), control.__dict__)
        self.assertEqual(self.relinked_builds(control), {'compile_host': None, 'run_build': None})

    def test_concurrent_lookups_of_one_unit_compile_it_once(self):
        caches = [HostObjects(self.root, COMPILER) for _ in range(4)]
        with ThreadPoolExecutor(len(caches)) as pool:
            objects = set(pool.map(lambda cache: cache.object(self.source, FLAGS, self.include), caches))
        self.assertEqual(len(objects), 1)
        self.assertEqual((sum(cache.compiled for cache in caches), sum(cache.hits for cache in caches)), (1, 3))
        self.assertEqual(self.observed(objects.pop()), 7)

    def test_every_lookup_checks_and_publishes_its_unit_under_the_claim(self):
        # Fresh gate phases compile the same units at once. A replacement by one
        # could otherwise fail another's ownership check with an unlinked file.
        def claimed(lock):
            descriptor = os.open(lock, os.O_RDONLY | os.O_CREAT, 0o600)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return False
            except BlockingIOError:
                return True
            finally:
                os.close(descriptor)
        owned, replace, run = host_objects.owned_file, os.replace, subprocess.run
        looked, published = [('check', True)] * 2, [('check', True)] + [('publish', True)] * 2
        # Only reuse needs the compile inside the claim; fresh lookups compile together.
        expected = {'1': looked + [('compile', True)] + published,
                    '0': [('compile', False)] + looked + published}
        for enabled in ('1', '0'):  # Reuse first, so each compiles once.
            states = []

            def record(event, path):
                if Path(path).suffix in ('.o', '.json'):
                    states.append((event, claimed(Path(path).with_suffix('.lock'))))

            def checked(root, path, **options):
                record('check', path)
                return owned(root, path, **options)

            def replaced(source, destination):
                record('publish', destination)
                return replace(source, destination)

            def ran(command, **options):
                if '-c' in command:
                    locks = (self.root / '.build/host-objects').glob('*.lock')
                    states.append(('compile', any(claimed(lock) for lock in locks)))
                return run(command, **options)
            with self.subTest(reuse=enabled), patch.dict(os.environ, {'NEWT_HOST_OBJECT_CACHE': enabled}), \
                 patch.object(host_objects, 'owned_file', side_effect=checked), \
                 patch.object(os, 'replace', side_effect=replaced), \
                 patch.object(subprocess, 'run', side_effect=ran):
                cache = HostObjects(self.root, COMPILER)
                self.assertEqual(self.observed(cache.object(self.source, FLAGS, self.include)), 7)
                self.assertEqual(cache.compiled, 1)
                self.assertEqual(states, expected[enabled])

    def test_build_diagnostics_leave_machine_stdout_clean(self):
        self.main.write_text('int main() { return 0; }\n')
        output = io.StringIO()
        with patch.object(host_common, 'ROOT', self.root), \
             patch.object(host_common, 'BUILD', self.root / '.build'), redirect_stdout(output):
            binary = host_common.compile_host(self.main, 'json-caller')
        self.assertEqual(output.getvalue(), '')
        subprocess.run([str(binary)], check=True)

    def test_compile_host_keeps_extra_c_units_in_c_in_cached_and_fresh_builds(self):
        (self.root / 'extra').mkdir()
        (self.root / 'extra/value.h').write_text('#define EXPECTED 7\n')
        source = self.root / 'extra/value.c'
        source.write_text('#ifdef __cplusplus\n#error C source compiled as C++\n#endif\n'
                          '#include "value.h"\nint value(void) { return EXPECTED; }\n')
        self.main.write_text('#include <value.h>\nextern "C" int value(void);\n'
                             'int main() { return value() == EXPECTED ? 0 : 1; }\n')
        for fresh in (False, True):
            with self.subTest(fresh=fresh), patch.object(host_common, 'ROOT', self.root), \
                 patch.object(host_common, 'BUILD', self.root / '.build'):
                binary = host_common.compile_host(
                    self.main, 'mixed-c-fresh' if fresh else 'mixed-c-cached',
                    fresh_directory=self.root / 'fresh' if fresh else None,
                    extra_sources=(source,), extra_include_dirs=(self.root / 'extra',))
                subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
