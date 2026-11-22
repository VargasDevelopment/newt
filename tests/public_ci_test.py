"""The public workflow (.github/workflows/public-ci.yml): GitHub-hosted runners
only, jobs only in a public repository, first-party actions pinned to commits,
read-only but for the tag job and the nightly job; its host shards, which
together run exactly the tier's plan; and the hosted toolchain cache, never
trusted over its check."""
import json
import os
from pathlib import Path
import random
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tests'))
import check as host_check
import ci_environment as setup
import ci_gate
import ci_hosted
import ci_persist
from ci_toolchain import read_lock
from workflow_expression import PRIVATE, PUBLIC, trust_expression, where
import gate_plan
import gate_runner
import gate_tier
import release_tag
from release_tools import release_python

WORKFLOWS = ROOT / '.github/workflows'
# Each tier's controls(script) for gate_plan.plan: a pull request's local tier
# adds the controls of the tests it changes.
TIERS = {'push': lambda script: False, 'full': None,
         'local': lambda script: script in ('installer_test.py', 'reader_turn_cost_test.py')}


def workflow(name):
    # The already required pinned release parser, not a new dependency.
    probe = 'import json,yaml,sys; print(json.dumps(yaml.load(open(sys.argv[1]),Loader=yaml.BaseLoader)))'
    return json.loads(subprocess.run([str(release_python()), '-I', '-B', '-c', probe, str(WORKFLOWS / name)],
                                     check=True, capture_output=True, text=True).stdout)


class PublicWorkflow(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow, cls.private = workflow('public-ci.yml'), workflow('fresh-gates.yml')
        cls.text = (WORKFLOWS / 'public-ci.yml').read_text()
        cls.check = cls.workflow['jobs']['check']

    def step(self, name):
        return next(step for step in self.check['steps'] if step.get('name') == name)

    def test_triggers_permissions_and_github_hosted_runners_only(self):
        self.assertEqual(set(self.workflow['on']), {'pull_request', 'push', 'schedule', 'workflow_dispatch'})
        self.assertEqual(self.workflow['on']['workflow_dispatch'], '')  # no inputs
        self.assertEqual(self.workflow['on']['push'], {'branches': ['main']})
        # The gate compares a candidate with origin/main, so only pull requests into main.
        self.assertEqual(self.workflow['on']['pull_request'], {'branches': ['main']})
        self.assertEqual(len(self.workflow['on']['schedule']), 1)  # The nightly full tier.
        self.assertNotIn('pull_request_target', self.text)
        self.assertEqual(self.workflow['permissions'], {'contents': 'read'})
        self.assertEqual(list(self.workflow['jobs']), ['check', 'gate', 'release-tag', 'nightly'])
        tokens = ('Tag the release if this run checked it', 'Push the nightly tag and publish its pre-release')
        for name, job in self.workflow['jobs'].items():
            self.assertEqual(job['runs-on'], 'ubuntu-24.04', name)
            self.assertEqual('permissions' in job, name in ('release-tag', 'nightly'), name)
            self.assertNotIn('continue-on-error', job, name)  # A job that may fail would pass the gate.
            for step in job['steps']:
                self.assertNotIn('continue-on-error', step)
                text = json.dumps(step)
                self.assertEqual('github.token' in text or 'secrets.' in text, step.get('name') in tokens,
                                 step.get('name'))
        self.assertEqual(self.workflow['jobs']['release-tag']['permissions'], {'actions': 'read', 'contents': 'write'})
        self.assertEqual(self.workflow['jobs']['nightly']['permissions'], {'contents': 'write'})

    def test_every_action_is_first_party_and_pinned_to_its_locked_commit(self):
        tools, _ = setup.settings(ROOT, 'linux-x86_64')
        uses = [step['uses'] for job in self.workflow['jobs'].values() for step in job['steps'] if 'uses' in step]
        self.assertTrue(uses)
        for each in uses:
            name, revision = each.split('@')
            self.assertTrue(name.startswith('actions/'), each)
            self.assertEqual(revision, tools['actions'][name], each)
            self.assertRegex(revision, '^[0-9a-f]{40}$')

    def test_jobs_run_only_in_a_public_repository(self):
        base = {'success': True, 'cancelled': False, 'repository': 'VargasDevelopment/newt',
                'ref': 'refs/heads/main', 'actor': 'VargasDevelopment'}
        for name, job in self.workflow['jobs'].items():
            allowed = trust_expression(job['if'])
            with self.subTest(job=name):
                for event_name in ('push', 'pull_request', 'schedule', 'workflow_dispatch'):
                    self.assertFalse(allowed(where({**base, **PRIVATE}, event_name)), event_name)
                # A missing visibility never passes, on any event.
                self.assertFalse(allowed({**base, **PUBLIC, 'event_name': 'push', 'event.repository.private': None}))
                self.assertTrue(allowed(where({**base, **PUBLIC}, 'push')))
                writes = name in ('release-tag', 'nightly')
                for event_name in ('pull_request', 'schedule'):
                    self.assertEqual(allowed(where({**base, **PUBLIC}, event_name)), not writes, event_name)
                # A manual run gates main and publishes a nightly, but never tags.
                self.assertEqual(allowed(where({**base, **PUBLIC}, 'workflow_dispatch')), name != 'release-tag')
                if writes:  # only the owner's runs on main, in this repository
                    for other in ({'ref': 'refs/heads/feature'}, {'actor': 'someone'}, {'repository': 'someone/newt'}):
                        self.assertFalse(allowed(where({**base, **PUBLIC, **other}, 'push')), other)
        # Why the guards compare JSON text: GitHub's == coerces null to false.
        self.assertTrue(trust_expression('github.event.repository.private == false')(where(PRIVATE, 'schedule')))
        # fresh-gates.yml is the mirror image; tests/ci_workflow_test.py checks its jobs in full.
        for name, job in self.private['jobs'].items():
            accepted = {**base, 'event_name': 'push', **PRIVATE}
            self.assertTrue(trust_expression(job['if'])(accepted), name)
            self.assertFalse(trust_expression(job['if'])({**accepted, **PUBLIC}), name)

    def test_parts_are_every_host_shard_and_the_firmware_build(self):
        parts = self.check['strategy']['matrix']['part']
        count = len(parts) - 1
        self.assertEqual(parts, [f'{index}/{count}' for index in range(count)] + ['native'])
        self.assertEqual(self.check['strategy']['fail-fast'], 'false')
        self.assertEqual(self.check['env']['PART'], '${{ matrix.part }}')
        gate = self.step('This part of the gate')
        self.assertEqual(gate['run'], 'python3 -B tools/ci_gate.py --expected-commit "$EXPECTED_COMMIT" --part "$PART"')
        python = Path(sys.executable)
        complete = ci_gate.commands(ROOT, python, 'push')
        host, *native = complete
        self.assertEqual(ci_gate.commands(ROOT, python, 'push', 'native'), native)
        for part in parts[:-1]:
            self.assertEqual(ci_gate.commands(ROOT, python, 'push', part), [(host[0], [*host[1], '--shard', part])])

    def test_shards_run_every_phase_of_the_tier_exactly_once(self):
        count = len(self.check['strategy']['matrix']['part']) - 1
        measured = gate_tier.phase_seconds(ROOT, gate_tier.SHARD_SECONDS)
        self.assertTrue(measured)
        shuffled = dict(zip(measured, random.Random(74).sample(list(measured.values()), len(measured))))
        for tier, controls in TIERS.items():
            plan = gate_plan.plan('linux', tier == 'full', controls)
            for name, seconds in (('measured', measured), ('none', {}), ('shuffled', shuffled)):
                with self.subTest(tier=tier, seconds=name):
                    shards = [gate_tier.shard(plan, index, count, seconds) for index in range(count)]
                    ran = [phase.name for shard in shards for phase in shard]
                    self.assertCountEqual(ran, [phase.name for phase in plan])
                    for shard in shards:
                        self.assertTrue(shard)
                        gate_runner.prerequisites(shard)  # each phase's `after` phases are in its shard
                        self.assertEqual(shard, [phase for phase in plan if phase in shard])  # in plan order
        with self.assertRaises(ValueError):
            gate_tier.shard(plan, count, count, measured)

    def test_phases_sharing_a_uses_path_go_to_separate_shards(self):
        """`uses` serializes phases on one machine; separate jobs run them in parallel."""
        Phase, lock = gate_runner.Phase, ('.build/shared',)
        plan = [Phase('a', uses=lock), Phase('b', uses=lock), Phase('c'), Phase('d')]
        seconds = {'a': 50, 'b': 30, 'c': 100, 'd': 10}
        shards = [[phase.name for phase in gate_tier.shard(plan, index, 2, seconds)] for index in range(2)]
        self.assertEqual(shards, [['b', 'c'], ['a', 'd']])  # load alone would put b beside a
        # With no free shard left, the lightest takes the group.
        self.assertEqual([phase.name for phase in gate_tier.shard(plan, 0, 1, seconds)], ['a', 'b', 'c', 'd'])

    def test_check_runs_exactly_its_shard(self):
        """The real tools/check.py entry hands the runner its shard of the push plan."""
        ran = []
        def run(plan, *args, **kwargs):
            ran.append([phase.name for phase in plan])
            return {}
        arguments = ['check.py', '--tier', 'push', '--shard', '3/7', '--no-time-budget', '--profile', 'linux']
        with patch.object(sys, 'argv', arguments), patch.object(host_check, 'verify_source_pin'), \
                patch.object(host_check, 'phase_run'), patch.object(host_check.gate_runner, 'run', side_effect=run), \
                patch.object(host_check.gate_tier, 'save_phase_seconds'):
            host_check.main()
        expected = gate_tier.shard(gate_plan.plan('linux', False, TIERS['push']), 3, 7,
                                   gate_tier.phase_seconds(ROOT, gate_tier.SHARD_SECONDS))
        self.assertEqual(ran, [[phase.name for phase in expected]])

    def test_a_pull_request_takes_the_local_tier(self):
        with patch.dict(os.environ, {'GITHUB_EVENT_NAME': 'pull_request'}):
            self.assertEqual(gate_tier.ci_tier(ROOT, 'HEAD'), 'local')
        tier = self.step('Which tier this run takes (tools/gate_tier.py)')
        self.assertEqual(tier['id'], 'tier')
        self.assertIn('tier="$(python3 -B tools/gate_tier.py --ci-tier "$EXPECTED_COMMIT")"', tier['run'])

    def test_python_comes_from_the_locked_uv_wheel_and_the_private_runtime_check(self):
        record = next(item for item in read_lock(ROOT, 'linux-x86_64')['artifacts'] if item['filename'].startswith('uv-'))
        uv = self.step('Locked uv for workspace-local Python')['run']
        self.assertIn(record['url'], uv)
        self.assertIn(f'echo "{record["sha256"]}  $wheel" | sha256sum --check --strict', uv)
        runtime = next(step for step in self.private['jobs']['fresh-gates']['steps']
                       if step.get('name') == 'Exact managed Python and real runtime checks')
        self.assertEqual(self.step('Exact managed Python and real runtime checks'), runtime)

    def test_installer_packages_come_from_the_lockfile(self):
        install = self.step('Installer packages exactly as installer/pnpm-lock.yaml pins them')
        self.assertEqual(install['working-directory'], 'installer')
        self.assertIn('test "$(pnpm --version)" = 11.10.0', install['run'])
        self.assertEqual(install['run'].splitlines()[-1], 'pnpm install --frozen-lockfile')

    def test_caches_are_keyed_on_the_locks_and_only_trusted_runs_save(self):
        steps = self.check['steps']
        names = [step.get('name') for step in steps]
        restores = [step for step in steps if step.get('uses', '').startswith('actions/cache/restore@')]
        saves = [step for step in steps if step.get('uses', '').startswith('actions/cache/save@')]
        self.assertEqual(len(restores), 3)
        self.assertEqual(len(saves), 3)
        for restore in restores:  # each restore has its save, with the same paths and key
            save = next(save for save in saves if save['with']['key'] == restore['with']['key'])
            self.assertEqual(save['with']['path'], restore['with']['path'])
        self.assertEqual(self.step('Restore the raw locked archives')['with']['path'].splitlines(), list(ci_persist.RAW))
        for save in saves:
            self.assertTrue("github.event_name != 'pull_request'" in save['if']
                            or "github.event_name == 'push'" in save['if'], save['name'])
        for lock in ci_persist.LOCKS:
            for step in restores[:2]:
                self.assertIn(f"'{lock}'", step['with']['key'])
        trees = self.step('Restore the toolchains a fresh run made from these locks')
        self.assertEqual(trees['with']['path'].splitlines(), ['.toolchain', '!.toolchain/downloads', '.release-venv',
                                                         ci_hosted.RECORD])
        self.assertEqual(trees['if'], "steps.tier.outputs.tier != 'full'")
        # The fresh toolchains are saved before any gate can change them.
        self.assertLess(names.index('Save the fresh toolchains'), names.index('This part of the gate'))
        self.assertEqual(self.step('Locked native, release, LLVM and static OpenSSL toolchains')['run'],
                         'python3 -B tools/ci_hosted.py --expected-commit "$EXPECTED_COMMIT"')
        objects = self.step("Restore this part's host objects from main's latest run")['with']['path'].splitlines()
        store = '${{ github.workspace }}/../' + ci_persist.STORE.name
        self.assertEqual(objects, [f'{store}/host-objects', f'{store}/gate-phase-seconds.json'])

    def test_the_gate_job_runs_and_fails_unless_every_job_it_needs_succeeded(self):
        """The real `if` and step, for each result GitHub gives a needed job."""
        jobs = self.workflow['jobs']
        self.assertEqual(release_tag.GATES['public-ci.yml'], 'gate')
        gate, tag = jobs['gate'], jobs['release-tag']
        self.assertEqual(gate['needs'], 'check')
        [step] = gate['steps']
        self.assertEqual(step['env'], {'NEEDS': '${{ toJSON(needs) }}'})
        base = {'repository': 'VargasDevelopment/newt', 'ref': 'refs/heads/main', 'actor': 'VargasDevelopment',
                **where(PUBLIC, 'push')}
        for result in ('success', 'failure', 'cancelled', 'skipped'):
            with self.subTest(check=result):
                # success() holds only when every need succeeded; cancelled() when the run was cancelled.
                state = {**base, 'success': result == 'success', 'cancelled': result == 'cancelled'}
                self.assertTrue(trust_expression(gate['if'])(state))  # it runs, so it can fail
                needs = json.dumps({'check': {'result': result, 'outputs': {}}})
                passed = subprocess.run(['bash', '-c', step['run']], env={**os.environ, 'NEEDS': needs},
                                        capture_output=True).returncode == 0
                self.assertEqual(passed, result == 'success')
                # The tag job runs only after the gate job itself succeeded.
                self.assertEqual(trust_expression(tag['if'])({**base, 'success': passed, 'cancelled': False}),
                                 passed)
        # A gate with nothing to judge does not pass either.
        self.assertNotEqual(subprocess.run(['bash', '-c', step['run']], env={**os.environ, 'NEEDS': '{}'},
                                           capture_output=True).returncode, 0)

    def test_the_nightly_job_builds_and_publishes_after_the_gate_as_releases_are_built(self):
        jobs = self.workflow['jobs']
        job = jobs['nightly']
        self.assertEqual(job['needs'], 'gate')
        self.assertEqual(job['concurrency'], {'group': 'nightly', 'cancel-in-progress': 'false'})
        base = {'repository': 'VargasDevelopment/newt', 'ref': 'refs/heads/main', 'actor': 'VargasDevelopment',
                'cancelled': False, **where(PUBLIC, 'push')}
        self.assertFalse(trust_expression(job['if'])({**base, 'success': False}))  # never without a passed gate
        # Its setup is the check job's own, step for step, and it saves no cache.
        steps = {step['name']: step for step in job['steps']}
        names = [step['name'] for step in job['steps']]
        stamp = names.index("Stamp main's tip with the nightly version")
        for name in names[:stamp]:
            self.assertEqual(steps[name], self.step(name), name)
        shared = set(self.check['env']) - {'EXPECTED_COMMIT', 'PART'}
        self.assertEqual(set(job['env']), shared | {'EXPECTED_COMMIT'})
        self.assertEqual({key: job['env'][key] for key in shared}, {key: self.check['env'][key] for key in shared})
        self.assertEqual(job['env']['EXPECTED_COMMIT'], '${{ github.sha }}')
        self.assertFalse(any(step.get('uses', '').startswith('actions/cache/save@') for step in job['steps']))
        self.assertEqual(names[stamp:], ["Stamp main's tip with the nightly version",
                                         'Build the stamped commit as a release is built',
                                         'Push the nightly tag and publish its pre-release'])
        # Only a manual run forces a nightly with no image change.
        run = steps["Stamp main's tip with the nightly version"]['run']
        self.assertIn('if [[ "$GITHUB_EVENT_NAME" == workflow_dispatch ]]; then force=(--force); fi', run)
        self.assertIn('python3 -B tools/nightly.py stamp "${force[@]}" >> "$GITHUB_OUTPUT"', run)
        # The one firmware build exactly as the gate's native part makes it, then the checked publish.
        build = steps['Build the stamped commit as a release is built']['run'].splitlines()
        python = Path(sys.executable)
        self.assertEqual([command for _, command in ci_gate.commands(ROOT, python, 'push', 'native')],
                         [[str(python), flag, str(ROOT / script), *rest] for _, flag, script, *rest in map(str.split, build)])
        publish = steps['Push the nightly tag and publish its pre-release']
        self.assertEqual(publish['env']['GH_TOKEN'], '${{ github.token }}')
        self.assertEqual(publish['run'].splitlines()[-2:], [
            'git push --no-follow-tags origin "refs/tags/v$VERSION:refs/tags/v$VERSION"',
            'python3 -B tools/release_publish.py --prerelease --publish'])
        for name in names[stamp + 1:]:
            self.assertEqual(steps[name]['if'], "steps.stamp.outputs.version != ''", name)

    def test_the_release_tag_job_tags_like_fresh_gates(self):
        jobs, private = self.workflow['jobs'], self.private['jobs']
        tag, expected = jobs['release-tag'], private['release-tag']
        self.assertEqual(tag['needs'], 'gate')
        self.assertEqual(tag['env'], {'EXPECTED_COMMIT': '${{ github.sha }}'})
        self.assertEqual(tag['steps'], expected['steps'])


class HostedToolchains(unittest.TestCase):
    """tools/ci_hosted.py with a stand-in materialization."""
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        for lock in ci_persist.LOCKS:
            (self.root / lock).parent.mkdir(parents=True, exist_ok=True)
            (self.root / lock).write_text('{}\n')
        (self.root / '.build').mkdir()
        self.made = 0

    def materialize(self, command, **kwargs):
        if command[2:] == list(ci_persist.PREPARE[0]):
            self.made += 1
            for name in ('.toolchain/llvm/bin/clang', '.release-venv/bin/python'):
                (self.root / name).parent.mkdir(parents=True, exist_ok=True)
                (self.root / name).write_text(f'made {self.made}\n')

    def prepare(self, tier):
        with patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': 'sha256:' + ci_persist.sha256(self.root / 'rust/toolchain.json')}), \
                patch.object(ci_hosted.subprocess, 'run', side_effect=self.materialize):
            ci_hosted.prepare(self.root, tier)

    def test_reuse_only_what_matches_its_record_and_never_in_the_full_tier(self):
        downloads = self.root / '.toolchain/downloads/archive.tar.xz'
        downloads.parent.mkdir(parents=True)
        downloads.write_text('raw')
        self.prepare('push')
        self.assertEqual(self.made, 1)
        self.prepare('local')
        self.assertEqual(self.made, 1)  # reused
        self.prepare('full')
        self.assertEqual(self.made, 2)  # never reused
        (self.root / '.toolchain/llvm/bin/clang').write_text('changed\n')
        self.prepare('push')
        self.assertEqual(self.made, 3)  # rejected and made fresh
        self.assertEqual((self.root / '.toolchain/llvm/bin/clang').read_text(), 'made 3\n')
        (self.root / ci_persist.LOCKS[0]).write_text('{"other": 1}\n')
        self.prepare('push')
        self.assertEqual(self.made, 4)  # made from other locks
        self.assertEqual(downloads.read_text(), 'raw')  # raw archives stay for the fresh materialization
        record = json.loads((self.root / ci_hosted.RECORD).read_text())
        self.assertEqual(record['manifest'], ci_persist.manifest(self.root))
        shutil.rmtree(self.root / '.release-venv')
        self.prepare('push')
        self.assertEqual(self.made, 5)  # a missing tree fails the manifest


class InstallPageWorkflow(unittest.TestCase):
    """install-page.yml: the install page on the public repository's Pages, with the latest release's pair."""

    @classmethod
    def setUpClass(cls):
        cls.workflow = workflow('install-page.yml')
        cls.jobs = cls.workflow['jobs']

    def test_one_read_job_and_one_pages_deploy_job_on_published_releases(self):
        self.assertEqual(self.workflow['on'], {'release': {'types': ['released']}, 'workflow_dispatch': ''})
        self.assertEqual(self.workflow['permissions'], {})
        self.assertEqual(list(self.jobs), ['build', 'deploy'])
        self.assertEqual(self.jobs['build']['permissions'], {'contents': 'read'})
        self.assertEqual(self.jobs['deploy']['permissions'], {'pages': 'write', 'id-token': 'write'})
        self.assertEqual(self.jobs['deploy']['environment']['name'], 'github-pages')
        build = {step.get('name'): step for step in self.jobs['build']['steps']}
        # The page's own build checks the pair; nothing else stands in for it.
        self.assertEqual(build['Build the page with the release\'s pair']['run'], 'pnpm build')
        for job in self.jobs.values():
            self.assertEqual(job['runs-on'], 'ubuntu-24.04')
            self.assertNotIn('continue-on-error', job)

    def test_the_page_is_built_from_the_latest_releases_tag_and_its_pair(self):
        build = {step.get('name'): step for step in self.jobs['build']['steps']}
        latest = build['The latest release and its three assets']['run']
        self.assertIn('tag="$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName --jq .tagName)"', latest)
        self.assertIn('gh release download "$tag" --repo "$GITHUB_REPOSITORY" --dir "$RUNNER_TEMP/release"', latest)
        checkout = build["Check out the page's source at that release"]['with']
        self.assertEqual(checkout['ref'], 'refs/tags/${{ steps.release.outputs.tag }}')
        self.assertEqual(build["Build the page with the release's pair"]['env'],
                         {'NEWT_RELEASE': '${{ runner.temp }}/release'})

    def test_every_action_is_first_party_and_pinned(self):
        tools, _ = setup.settings(ROOT, 'linux-x86_64')
        uses = [step['uses'] for job in self.jobs.values() for step in job['steps'] if 'uses' in step]
        self.assertEqual([each.split('@')[0] for each in uses], ['actions/checkout', 'actions/setup-node',
                                                                 'actions/upload-pages-artifact', 'actions/deploy-pages'])
        for each in uses:
            name, revision = each.split('@')
            self.assertRegex(revision, '^[0-9a-f]{40}$')
            if name in tools['actions']:
                self.assertEqual(revision, tools['actions'][name], each)

    def test_jobs_run_only_in_the_public_repository(self):
        base = {'success': True, 'repository': 'VargasDevelopment/newt'}
        for name, job in self.jobs.items():
            allowed = trust_expression(job['if'])
            with self.subTest(job=name):
                for event_name in ('release', 'workflow_dispatch'):
                    self.assertTrue(allowed(where({**base, **PUBLIC}, event_name)), event_name)
                    self.assertFalse(allowed(where({**base, **PRIVATE}, event_name)), event_name)
                    self.assertFalse(allowed(where({**base, **PUBLIC, 'repository': 'someone/newt'}, event_name)))
                self.assertFalse(allowed({**base, **PUBLIC, 'event.repository.private': None}))


if __name__ == '__main__':
    unittest.main()
