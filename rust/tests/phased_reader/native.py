"""Native host build and SDK input verification for the canonical Rust reader."""
import importlib.util
import json
from pathlib import Path
import shlex
import shutil
import sys
sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PIN = 'bc9ffda52b85f277e46c63718ac88d62fba5d368'
HOST = 'x86_64-unknown-linux-gnu'
RV32 = 'riscv32imc-unknown-none-elf'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def native(run, support, ws, original, sdk):
    run.cargo(ws, run.output/'default-target', 'build', '-p', 'newt-phased-reader', '--target', HOST)
    sdk = ROOT/'vendor/freeink-sdk'
    verify_native_inputs(support, sdk)
    book = sdk/'libs/book/FreeInkBook'
    includes = [arg for p in (book/'include',
                 sdk/'libs/font/FreeInkFont/include', book/'third_party/expat',
                 book/'third_party/miniz', book/'third_party/libunibreak') for arg in ('-I', p)]
    flags = ['-O1', '-g', '-DFREEINK_BOOK_SMALL=1', '-fsanitize=address,undefined',
             '-fno-sanitize-recover=all', '-fno-omit-frame-pointer']
    objects = {}
    for name in ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge'):
        path = ws/'target-expat/native/expat_bridge.c' if name == 'bridge' else book/'src/vendor'/(name+'.c')
        obj = run.output/(name+'.o')
        run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99',
                 *flags, *includes, '-c', path, '-o', obj], ROOT)
        objects[name] = obj
    host_flags = ['-Dwarnings', '-Ccodegen-units=1', '-C', 'link-arg=-fsanitize=address,undefined',
                  '-C', 'link-arg=-fno-sanitize-recover=all']
    for name in ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge'):
        host_flags += ['-C', 'link-arg='+str(objects[name])]
    run.env['RUSTFLAGS'] = shlex.join(host_flags)
    run.cargo(ws, run.output/'target', 'build', '-p', 'newt-phased-reader',
              '--bin', 'phased-reader-host', '--features', 'native-host', '--target', HOST)
    return None, run.output/'target'/HOST/'debug/phased-reader-host'


def book_language(run, ws, executable):
    fixture_dir = run.output/'language-fixtures'
    load('phased_reader_fixtures', ws/'phased-reader/host/fixtures.py').write_languages(fixture_dir)
    run.run([executable, '--language', fixture_dir], ROOT)


def assemble(output, support):
    """Snapshot the adopted workspace; never inject or replace production bodies."""
    ws = output / 'source'
    root_ws = ROOT / 'rust'
    support.copy_workspace(root_ws, ws)
    resource = Path('tests/phased_reader/host_resources.rs')
    (ws / resource).parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root_ws / resource, ws / resource)
    load('producer_expected_recipe', ROOT/'rust/tests/producer_forwarding/recipe.py').stage(ws)
    fixtures = load('phased_reader_fixtures', ws/'phased-reader/host/fixtures.py')
    fixtures.write_producer_language(ws/'tests/producer_forwarding/ja.epub')
    fixtures.write_language_fixture(ws/'tests/producer_forwarding/ja-short.epub', 'ja')
    return ws


def verify_native_inputs(support, sdk):
    pins = json.loads((HERE/'native-inputs.json').read_text())
    support.require(pins['reference'] == PIN and pins['sdk'] == 'cd6f5b831c8fad899723b6d406586f00c575f021',
                    'native product identities')
    for relative, sha in pins['sha256'].items():
        support.require(support.digest(sdk/relative) == sha, 'native product source: '+relative)
