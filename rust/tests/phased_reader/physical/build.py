"""Host transport build shared by physical storage positives and controls."""
import json
import importlib.util
from pathlib import Path


def host_link_args(run, libraries, native_arguments, *, panic='abort'):
    args = [run.tools['rustc'], '--edition=2024', '--target', 'x86_64-unknown-linux-gnu',
            '-Dwarnings', '-Cpanic='+panic, '-Clto=fat', '-Ccodegen-units=1',
            '-Clinker='+str(run.tools['clang++']), '-Clink-arg=--no-default-config',
            '-Clink-arg=-fuse-ld='+str(run.tools['native-ld'])]
    for name, path in libraries.items():
        args += ['--extern', name+'='+str(path)]
    for directory in sorted({str(Path(p).parent) for p in libraries.values()}):
        args += ['-L', 'dependency='+directory]
    for argument in native_arguments:
        args += ['-Clink-arg='+str(argument)]
    return args


def storage_host(run, ws, support, here):
    sdk = support.ROOT / 'vendor/freeink-sdk'
    provenance = json.loads((ws / 'tests/page_renderer/PROVENANCE.json').read_text())
    support.require(provenance['reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368'
                    and provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021',
                    'original SDK identities')
    spec=importlib.util.spec_from_file_location('physical_native_inputs',Path(__file__).parents[1]/'native.py')
    native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)
    native.verify_native_inputs(support,sdk)
    target = run.output / 'target'
    run.cargo(ws, target, 'build', '--release', '--lib', '-p', 'newt-phased-reader',
              '--target', 'x86_64-unknown-linux-gnu', '--features',
              'newt-phased-reader/style-recapture-fixture')
    deps = {p.name[3:].split('-')[0]: p for p in
            (target / 'x86_64-unknown-linux-gnu/release/deps').glob('libnewt_*.rlib')}
    book = sdk / 'libs/book/FreeInkBook'
    includes = [value for path in [book/'include', book/'third_party/expat',
                book/'third_party/miniz', book/'third_party/libunibreak']
                for value in ['-I', path]]
    objects = []
    for name in ['expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge']:
        source = ws/'target-expat/native/expat_bridge.c' if name == 'bridge' else book/'src/vendor'/(name+'.c')
        obj = run.output / (name+'.o')
        run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99',
                 '-O1', '-g', '-fsanitize=address,undefined', '-fno-sanitize-recover=all',
                 *includes, '-c', source, '-o', obj], ws)
        objects.append(obj)
    args = host_link_args(run, deps, [*objects, '-fsanitize=address,undefined',
                                      '-fno-sanitize-recover=all', '-lstdc++'])
    exe = run.output / 'storage-host'
    run.run([*args, here/'storage_host.rs', '-o', exe], ws)
    return exe
