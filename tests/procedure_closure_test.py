"""Every procedure or source identity binds the project Python its procedure
runs. For each fingerprint, the files its real function digests must include
the closure, read statically, of every Python file it covers and of its entry
points: each project file one imports or loads, and for a fingerprint of the
scripts its procedure runs, each project script one starts, however it names
the launcher. procedure_reading.py and procedure_launches.py hold the reading,
which fails closed."""
import ast
import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
from procedure_launches import LAUNCHERS, PROGRAMS, closure, read, wrapping
from procedure_reading import ALLOWED, EXTERNAL, ROOT, SOURCES, TOOLS, relative
sys.path.insert(0, str(TOOLS))
import code_health_resources
import host_objects
import reading_acceptance

# A project helper no fingerprint digests, for controls that start it.
HELPER, HELPER_PATH = 'tools/newt19_review7_dependency.py', "str(ROOT / 'tools/newt19_review7_dependency.py')"


def digested(owner, name, fingerprint):
    """Every file a real fingerprint function digests: call it with the digest
    function owner.name recording each path, then calling through."""
    original, paths = getattr(owner, name), set()

    def recording(path, *arguments, **options):
        paths.add(Path(path).resolve())
        return original(path, *arguments, **options)
    with patch.object(owner, name, recording):
        fingerprint()
    return paths


# Each fingerprint: the tool that defines it, the files its real function
# digests (given that tool's module), its entry points, whether it binds the
# scripts its procedure runs, and the files its receipt binds beside it.
FINGERPRINTS = {
    # Each variant's native build and code-health receipts (procedures_sha256),
    # with the scripts they run. Every lane prepares its composed FreeInk copy.
    **{f'code_health_resources procedures {variant}': (
        'tools/code_health_resources.py', lambda tool, variant=variant: digested(
            tool, 'sha256', lambda: tool.tree_hash(ROOT, tool.procedure_files(ROOT, variant))),
        [], True, ())
       for variant in code_health_resources.VARIANTS},
    # Reading acceptance reports and journeys (procedure_sha256).
    'reading_acceptance procedure': (
        'tools/reading_acceptance.py', lambda tool: digested(
            code_health_resources, 'sha256', lambda: tool.procedure_identity(ROOT)), [], False, ()),
    # Each host object cache record's identity (procedure), part of its key.
    'host_objects procedure': (
        'tools/host_objects.py', lambda tool: digested(tool, 'digest', tool.procedure),
        ['tools/host_objects.py'], False, ()),
}
MODULES = {'tools/code_health_resources.py': code_health_resources,
           'tools/reading_acceptance.py': reading_acceptance, 'tools/host_objects.py': host_objects}
# Negative controls: a module this branch added (and scripts a build runs, by
# name or through a loop), each replaced in a fingerprint's list by a file
# the list already names.
# Reading acceptance also names gate_previews.py and host_compile.py, phase
# scripts the gate runs, outside this import rule; reading_acceptance_test
# checks that a change to either changes its identity.
CONTROLS = [('host_objects procedure', "'host_jobs.py'", "'host_objects.py'", 'tools/host_jobs.py'),
            *((f'code_health_resources procedures {variant}', f'"{name}"', '"native_output.py"', f'tools/{name}')
              for variant in code_health_resources.VARIANTS
              for name in ('native_toolchain.py', 'toolchain-share.py', 'verify-nvs-guard.py',
                           'freeink_sdk_routes.py', 'freeink_sdk_provenance.py',
                           'freeink_source_pin.py', 'daily_firmware.py', 'rust_app_archive.py', 'rust_resource_inputs.py')),
            *(('reading_acceptance procedure', f"'tools/{name}'", "'tools/check.py'", f'tools/{name}')
              for name in ('bench_lease.py', 'bench_candidate.py', 'x3_slots.py', 'dev_usb_panel_telemetry.py', 'dev_usb_firmware_rows.py', 'dev_usb_put.py', 'fatfs_case.py', 'check_scratch.py', 'gate_scratch.py', 'gate_plan.py', 'gate_runner.py', 'gate_tier.py', 'rust_gate_families.py', 'release_tag.py', 'host_jobs.py', 'native_toolchain.py',
                           'toolchain-share.py', 'freeink_sdk_provenance.py', 'freeink_source_pin.py'))]


def rebuilt(defining, old, new):
    """The module of a fingerprint's tool, from its source with old replaced by new once."""
    path = ROOT / defining
    source = path.read_text()
    if source.count(old) != 1:
        raise AssertionError(f'{defining} must name {old} once')
    name = 'rebuilt_' + path.stem.replace('-', '_')
    module = types.ModuleType(name)
    module.__file__ = str(path)
    with patch.dict(sys.modules, {name: module}):
        exec(compile(source.replace(old, new), str(path), 'exec'), module.__dict__)
    return module


def reach(key, module):
    """The files a fingerprint's real function digests, and the closure its procedure reaches."""
    defining, fingerprint, entries, follow_runs, beside = FINGERPRINTS[key]
    files = fingerprint(module)
    covered = [path for path in files if path.suffix == '.py']
    return files, closure([*(ROOT / entry for entry in entries), *covered], follow_runs)


def missing(key, files, reached):
    beside = {(ROOT / name).resolve() for name in FINGERPRINTS[key][4]}
    return sorted(relative(path) for path in reached - files - beside)


def unbound(key, module):
    """The project-relative files a fingerprint's procedure runs that it does not digest."""
    return missing(key, *reach(key, module))


def overlaid(changes):
    """Controls' sources: each project-relative path and the text read in its place."""
    return patch.dict(SOURCES, {(ROOT / name).resolve(): text for name, text in changes.items()})


def appended(name, snippet):
    """A project file's source with snippet appended, and the snippet's last line."""
    text = (ROOT / name).read_text() + snippet
    return text, text.rstrip('\n').count('\n') + 1




def started(name, snippet, sources=()):
    """Overlay a covered file with snippet appended, beside HELPER and any other sources; and the snippet's line."""
    text, line = appended(name, '\n' + snippet + '\n')
    return overlaid({name: text, HELPER: 'STEP = 1\n', **dict(sources)}), line


# Project functions in a covered file that start what their callers pass, and an alias of a launcher.
WRAPPERS = ('tools/python_runtime.py', (ROOT / 'tools/python_runtime.py').read_text() + (
    '\ndef launch(command, **options):\n    return subprocess.run(command, **options)\n'
    '\ndef shell(line):\n    return os.system(line)\n\nopen_process = subprocess.Popen\n'))


class ProcedureClosureTests(unittest.TestCase):
    def test_every_fingerprint_binds_the_project_python_its_procedure_runs(self):
        allowed, external, programs, sites, direct = set(), set(), set(), {}, set()
        for key, (defining, _, _, follow_runs, _) in FINGERPRINTS.items():
            with self.subTest(fingerprint=key):
                files, reached = reach(key, MODULES[defining])
                self.assertEqual(missing(key, files, reached), [])
                for found in map(read, reached):
                    allowed |= found.allowed
                    external |= found.external
                    if follow_runs:
                        programs |= found.programs
                        sites.update(found.sites)
                # Every call of a launcher by its own name in a covered file is a launch site.
                direct |= {(relative(path), node.lineno, ast.unparse(node)) for path in reached if follow_runs
                           for node in ast.walk(ast.parse(path.read_text()))
                           if isinstance(node, ast.Call) and ast.unparse(node.func) in LAUNCHERS}
        # Each exception stands in for a load, run, module or program a covered file has.
        self.assertEqual(allowed, set(ALLOWED))
        self.assertEqual(external, set(EXTERNAL))
        self.assertEqual(programs, set(PROGRAMS))
        # The launch sites, which are not none, are each resolved or reported by an ALLOWED entry.
        self.assertTrue(direct)
        self.assertLessEqual(direct, set(sites))
        self.assertEqual(set(sites.values()), {'resolved', 'allowed'})

    def test_the_closure_reaches_split_modules_through_imports_loads_and_runs(self):
        # The rule itself: host_jobs.py through host_common.py, toolchain-share.py
        # loaded by path, verify-nvs-guard.py run as a script, and
        # make-stage-backdrop.py run through code_health.py's generator loop.
        self.assertIn((TOOLS / 'host_jobs.py').resolve(), closure([TOOLS / 'trusted_host.py'], False))
        build = closure([TOOLS / 'firmware-build.py', TOOLS / 'code_health.py'], True)
        loaded, *runs = ((TOOLS / name).resolve() for name in (
            'toolchain-share.py', 'verify-nvs-guard.py', 'make-stage-backdrop.py'))
        self.assertTrue({loaded, *runs} <= build)
        self.assertFalse(set(runs) & closure([TOOLS / 'firmware-build.py', TOOLS / 'code_health.py'], False))
        # Loads the rule resolves: paths given to a loading function by each
        # call in the file, a constant from another module, and a chain of them.
        for entry, loaded in (('tests/reading_latency_ready_test.py', 'tools/reading-ready.py'),
                              ('tests/reading_latency_ready_test.py', 'tests/reading_latency_test.py'),
                              ('tools/system_ui_contract.py', 'tools/library-redesign-preview.py'),
                              ('tools/make-install-wordmark.py', 'tools/make-stage-backdrop.py')):
            self.assertIn((ROOT / loaded).resolve(), closure([ROOT / entry], False))
        # The project's own wrappers pass their callers' commands on, so each
        # call of one is a launch site, read where it is called.
        for path, name in (('tools/gate_runner.py', 'phase_run'), ('tools/host_jobs.py', 'run')):
            self.assertEqual(wrapping(ROOT / path, name)[1], (('argv', 'command'),))
        check = read(TOOLS / 'check.py').launched()
        calls = {outcome for (_, _, text), outcome in check.sites.items() if text.startswith('phase_run([')}
        self.assertEqual(calls, {'resolved'})
        self.assertIn((TOOLS / 'release_tools.py').resolve(), check.runs)

    def test_a_load_the_rule_cannot_reduce_fails_at_its_location(self):
        # The r6 review's computed name for a module that is not there, a
        # variable name, a parameter no call completes, or one whose function
        # is also passed on, a name bound twice, a file's text run as code, a
        # computed sys.path entry, and a load function under another name.
        for snippet in ("\nimport importlib\nimportlib.import_module('newt19_review_' + 'dependency')\n",
                        '\nimport importlib\nimportlib.import_module(sys.argv[1])\n',
                        '\nimport importlib\ndef load(name):\n    return importlib.import_module(name)\n',
                        "\nimport importlib\nCHOICES = (load('host_jobs'), load)\n"
                        'def load(name):\n    return importlib.import_module(name)\n',
                        "\nimport importlib\nname = 'host_jobs'\nname = name.upper()\nimportlib.import_module(name)\n",
                        "\nexec(Path('tools/host_jobs.py').read_text())\n",
                        "\nsys.path.insert(0, os.environ['NEWT_PATH'])\n",
                        '\nimport importlib\nload = importlib.import_module\n'):
            text, line = appended('tools/native_output.py', snippet)
            with self.subTest(snippet=snippet), overlaid({'tools/native_output.py': text}):
                with self.assertRaisesRegex(AssertionError, rf'(?m)^tools/native_output\.py:{line}: cannot resolve'):
                    unbound('host_objects procedure', host_objects)

    def test_a_load_the_rule_reduces_binds_the_module_it_names(self):
        # Each names tools/reading_sampling.py, which host_objects does not
        # digest: through concatenation, an f-string, a path, a call of a
        # function that loads its parameter, a loop, and code run from a string.
        for snippet in ("\nimport importlib\nimportlib.import_module('reading_' + 'sampling')\n",
                        "\nimport importlib\nPART = 'sampling'\nimportlib.import_module(f'reading_{PART}')\n",
                        "\nimport importlib.util\nimportlib.util.spec_from_file_location(\n"
                        "    'sampling', Path(__file__).resolve().parent / 'reading_sampling.py')\n",
                        "\nimport importlib\ndef load(name):\n    return importlib.import_module(name)\n"
                        "load('reading_' + 'sampling')\n",
                        "\nfor name in ('reading_sampling',):\n    __import__(name)\n",
                        "\nexec('import reading_' + 'sampling')\n"):
            text, _ = appended('tools/native_output.py', snippet)
            with self.subTest(snippet=snippet), overlaid({'tools/native_output.py': text}):
                self.assertEqual(unbound('host_objects procedure', host_objects), ['tools/reading_sampling.py'])
        # The r6 review's case: a computed import of a project helper the
        # fingerprint did not digest, so a change to it escaped.
        text, _ = appended('tools/native_output.py', "\nimport importlib\nreview_dependency = "
                                                   "importlib.import_module('newt19_review_' + 'dependency')\n")
        with overlaid({'tools/native_output.py': text, 'tools/newt19_review_dependency.py': 'STEP = 1\n'}):
            self.assertEqual(unbound('host_objects procedure', host_objects),
                             ['tools/newt19_review_dependency.py'])

    def test_each_way_of_starting_a_process_binds_the_script_it_runs(self):
        # The r7 review's two forms, which the build fingerprint missed, then
        # each other launcher and way to name it or its command: an alias, a
        # from-import, getattr, __import__, executable=, a shell string, the
        # os.exec*, os.spawn*, posix_spawn, asyncio and pty launchers, -m, a
        # list grown after its script, and calls of a project function that
        # starts what its callers pass, by module, from-import and keyword,
        # and launchers reached through another module, project or standard.
        for snippet in (f"subprocess.run(args=['python3', {HELPER_PATH}], check=True)",
                        f"from subprocess import run\nrun(['python3', {HELPER_PATH}])",
                        f"import subprocess as sp\nsp.Popen(['git', {HELPER_PATH}], executable=sys.executable)",
                        f"from subprocess import Popen as P\nP(args=('/usr/bin/python3.12', {HELPER_PATH}))",
                        f"subprocess.check_output(['newt', {HELPER_PATH}], executable=sys.executable)",
                        f"PYTHON = sys.executable\nsubprocess.call([PYTHON, {HELPER_PATH}])",
                        f"import os\nos.system('python3 ' + {HELPER_PATH})",
                        f'from os import system as shell\nshell(f"python3 {{{HELPER_PATH}}}")',
                        f"subprocess.run('python3 ' + {HELPER_PATH}, shell=True)",
                        f"getattr(subprocess, 'check_call')(['python3', {HELPER_PATH}])",
                        f"__import__('subprocess').getoutput('python3 ' + {HELPER_PATH})",
                        f"import shutil\nlaunch = subprocess.Popen\nlaunch([shutil.which('python3'), {HELPER_PATH}])",
                        f"import os\nos.execv(sys.executable, ['python', {HELPER_PATH}])",
                        f"import os\nos.spawnlp(os.P_WAIT, 'python3', 'python3', {HELPER_PATH})",
                        f"import os\nos.posix_spawn(sys.executable, [sys.executable, {HELPER_PATH}], {{}})",
                        f"import asyncio\nasyncio.create_subprocess_exec('python3', {HELPER_PATH})",
                        f"import pty\npty.spawn(['python3', {HELPER_PATH}])",
                        "subprocess.run(['python3', '-I', '-m', 'newt19_review7_dependency'])",
                        f"command = [sys.executable, '-B', {HELPER_PATH}]\ncommand += ['--check']\n"
                        "command.append('--quiet')\nsubprocess.run(command)",
                        f"import python_runtime\npython_runtime.launch(['python3', {HELPER_PATH}], check=True)",
                        f"from python_runtime import launch as start\nstart(command=[sys.executable, {HELPER_PATH}])",
                        f"import python_runtime\npython_runtime.shell('python3 ' + {HELPER_PATH})",
                        "from python_runtime import open_process\n"
                        f"open_process(['git', {HELPER_PATH}], executable='python3')",
                        f"import python_runtime\npython_runtime.subprocess.getoutput('python3 ' + {HELPER_PATH})",
                        f"from python_runtime import subprocess as sp\nsp.run('python3 ' + {HELPER_PATH}, shell=True)",
                        f"import shutil\nshutil.os.system('python3 ' + {HELPER_PATH})",
                        f"import posix\nposix.system('python3 ' + {HELPER_PATH})"):
            overlay, _ = started('tools/code_health.py', snippet, [WRAPPERS])
            with self.subTest(snippet=snippet), overlay:
                self.assertEqual(unbound('code_health_resources procedures regional-dev-usb', code_health_resources), [HELPER])

    def test_a_launch_the_rule_cannot_read_fails_at_its_location(self):
        # An irreducible program, a program outside PROGRAMS, shell syntax,
        # a launcher used other than by a call, one the rule cannot place,
        # one under a computed name, an unknown launcher, options it cannot
        # read, a list changed in place or grown before its script, a method
        # that starts what its callers pass, options passed through a project
        # function, another list naming Python, -m of a runner module, a
        # launcher aliased in a class, a computed function called, executable
        # by position, a launcher bound by a global another scope rebinds, and
        # a star import, whose names the rule cannot trace.
        for snippet in (f"import os\nsubprocess.run([os.environ['PYTHON'], {HELPER_PATH}])",
                        f"subprocess.run(['env', 'python3', {HELPER_PATH}])",
                        f"import os\nos.system('python3 ' + {HELPER_PATH} + ' && true')",
                        "subprocess.run('python3 tools/reading_sampling.py; true', shell=True)",
                        f"import functools\nRUN = functools.partial(subprocess.run, ['python3', {HELPER_PATH}])",
                        "sys.modules['subprocess'].run(['git', 'status'])",
                        "getattr(subprocess, sys.argv[1])(['git', 'status'])",
                        "import os\nos.startfile('newt')",
                        "subprocess.run(['git', 'status'], **OPTIONS)",
                        f"command = ['git']\ncommand[0] = 'python3'\nsubprocess.run(command + [{HELPER_PATH}])",
                        f"command = [sys.executable]\ncommand.append({HELPER_PATH})\nsubprocess.run(command)",
                        "class Launcher:\n    def start(self, command):\n        return subprocess.run(command)",
                        "def git(*arguments, **options):\n    return subprocess.run(['git', *arguments], **options)\n"
                        "git('status', executable=sys.executable)",
                        "COMMAND = ['python3', str(ROOT / 'tools' / os.environ['SCRIPT'])]",
                        "subprocess.run([sys.executable, '-m', 'unittest', 'newt19_review7_dependency'])",
                        "class Tools:\n    run = subprocess.run",
                        f"import os\nos.__dict__['system']('python3 ' + {HELPER_PATH})",
                        f"run = eval('subprocess.run')\nrun(['python3', {HELPER_PATH}])",
                        f"subprocess.Popen(['git', {HELPER_PATH}], -1, sys.executable)",
                        "def choose():\n    global start\n    start = subprocess.run\n"
                        f"start = print\nstart(['python3', {HELPER_PATH}])",
                        "def go():\n    launch = Popen\n"
                        f"    return launch(['git', {HELPER_PATH}], executable='python3')\nfrom subprocess import *"):
            overlay, line = started('tools/code_health.py', snippet)
            with self.subTest(snippet=snippet), overlay:
                with self.assertRaisesRegex(AssertionError, rf'(?m)^tools/code_health\.py:{line}: cannot resolve'):
                    unbound('code_health_resources procedures regional-dev-usb', code_health_resources)

    def test_exceptions_and_runs_stay_exact(self):
        # A function whose parameter the rule takes from its own file's calls
        # must not be called from a file that imports it.
        common, _ = appended('tools/host_common.py', "\nimport importlib\ndef load(name):\n"
                                                     "    return importlib.import_module(name)\nload('host_jobs')\n")
        host, _ = appended('tools/trusted_host.py', "\nhost_common.load('reading_sampling')\n")
        with overlaid({'tools/host_common.py': common, 'tools/trusted_host.py': host}):
            with self.assertRaisesRegex(AssertionError, r"tools/trusted_host\.py may call \['load'\]"):
                closure([TOOLS / 'trusted_host.py'], False)
        # An ALLOWED entry holds at its own line only.
        test = ROOT / 'tests/host_objects_test.py'
        with overlaid({'tests/host_objects_test.py': '\n' + test.read_text()}):
            with self.assertRaisesRegex(AssertionError, r'(?m)^tests/host_objects_test\.py:332: cannot resolve'):
                closure([test], False)
        # A build's runs: an unreduced one fails only where runs are followed,
        # and a reduced script, -m module or -c code the fingerprint does not
        # digest is unbound, as is a script a plain shell string runs.
        key = 'code_health_resources procedures regional-dev-usb'
        text, line = appended('tools/code_health.py', "\nsubprocess.run([sys.executable, '-B', "
                                                      "str(ROOT / 'tools' / os.environ['SCRIPT'])])\n")
        with overlaid({'tools/code_health.py': text}):
            with self.assertRaisesRegex(AssertionError, rf'(?m)^tools/code_health\.py:{line}: cannot resolve'):
                unbound(key, code_health_resources)
            closure([TOOLS / 'code_health.py'], False)
        for command in ("[sys.executable, '-B', str(ROOT / 'tools/reading_sampling.py')]",
                        "[sys.executable, '-I', '-m', 'reading_sampling']",
                        "[sys.executable, '-c', 'import reading_sampling']",
                        "'python3 tools/reading_sampling.py', shell=True"):
            text, _ = appended('tools/code_health.py', f'\nsubprocess.run({command})\n')
            with self.subTest(command=command), overlaid({'tools/code_health.py': text}):
                self.assertEqual(unbound(key, code_health_resources), ['tools/reading_sampling.py'])

    def test_a_module_dropped_from_a_fingerprint_list_fails_the_check(self):
        for key, old, new, dropped in CONTROLS:
            with self.subTest(fingerprint=key, dropped=dropped):
                module = rebuilt(FINGERPRINTS[key][0], old, new)
                self.assertEqual(unbound(key, module), [dropped])


if __name__ == '__main__':
    unittest.main()
