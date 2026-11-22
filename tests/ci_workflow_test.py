"""CI scope, exact source, failure and fresh-execution controls; no hosted actions."""
import json
import ast
import hashlib
from contextlib import redirect_stdout
import io
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import ci_environment as setup
import ci_gate as gate
import check as host_check
import release_tag
from release_tools import release_python
from workflow_expression import PRIVATE, PUBLIC, trust_expression, where

ROOT = Path(__file__).resolve().parents[1]


class WorkflowControls(unittest.TestCase):
    def workflow(self):
        # Use the already required pinned release parser, not a new dependency.
        probe = 'import json,yaml,sys; print(json.dumps(yaml.load(open(sys.argv[1]),Loader=yaml.BaseLoader)))'
        return json.loads(subprocess.run([str(release_python()), '-I', '-B', '-c', probe,
                          str(ROOT / '.github/workflows/fresh-gates.yml')],
                          check=True, capture_output=True, text=True).stdout)

    def runtime_step(self):
        return next(step for step in self.workflow()['jobs']['fresh-gates']['steps']
                    if 'uv --no-config python install' in step.get('run', ''))

    def runtime_probe(self):
        return self.runtime_step()['run'].split("<<'PYTHON'\n", 1)[1].split('\nPYTHON\n', 1)[0]

    def test_prerequisite_failure_stops_every_development_lane(self):
        class PrerequisiteFailure(RuntimeError):
            pass

        for suite in ('all', 'quick', 'ui', 'cache'):
            with self.subTest(suite=suite):
                calls = []
                def phase(command, **kwargs):
                    calls.append(command)
                    if Path(command[-1]).name == 'release_tools_test.py':
                        raise PrerequisiteFailure('authored prerequisite failure')
                arguments = ['check.py', '--suite', suite]
                if sys.platform == 'linux':
                    arguments.extend(['--profile', 'linux'])
                with patch.object(sys, 'argv', arguments), \
                        patch.object(host_check, 'verify_source_pin'), \
                        patch.object(host_check, 'phase_run', side_effect=phase), \
                        patch.object(host_check, 'focused') as focused:
                    with self.assertRaises(PrerequisiteFailure):
                        host_check.main()
                    focused.assert_not_called()
                self.assertEqual([Path(command[-1]).name for command in calls],
                                 ['release_tools.py', 'release_tools_test.py'])

    def test_workflow_scope_permissions_pins_cache_and_fresh_commands(self):
        workflow = self.workflow()
        tools, release = setup.settings(ROOT, 'linux-x86_64')
        self.assertEqual(set(workflow['on']), {'push', 'schedule', 'workflow_dispatch'})
        self.assertEqual(len(workflow['on']['schedule']), 1)  # The nightly full tier.
        self.assertEqual(workflow['on']['push']['branches'], ['main'])
        self.assertEqual(workflow['permissions'], {'contents': 'read'})
        self.assertEqual(list(workflow['jobs']), ['release-check', 'fresh-gates', 'release-tag'])
        job = workflow['jobs']['fresh-gates']
        self.assertEqual(job['runs-on'], tools['runner'])
        self.assertEqual(job['timeout-minutes'], '120')
        self.assertNotIn('DEVELOPER_DIR', job['env'])
        self.assertNotIn('SDKROOT', job['env'])
        self.assertEqual(job['env']['OPENSSL_CONF'], '/dev/null')
        self.assertEqual(job['env']['EXPECTED_COMMIT'], '${{ inputs.approved_commit || github.sha }}')
        self.assertNotIn('pull_request_target', workflow['on'])
        allowed = trust_expression(job['if'])
        accepted = {'repository': 'VargasDevelopment/newt', 'ref': 'refs/heads/main',
                    'actor': 'VargasDevelopment', 'event_name': 'push', **PRIVATE}
        self.assertTrue(allowed(accepted))
        self.assertTrue(allowed({**accepted, 'event_name': 'workflow_dispatch'}))
        self.assertTrue(allowed(where(accepted, 'schedule')))
        for key, bad in [('repository', 'untrusted/fork'), ('ref', 'refs/heads/topic'),
                         ('actor', 'untrusted-user'), ('event_name', 'pull_request'),
                         ('event_name', 'pull_request_target')]:
            self.assertFalse(allowed({**accepted, key: bad}), (key, bad))
        # Never in the public repository, whatever its name (public-ci.yml runs there).
        for event_name in ('push', 'workflow_dispatch', 'schedule'):
            self.assertFalse(allowed(where({**accepted, **PUBLIC}, event_name)), event_name)
        self.assertFalse(allowed({**accepted, 'event.repository.private': None}))
        for step in job['steps']:
            self.assertNotIn('if', step)
            self.assertNotIn('continue-on-error', step)
            if 'uses' in step:
                name, revision = step['uses'].split('@')
                self.assertEqual(revision, tools['actions'][name])
                if name == 'actions/checkout':
                    self.assertEqual(step['with']['persist-credentials'], 'false')
                self.assertNotEqual(name, 'actions/setup-python')
        # The job's start, before anything else, so the gate's budget covers setup.
        self.assertEqual(job['steps'][0]['run'], 'echo "NEWT_JOB_STARTED=$(date +%s)" >> "$GITHUB_ENV"')
        checkout = job['steps'][1]['with']
        self.assertEqual(checkout['ref'], '${{ env.EXPECTED_COMMIT }}')
        self.assertEqual(checkout['fetch-depth'], '0')
        self.assertEqual(checkout['fetch-tags'], 'true')
        trust = job['steps'][2]
        self.assertEqual(trust['shell'], 'bash')
        self.assertIn('^[0-9a-f]{40}$', trust['run'])
        self.assertIn('git merge-base --is-ancestor "$EXPECTED_COMMIT" refs/remotes/origin/main', trust['run'])
        # Execute the real workflow shell against authored Git branches.
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.run(['git', *args], cwd=directory, check=True,
                                      capture_output=True, text=True).stdout.strip()
            git('init', '-q'); git('config', 'user.name', 'CI fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            git('config', 'core.hooksPath', directory + '/no-hooks')
            git('commit', '--allow-empty', '-qm', 'chore: trusted main')
            main = git('rev-parse', 'HEAD')
            git('update-ref', 'refs/remotes/origin/main', main)
            git('commit', '--allow-empty', '-qm', 'chore: unapproved topic')
            topic = git('rev-parse', 'HEAD')
            for head, expected, admitted in [(main, main, True), (topic, topic, False),
                                              (main, topic, False), (main, 'main', False)]:
                git('checkout', '--detach', '-q', head)
                result = subprocess.run(['bash', '-c', trust['run']], cwd=directory,
                                        env={'PATH': '/usr/bin:/bin', 'EXPECTED_COMMIT': expected},
                                        capture_output=True)
                self.assertEqual(result.returncode == 0, admitted)
        # Raw archives, toolchains and objects persist in tools/ci_persist.py's store, not a hosted cache.
        self.assertFalse(any(s.get('uses', '').startswith('actions/cache@') for s in job['steps']))
        preflight = next(i for i, s in enumerate(job['steps']) if s.get('run', '').endswith('--check-host'))
        self.assertLess(2, preflight)
        self.assertEqual(preflight, len(job['steps']) - 3)
        self.assertEqual(job['steps'][-2]['run'], 'python3 -B tools/ci_persist.py --expected-commit "$EXPECTED_COMMIT"')
        self.assertEqual(job['steps'][-1]['run'], 'python3 -B tools/ci_gate.py --expected-commit "$EXPECTED_COMMIT"')

    def test_only_the_release_tag_job_writes_and_only_after_its_push_gate_passed(self):
        workflow = self.workflow()
        tools, _ = setup.settings(ROOT, 'linux-x86_64')
        gate, job = workflow['jobs'][release_tag.GATES['fresh-gates.yml']], workflow['jobs']['release-tag']
        # The one write grant is on the small job; the gate job keeps the read-only default.
        self.assertEqual(workflow['permissions'], {'contents': 'read'})
        self.assertNotIn('permissions', gate)
        self.assertEqual(job['permissions'], {'actions': 'read', 'contents': 'write'})
        self.assertEqual(job['needs'], release_tag.GATES['fresh-gates.yml'])
        self.assertEqual(job['runs-on'], gate['runs-on'])
        self.assertEqual(job['env'], {'EXPECTED_COMMIT': gate['env']['EXPECTED_COMMIT']})
        self.assertNotIn('continue-on-error', job)
        allowed = trust_expression(job['if'])
        accepted = {'success': True, 'repository': 'VargasDevelopment/newt', 'ref': 'refs/heads/main',
                    'actor': 'VargasDevelopment', 'event_name': 'push', **PRIVATE}
        self.assertTrue(allowed(accepted))
        self.assertFalse(allowed({**accepted, **PUBLIC}))
        # A manual run never tags: its head_sha can differ from the approved_commit its gate checked.
        for key, bad in [('success', False), ('event_name', 'workflow_dispatch'), ('repository', 'untrusted/fork'),
                         ('ref', 'refs/heads/topic'), ('actor', 'untrusted-user'), ('event_name', 'pull_request')]:
            self.assertFalse(allowed({**accepted, key: bad}), (key, bad))
        checkout, trust, tag = job['steps']
        self.assertEqual(trust, gate['steps'][2])
        self.assertEqual(checkout['uses'], f"actions/checkout@{tools['actions']['actions/checkout']}")
        self.assertEqual(checkout['with'], {'ref': '${{ env.EXPECTED_COMMIT }}', 'fetch-depth': '0',
                                            'fetch-tags': 'true', 'persist-credentials': 'false',
                                            'set-safe-directory': 'false'})
        self.assertEqual(tag['shell'], 'bash')
        self.assertEqual(tag['run'].splitlines()[-1],
                         'python3 -B tools/release_tag.py --ci-run "$GITHUB_RUN_ID" "$EXPECTED_COMMIT"')
        self.assertEqual(tag['env'], {
            'GH_TOKEN': '${{ github.token }}', 'GIT_COMMITTER_NAME': 'github-actions[bot]',
            'GIT_COMMITTER_EMAIL': '41898282+github-actions[bot]@users.noreply.github.com'})
        for name, each in workflow['jobs'].items():
            for step in each['steps']:
                self.assertNotIn('if', step)
                self.assertNotIn('continue-on-error', step)
                text = json.dumps(step)
                self.assertEqual('github.token' in text or 'secrets.' in text, step is tag, (name, step.get('name')))

    def test_a_push_never_cancels_a_release_merge_run(self):
        """Only push and nightly runs share the cancelling group; a release merge's push run
        (gate_tier.release_push) has a group of its own run, so it always finishes and tags."""
        workflow = self.workflow()
        check, gate, tag = (workflow['jobs'][name] for name in ('release-check', 'fresh-gates', 'release-tag'))
        self.assertNotIn('concurrency', workflow)  # A workflow-wide group would cancel the release run.
        self.assertNotIn('concurrency', check)
        self.assertNotIn('concurrency', tag)
        self.assertEqual(gate['needs'], 'release-check')
        group = ("${{ needs.release-check.outputs.release == 'true' && format('newt-release-{0}', github.run_id)"
                 " || format('newt-{0}-{1}', github.workflow, github.ref) }}")
        self.assertEqual(gate['concurrency'], {'group': group, 'cancel-in-progress': 'true'})
        self.assertEqual((check['if'], check['env']), (gate['if'], {'EXPECTED_COMMIT': gate['env']['EXPECTED_COMMIT']}))
        # On the one self-hosted runner, a push's classifier would wait behind the run it must cancel.
        self.assertEqual(check['runs-on'], 'ubuntu-latest')
        self.assertNotIn('permissions', check)  # Only the read-only default token.
        self.assertEqual(check['outputs'], {'release': '${{ steps.release.outputs.release }}'})
        checkout, trust, decide = check['steps']
        self.assertEqual(checkout['uses'], gate['steps'][1]['uses'])
        self.assertEqual(checkout['with'], {'ref': '${{ env.EXPECTED_COMMIT }}', 'fetch-depth': '0',
                                            'persist-credentials': 'false', 'set-safe-directory': 'false'})
        self.assertEqual(trust, gate['steps'][2])
        self.assertEqual(decide['id'], 'release')
        # set -e: a failed decision fails the run instead of making it cancellable.
        self.assertEqual(decide['run'].splitlines(), [
            'set -euo pipefail',
            'release="$(python3 -B tools/gate_tier.py --release-push "$EXPECTED_COMMIT")"',
            'echo "release=$release" >> "$GITHUB_OUTPUT"'])

    def test_the_tag_step_neither_stores_its_token_nor_takes_an_inherited_credential(self):
        """Runs the tag step's real script with a store helper in the runner's own config."""
        tag = self.workflow()['jobs']['release-tag']['steps'][2]
        script, step_env = tag['run'], {k: v for k, v in tag['env'].items() if '${{' not in v}
        def run(step, stored):
            with tempfile.TemporaryDirectory() as scratch:
                home, bin_dir = Path(scratch, 'home'), Path(scratch, 'bin')
                home.mkdir()
                bin_dir.mkdir()
                (home / '.gitconfig').write_text('[credential]\n\thelper = store\n')
                store = home / '.git-credentials'
                store.write_text('https://runner:stored-credential@github.com\n' if stored else '')
                gh, python = bin_dir / 'gh', bin_dir / 'python3'
                gh.write_text('#!/bin/sh\n[ "$*" = "auth git-credential get" ] || exit 0\n'
                              'printf "username=x-access-token\\npassword=%s\\n" "$GH_TOKEN"\n')
                # Stands in for release_tag.py: git asks for github.com's credential, then approves it after a push.
                python.write_text('#!/bin/sh\nset -e\nanswer=$(printf "protocol=https\\nhost=github.com\\n\\n" | git credential fill)\n'
                                  'printf "%s\\n\\n" "$answer" | git credential approve\nprintf "%s\\n" "$answer"\n')
                gh.chmod(0o755)
                python.chmod(0o755)
                env = {'PATH': f'{bin_dir}:/usr/bin:/bin', 'HOME': str(home), 'GIT_CONFIG_NOSYSTEM': '1',
                       'GIT_TERMINAL_PROMPT': '0', 'GH_TOKEN': 'job-token', 'GITHUB_RUN_ID': '1',
                       'EXPECTED_COMMIT': '0' * 40, **step_env}
                answer = subprocess.run(['bash', '-c', step], cwd=scratch, env=env, check=True,
                                        capture_output=True, text=True).stdout.splitlines()
                return answer, store.read_text()
        for stored in (True, False):
            answer, store = run(script, stored)
            self.assertIn('password=job-token', answer, stored)
            self.assertNotIn('job-token', store, stored)
        # Control: with gh's helper only, an inherited store answers first or keeps the job token.
        unsafe = script.replace('GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= \\\n'
                                '  GIT_CONFIG_KEY_1=credential.helper GIT_CONFIG_VALUE_1=',
                                'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0=')
        self.assertNotEqual(unsafe, script)
        self.assertIn('password=stored-credential', run(unsafe, True)[0])
        self.assertIn('job-token', run(unsafe, False)[1])

    def test_managed_python_pins_workspace_paths_and_real_runtime_probe(self):
        job = self.workflow()['jobs']['fresh-gates']
        tools, release = setup.settings(ROOT, 'linux-x86_64')
        bootstrap = tools['python_bootstrap']
        self.assertEqual(bootstrap['manager'], 'uv')
        self.assertEqual(bootstrap['version'], '0.12.21')
        self.assertEqual(bootstrap['manager_sha256'], '23f02075b652bb1df64178cfae41b5caf160822e720e2663568f3f5d63bc52c0')
        self.assertEqual(bootstrap['request'], 'cpython-3.12.6-linux-x86_64-gnu')
        self.assertEqual(bootstrap['mirror'], 'https://github.com/astral-sh/python-build-standalone/releases/download')
        self.assertEqual(bootstrap['archive_build'], '20240909')
        self.assertEqual(bootstrap['archive_sha256'], 'b080463e4f0c452e592cdac1ca97936a6a19bb3d9a64da669a50ca843fce0108')
        self.assertEqual(bootstrap['archive_size'], 21243376)
        self.assertEqual(release['python'], '3.12.6')
        for key, suffix in (('RUNNER_TOOL_CACHE', 'tool-cache'), ('UV_CACHE_DIR', 'cache'),
                            ('UV_PYTHON_INSTALL_DIR', 'python'), ('UV_PYTHON_BIN_DIR', 'bin'),
                            ('UV_TOOL_DIR', 'tools'), ('UV_TOOL_BIN_DIR', 'tool-bin')):
            self.assertEqual(job['env'][key], '${{ github.workspace }}/.build/python-bootstrap/' + suffix)
        manager = next(step for step in job['steps'] if step.get('uses', '').startswith('astral-sh/setup-uv@'))
        self.assertEqual(manager['uses'], 'astral-sh/setup-uv@c18668ad3cf93ea998bef934396af7bb5c839dc7')
        self.assertEqual(manager['with'], {'version': bootstrap['version'], 'checksum': bootstrap['manager_sha256'],
                         'download-from-astral-mirror': 'false', 'enable-cache': 'false', 'cache-python': 'false',
                         'tool-dir': '${{ env.UV_TOOL_DIR }}', 'tool-bin-dir': '${{ env.UV_TOOL_BIN_DIR }}'})
        runtime = self.runtime_step()
        shell = runtime['run']
        self.assertEqual(runtime['shell'], 'bash')
        self.assertIn('set -euo pipefail', shell)
        self.assertIn('test ! -e "$UV_PYTHON_INSTALL_DIR"', shell)
        self.assertIn('request=' + bootstrap['request'], shell)
        self.assertIn(bootstrap['archive_sha256'], shell)
        self.assertIn('uv --no-config python install --no-bin "$request"', shell)
        self.assertIn('--mirror ' + bootstrap['mirror'], shell)
        self.assertIn('uv --no-config python find --managed-python --no-python-downloads "$request"', shell)
        self.assertIn('"$python_path" -I -B -', shell)
        self.assertIn('test -x "$(dirname "$python_path")/python3"', shell)
        self.assertIn('echo "$(dirname "$python_path")" >> "$GITHUB_PATH"', shell)
        self.assertNotIn('||', shell)
        probe = self.runtime_probe()
        syntax = ast.parse(probe)
        imports = {alias.name for node in syntax.body if isinstance(node, ast.Import) for alias in node.names}
        self.assertTrue({'bz2', 'ctypes', 'lzma', 'sqlite3', 'ssl', 'venv', 'zlib'} <= imports)
        self.assertIn('assert sys.version_info[:3] == (3, 12, 6)', probe)
        self.assertIn("pathlib.Path(os.environ['UV_PYTHON_INSTALL_DIR']).resolve()", probe)
        self.assertIn('assert pathlib.Path(sys.executable).resolve().is_relative_to(root)', probe)
        self.assertIn('venv.EnvBuilder(with_pip=True, symlinks=True).create(probe)', probe)
        runs = [node for node in ast.walk(syntax) if isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute) and isinstance(node.func.value, ast.Name)
                and node.func.value.id == 'subprocess' and node.func.attr == 'run']
        self.assertEqual(len(runs), 1)
        self.assertTrue(any(keyword.arg == 'check' and isinstance(keyword.value, ast.Constant)
                            and keyword.value.value is True for keyword in runs[0].keywords))
        subprocess.run(['bash', '-n', '-c', shell], check=True, capture_output=True)
        self.assertLess(1, job['steps'].index(manager))
        self.assertLess(job['steps'].index(manager), job['steps'].index(runtime))
        preflight = next(step for step in job['steps'] if step.get('run', '').endswith('--check-host'))
        self.assertLess(job['steps'].index(runtime), job['steps'].index(preflight))

    def test_managed_python_install_or_discovery_failure_never_registers_path(self):
        # Execute the workflow's actual shell, stopping before any runtime launch.
        # These authored failures never acquire or simulate a Linux installation.
        shell = self.runtime_step()['run']
        for phase, code in (('install', 61), ('find', 62)):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                workspace = Path(directory)
                bin_dir = workspace / 'authored-bin'
                bin_dir.mkdir()
                uv = bin_dir / 'uv'
                uv.write_text('#!/bin/bash\n'
                              'printf "%s\\n" "$*" >> "$BOOTSTRAP_CALLS"\n'
                              f'if [[ "$*" == *"python {phase}"* ]]; then exit {code}; fi\n'
                              'exit 0\n')
                uv.chmod(0o755)
                environment = {'PATH': str(bin_dir) + ':/usr/bin:/bin', 'GITHUB_WORKSPACE': directory,
                               'UV_PYTHON_INSTALL_DIR': str(workspace / 'managed-python'),
                               'GITHUB_PATH': str(workspace / 'registered-path'),
                               'BOOTSTRAP_CALLS': str(workspace / 'calls')}
                failed = subprocess.run(['bash', '-c', shell], cwd=workspace, env=environment,
                                        capture_output=True, text=True)
                self.assertEqual(failed.returncode, code)
                self.assertEqual(len((workspace / 'calls').read_text().splitlines()), 1 if phase == 'install' else 2)
                self.assertFalse((workspace / 'registered-path').exists())

    def test_managed_python_probe_rejects_a_broken_venv(self):
        # Use an already installed actual 3.12.6 for the probe's stdlib assertions.
        # Only the venv failure is authored; this is not a Linux bootstrap run.
        python = sys.executable if sys.version_info[:3] == (3, 12, 6) else shutil.which('python3.12')
        if not python:
            self.skipTest('No already approved local Python 3.12.6 for the runtime failure control')
        actual = subprocess.run([python, '-I', '-B', '-c', 'import sys; print(sys.version_info[:3])'],
                                check=True, capture_output=True, text=True).stdout.strip()
        if actual != '(3, 12, 6)':
            self.skipTest('The existing local Python is not exactly 3.12.6')
        driver = '''import pathlib, sys, venv
from unittest.mock import patch
class BrokenVenv:
    def __init__(self, **kwargs):
        pass
    def create(self, path):
        binary = pathlib.Path(path) / 'bin/python'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\\nexit 73\\n')
        binary.chmod(0o755)
with patch.object(venv, 'EnvBuilder', BrokenVenv):
    exec(compile(sys.argv[1], '<actual workflow Python probe>', 'exec'))
'''
        with tempfile.TemporaryDirectory() as directory:
            environment = {'PATH': os.defpath, 'GITHUB_WORKSPACE': directory,
                           'UV_PYTHON_INSTALL_DIR': str(Path(python).resolve().parent)}
            failed = subprocess.run([python, '-I', '-B', '-c', driver, self.runtime_probe()], env=environment,
                                    capture_output=True, text=True)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('exit status 73', failed.stderr)
        self.assertNotIn('PASS exact workspace-local Python', failed.stdout)

    def test_setup_plan_is_pinned_and_local_prepare_rejected(self):
        tools, release = setup.settings(ROOT, 'macos-arm64')
        self.assertEqual(len(release['artifacts']), 16)
        self.assertEqual(tools['llvm']['sha256'], 'f260f4f7c0d430828a81ae8a3826a1d63fc0963ec2459489308cc23b1f7eab4f')
        with patch.dict(setup.os.environ, {}, clear=True), patch.object(setup, 'acquire') as acquire:
            with self.assertRaisesRegex(RuntimeError, 'hosted CI only'):
                setup.prepare(ROOT)
            acquire.assert_not_called()
        with patch.dict(setup.os.environ, {'GITHUB_ACTIONS': 'true'}, clear=True), \
             patch.object(setup.platform, 'system', return_value='Linux'):
            with self.assertRaisesRegex(RuntimeError, 'macOS ARM64'):
                setup.require_host(tools)
        command = setup.installation_command(Path('/fixture/python'), [Path('/fixture/wheel.whl')])
        self.assertTrue({'--isolated', '--no-index', '--no-deps', '--no-compile'}.issubset(command))
        self.assertTrue(setup.llvm_member(PurePosixPath('bin/clang-tidy'), 'file'))
        self.assertTrue(setup.llvm_member(PurePosixPath('lib/libLLVM.dylib'), 'file'))
        self.assertFalse(setup.llvm_member(PurePosixPath('bin/clang'), 'file'))
        self.assertFalse(setup.llvm_member(PurePosixPath('lib/libLLVM.a'), 'file'))

    def test_llvm_runtime_retains_builtin_tree_and_requires_headers(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name).resolve()
            archive = root / 'llvm.tar'
            headers = setup.LLVM_HEADERS
            included = ['bin/clang-tidy', 'lib/libLLVM.dylib',
                        *[str(headers / n) for n in ('stddef.h', 'stdint.h', 'stdarg.h', '__stddef_size_t.h')]]
            excluded = ['include/clang/Tooling/Tooling.h', 'lib/clang/21/include/stddef.h',
                        'bin/clang', 'lib/libLLVM.a']
            with tarfile.open(archive, 'w') as handle:
                for path in included + excluded:
                    item = tarfile.TarInfo('LLVM-22.1.8/' + path)
                    item.size = len(b'fixture')
                    handle.addfile(item, io.BytesIO(b'fixture'))
            destination = root / 'llvm'
            setup.extract(root, archive, destination, select=setup.llvm_member)
            self.assertEqual(sorted(str(p.relative_to(destination)) for p in destination.rglob('*') if p.is_file()),
                             sorted(included))
            setup.require_llvm_resources(root, destination)
            for missing in ('stddef.h', 'stdarg.h'):
                header = destination.joinpath(*headers.parts) / missing
                header.unlink()
                with self.assertRaisesRegex(RuntimeError, 'builtin resource header is missing'):
                    setup.require_llvm_resources(root, destination)
                header.write_bytes(b'fixture')

    def test_mutated_release_requirements_rejected(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name).resolve()
            shutil.copytree(ROOT / '.github', root / '.github')
            shutil.copyfile(ROOT / 'requirements-release.txt', root / 'requirements-release.txt')
            (root / 'requirements-release.txt').write_text('commitizen==0.0.0\n')
            with self.assertRaisesRegex(RuntimeError, 'wheel lock differs'):
                setup.settings(root)

    def test_linux_selection_pins_real_crypto_and_retains_sanitizer_runtime(self):
        tools, release = setup.settings(ROOT, 'linux-x86_64')
        self.assertEqual(tools['runner'], ['self-hosted', 'linux', 'x64', 'newt'])
        self.assertEqual(tools['openssl']['version'], '3.5.9')
        self.assertEqual(len(release['artifacts']), 16)
        self.assertTrue(setup.linux_llvm_member(PurePosixPath('bin/clang++'), 'symlink'))
        self.assertTrue(setup.linux_llvm_member(PurePosixPath('bin/clang-22'), 'file'))
        self.assertTrue(setup.linux_llvm_member(PurePosixPath('lib/libclang-cpp.so.22.1'), 'file'))
        self.assertTrue(setup.linux_llvm_member(setup.LLVM_HEADERS.parent / 'lib/linux/libclang_rt.asan-x86_64.a', 'file'))
        self.assertFalse(setup.linux_llvm_member(PurePosixPath('bin/llvm-ar'), 'file'))
        with patch.dict(setup.os.environ, {'GITHUB_ACTIONS': 'false'}, clear=True), \
             patch.object(setup, 'acquire') as acquire:
            with self.assertRaisesRegex(RuntimeError, 'hosted CI only'):
                setup.require_host(tools)
            acquire.assert_not_called()

    def test_linux_crypto_preparation_rejects_preexisting_trees_before_acquisition(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / '.toolchain/openssl').mkdir(parents=True)
            tools, _ = setup.settings(ROOT, 'linux-x86_64')
            with patch.object(setup, 'acquire') as acquire:
                with self.assertRaisesRegex(RuntimeError, 'freshly materialized'):
                    setup.prepare_openssl(root, tools['openssl'], root / 'cache', root / 'llvm')
                acquire.assert_not_called()

    def test_pinned_job_xcode_cannot_hide_different_canonical_compiler(self):
        tools, _ = setup.settings(ROOT, 'macos-arm64')
        pinned = Path(tools['xcode'])
        stdout = f"Xcode {tools['xcode_version']}\nBuild version {tools['xcode_build']}\n"
        for actual, accepted in ((pinned / 'Toolchains/default/usr/bin/clang', True),
                                 (Path('/Applications/Other.app/Contents/Developer/bin/clang'), False)):
            with patch.dict(setup.os.environ, {'GITHUB_ACTIONS': 'true', 'DEVELOPER_DIR': str(pinned)}, clear=True), \
                 patch.object(setup.platform, 'system', return_value='Darwin'), \
                 patch.object(setup.platform, 'machine', return_value='arm64'), \
                 patch.object(setup.platform, 'python_version', return_value=tools['python']), \
                 patch.object(Path, 'is_dir', return_value=True), \
                 patch.object(Path, 'resolve', return_value=pinned), \
                 patch.object(setup.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, stdout)), \
                 patch.object(setup, 'compiler_environment', return_value={'PATH': '/usr/bin'}), \
                 patch.object(setup, 'compiler_identity', return_value={'executable_path': str(actual)}) as compiler:
                if accepted:
                    setup.require_host(tools)
                    self.assertEqual(compiler.call_count, 2)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'Canonical host compiler differs'):
                        setup.require_host(tools)


class GateControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.expected = 'a' * 40
        self.identity = {'git_revision': self.expected, 'dirty': False}

    def test_native_receipt_requires_the_full_shipping_rust_image(self):
        variant = gate.FIRMWARE_BUILD
        directory = self.root / 'artifacts'
        directory.mkdir()
        image = f'.pio/{variant}/build/newt_x3/firmware.bin'
        resource = {'identity': {}, 'artifacts': {image: 'b' * 64}}
        (directory / f'code-health-{variant}.json').write_text(json.dumps(resource))
        receipt = {**self.identity, 'variant': variant, 'firmware_version': '0.1.0-alpha.17',
                   'resource_identity': {}, 'binary_sha256': 'b' * 64,
                   'image': 'rust-app',
                   'features': ['rust-app', 'dev-usb', 'wifi-update', 'embedded-sample']}
        path = directory / f'firmware-identity-{variant}.json'
        with patch.object(gate, 'read_baseline', return_value={}), \
                patch.object(gate, 'check_resources', return_value=[]), \
                patch.object(gate, 'source_identity', return_value=receipt):
            path.write_text(json.dumps(receipt))
            gate.validate_native(self.root, self.expected)
            for change in ({'image': 'cpp'}, {'features': ['rust-app', 'dev-usb']}):
                path.write_text(json.dumps({**receipt, **change}))
                with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'full Rust daily'):
                    gate.validate_native(self.root, self.expected)

    def run_mocked(self, returncodes, *, after=None, validation_error=None, tier='full', started=None):
        calls = []
        def execute(command, **kwargs):
            self.assertEqual(kwargs['env']['UBSAN_OPTIONS'], 'halt_on_error=1')
            self.assertEqual(kwargs['env']['ASAN_OPTIONS'], 'halt_on_error=1:abort_on_error=1')
            calls.append(command)
            return subprocess.CompletedProcess(command, returncodes[len(calls)-1])
        def validate(*args):
            if validation_error:
                raise RuntimeError(validation_error)
        with patch.object(gate, 'committed_source', side_effect=[self.identity, after or self.identity]), \
             patch.object(gate, 'validate_native', side_effect=validate) as native, \
             patch.object(gate.gate_tier, 'ci_tier', return_value=tier), \
             patch.object(gate.subprocess, 'run', side_effect=execute), redirect_stdout(io.StringIO()):
            gate.run_gate(self.root, self.expected, started=started)
        return calls, native

    def test_ambient_sanitizer_recovery_cannot_weaken_gate_execution(self):
        with patch.dict(gate.os.environ, {'UBSAN_OPTIONS': 'halt_on_error=0',
                                         'ASAN_OPTIONS': 'exitcode=0'}):
            calls, _ = self.run_mocked([0] * 3)
        self.assertEqual(len(calls), 3)

    def test_ci_host_jobs_use_all_available_cpu_quota(self):
        for quota in (1, 8, 16):
            with self.subTest(quota=quota), patch.object(gate.host_jobs, 'available_cpus',
                                                       return_value=(quota, 'test cgroup quota')):
                host = gate.commands(self.root, sys.executable, 'push')[0][1]
                self.assertEqual(host.count('-j'), 1)
                self.assertEqual(host[host.index('-j') + 1], str(quota))

    def test_matching_forged_history_never_skips_any_gate(self):
        for _ in range(2):
            report = self.root / '.build/ci-gate/report.json'
            report.parent.mkdir(parents=True, exist_ok=True)
            report.write_text(json.dumps({'passed': True, 'source': self.identity}))
            calls, native = self.run_mocked([0] * 3)
            self.assertEqual(calls, [command for _, command in gate.commands(self.root, sys.executable, 'full')])
            self.assertIn('--fresh-objects', calls[0])
            self.assertEqual(calls[0][calls[0].index('--tier') + 1], 'full')
            self.assertEqual([name for name, _ in gate.commands(self.root, sys.executable, 'full')][:2],
                             ['host', 'native-preparation'])
            self.assertEqual(calls[1], [sys.executable, '-B', str(self.root / 'tools/firmware-build.py'), '--clean'])
            # The one firmware build, gated with a fresh health report and no variant flags.
            self.assertEqual(calls[2:], [[sys.executable, '-B', str(self.root / 'tools/firmware-build.py'),
                                          '--clean', '--health-report']])
            native.assert_called_once_with(self.root, self.expected)
            self.assertFalse(json.loads(report.read_text())['reusable_proof'])

    def test_each_gate_failure_remains_failure_and_invalidates_old_record(self):
        report = self.root / '.build/ci-gate/report.json'
        report.parent.mkdir(parents=True)
        for position in range(3):
            for _ in range(2):
                report.write_text('{"passed":true}')
                with self.assertRaises(subprocess.CalledProcessError) as error:
                    self.run_mocked([0] * position + [17])
                self.assertEqual(error.exception.returncode, 17)
                self.assertFalse(report.exists())

    def test_push_tier_reuses_objects_and_runs_the_same_three_gates(self):
        calls, _ = self.run_mocked([0] * 3, tier='push')
        self.assertEqual(calls, [command for _, command in gate.commands(self.root, sys.executable, 'push')])
        self.assertNotIn('--fresh-objects', calls[0])
        self.assertEqual(calls[0][calls[0].index('--tier') + 1], 'push')
        self.assertIn('--no-time-budget', calls[0])  # The job's budget covers the host gate.
        self.assertEqual(json.loads((self.root / '.build/ci-gate/report.json').read_text())['tier'], 'push')

    def test_a_job_over_its_tier_budget_fails_without_a_report(self):
        budget = gate.gate_tier.time_budget('push', 'ci')
        with patch.object(gate.gate_tier.os, 'getloadavg', return_value=(1.0, 1.0, 1.0)), \
                self.assertRaisesRegex(gate.gate_tier.GateFailure, 'CI push tier job took'):
            self.run_mocked([0] * 3, tier='push', started=gate.time.time() - budget - 1)
        self.assertFalse((self.root / '.build/ci-gate/report.json').exists())

    def test_release_merges_nightly_and_manual_runs_take_the_full_tier(self):
        """The tier comes from the committed version and the event; nothing lowers it."""
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.run(['git', *args], cwd=directory, check=True,
                                      capture_output=True, text=True).stdout.strip()
            def commit(version, message):
                Path(directory, '.cz.toml').write_text(f'[tool.commitizen]\nversion = "{version}"\n')
                git('add', '.cz.toml'); git('commit', '-qm', message)
                return git('rev-parse', 'HEAD')
            git('init', '-q'); git('config', 'user.name', 'CI fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            git('config', 'core.hooksPath', directory + '/no-hooks')
            first = commit('0.1.0', 'chore: first')
            Path(directory, 'feature').write_text('x\n'); git('add', 'feature')
            git('commit', '-qm', 'feat: change'); change = git('rev-parse', 'HEAD')
            release = commit('0.1.1', 'chore(release): 0.1.1')
            for event, commit_, tier in (('push', change, 'push'), ('push', release, 'full'),
                                         ('push', first, 'full'), ('schedule', change, 'full'),
                                         ('schedule', release, 'full'),
                                         ('workflow_dispatch', change, 'full'), (None, change, 'full')):
                with self.subTest(event=event, commit=commit_), \
                        patch.dict(os.environ, {'GITHUB_EVENT_NAME': event} if event else {}, clear=False):
                    if event is None:
                        os.environ.pop('GITHUB_EVENT_NAME', None)
                    self.assertEqual(gate.gate_tier.ci_tier(Path(directory), commit_), tier)
                    # Only a release merge's push run is never cancelled; a nightly run of it may be.
                    self.assertEqual(gate.gate_tier.release_push(Path(directory), commit_),
                                     event == 'push' and commit_ in (release, first))

    def test_final_source_and_resource_drift_prevent_report(self):
        for kwargs in ({'after': {'git_revision': 'b' * 40}}, {'validation_error': 'resource drift'}):
            with self.assertRaises(RuntimeError):
                self.run_mocked([0] * 4, **kwargs)
            self.assertFalse((self.root / '.build/ci-gate/report.json').exists())

    def test_committed_source_rejects_wrong_dirty_and_hidden_modified_bytes(self):
        for name in ('.cz.toml', 'rust/firmware/native/firmware_version.h', gate.UI_REVIEW_AUTHORITY):
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, destination)
        def git(*args):
            return subprocess.run(['git', *args], cwd=self.root, check=True, capture_output=True, text=True).stdout.strip()
        git('init', '-q')
        git('config', 'user.name', 'CI fixture')
        git('config', 'user.email', 'fixture@example.invalid')
        git('config', 'core.hooksPath', str(self.root / 'no-hooks'))
        git('add', '.')
        git('commit', '-qm', 'chore: authored CI fixture')
        expected = git('rev-parse', 'HEAD')
        with patch.object(gate, 'verify_source_pin'):
            gate.committed_source(self.root, expected)
            for wrong in ('short', 'b' * 40):
                with self.assertRaises(RuntimeError):
                    gate.committed_source(self.root, wrong)
            header = self.root / 'rust/firmware/native/firmware_version.h'
            header.write_text(header.read_text() + '// mutation\n')
            with self.assertRaisesRegex(RuntimeError, 'clean commit'):
                gate.committed_source(self.root, expected)
            git('update-index', '--assume-unchanged', 'rust/firmware/native/firmware_version.h')
            with self.assertRaisesRegex(RuntimeError, 'source bytes/mode'):
                gate.committed_source(self.root, expected)

    def test_reviewed_ui_authority_cannot_be_modified_or_removed_as_generated_output(self):
        for name in ('.cz.toml', 'rust/firmware/native/firmware_version.h', gate.UI_REVIEW_AUTHORITY):
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, destination)
        def git(*args):
            return subprocess.run(['git', *args], cwd=self.root, check=True,
                                  capture_output=True, text=True).stdout.strip()
        git('init', '-q')
        git('config', 'user.name', 'CI fixture')
        git('config', 'user.email', 'fixture@example.invalid')
        git('config', 'core.hooksPath', str(self.root / 'no-hooks'))
        git('add', '.')
        git('commit', '-qm', 'chore: commit reviewed UI authority')
        expected = git('rev-parse', 'HEAD')
        authority = self.root / gate.UI_REVIEW_AUTHORITY
        original = authority.read_bytes()
        with patch.object(gate, 'verify_source_pin'):
            identity = gate.committed_source(self.root, expected)
            self.assertEqual(identity['reviewed_ui_manifest_sha256'], hashlib.sha256(original).hexdigest())
            # Generated output remains outside source identity and cannot confer proof.
            (self.root / 'artifacts/generated-receipt.json').write_text('{"passed":true}')
            self.assertEqual(gate.committed_source(self.root, expected), identity)
            git('update-index', '--assume-unchanged', gate.UI_REVIEW_AUTHORITY)
            manifest = json.loads(original)
            manifest['frames']['library'] = '0' * 64
            authority.write_text(json.dumps(manifest))
            self.assertFalse(gate.source_identity(self.root)['dirty'])
            with self.assertRaisesRegex(RuntimeError, 'Reviewed UI authority bytes/mode'):
                gate.committed_source(self.root, expected)
            authority.write_bytes(original)
            authority.chmod(0o755)
            with self.assertRaisesRegex(RuntimeError, 'Reviewed UI authority bytes/mode'):
                gate.committed_source(self.root, expected)
            authority.chmod(0o644)
            authority.unlink()
            with self.assertRaises(OSError):
                gate.committed_source(self.root, expected)
        git('rm', '--cached', gate.UI_REVIEW_AUTHORITY)
        git('commit', '-qm', 'chore: remove reviewed UI authority')
        with patch.object(gate, 'verify_source_pin'), self.assertRaisesRegex(RuntimeError, 'Missing committed reviewed UI'):
            gate.committed_source(self.root, git('rev-parse', 'HEAD'))


if __name__ == '__main__':
    unittest.main()
