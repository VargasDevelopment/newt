#!/usr/bin/env python3
"""Compile the actual Rust firmware for the host, over C fakes of its natives.

One recipe, used by the host preview (tools/rust_preview.py). Every
rust/firmware/*.rs file is copied unchanged
into OUTPUT/source, with its #[path] attributes resolved to that copy, so a
caller may add observers to the copy without touching the firmware. lib.rs
loses only its target allocator, panic handler and native entry point; the
caller appends its own host root. Native symbols that the caller's C fakes
do not define become traps that abort at their first call, so an unexpected
peripheral call never manufactures a result.

No acquisition: the pinned toolchain under TOOL_ROOT/.toolchain must exist.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
FIRMWARE = ROOT / 'rust/firmware'
SALAMANDER = ROOT / 'rust/salamander'
HOST = 'x86_64-unknown-linux-gnu'
# The firmware's own features, as the owner's daily image enables them.
BASE_FEATURES = ('newt-reader-ui/native-host', 'newt-native-input-owner/native-atomics', 'newt-core/dev-usb',
                 'newt-device-ui/dev-usb', 'newt-native-storage/dev-usb', 'newt-phased-reader/dev-usb')
NATIVE = ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl')


def resolve_paths(text, parent, replacements=None):
    """Point every #[path] at its absolute (or replaced) file."""
    replacements = replacements or {}
    def replace(match):
        source = (parent / match[1]).resolve()
        return '#[path = ' + json.dumps(str(replacements.get(source, source))) + ']'
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]', replace, text)


def _module_paths(text, directory):
    """Give each bare `mod name;` (no #[path] among its attributes) an
    explicit #[path] under `directory`."""
    lines = text.splitlines()
    def attributes(i):
        while i > 0 and lines[i - 1].startswith('#['):
            i -= 1
            yield lines[i]
    return '\n'.join((('#[path = ' + json.dumps(str(directory / (m[2] + '.rs'))) + ']\n')
                      if (m := re.fullmatch(r'(pub(?:\([^)]*\))? )?mod (\w+);', line))
                      and not any(a.startswith('#[path') for a in attributes(i)) else '') + line
                     for i, line in enumerate(lines))


def _includes(text, parent):
    """Point include_bytes!/include_str! at the original file."""
    return re.sub(r'(include_(?:bytes|str)!\()"([^"]+)"',
                  lambda m: m[1] + json.dumps(str((parent / m[2]).resolve())), text)


class Build:
    """One host build under `out`. `run(command, succeeds=True)` is the
    caller's logged runner (its receipts are the caller's evidence)."""

    def __init__(self, out, tool_root, run):
        self.out = out
        tool = Path(tool_root).resolve() / '.toolchain'
        self.rustc = tool / 'rust/toolchain/bin/rustc'
        self.cargo = tool / 'rust/toolchain/bin/cargo'
        self.clang = tool / 'llvm/bin/clang++'
        self.tool = tool
        self.run = run
        self.libraries = {}
        self.objects = []
        self.replacements = {}
        self.command = None

    def environment(self, base, build_id):
        """The cargo/rustc environment for the real firmware and its dependencies."""
        env = dict(base)
        env.update(RUSTC=str(self.rustc), RUSTDOC=str(self.tool / 'rust/toolchain/bin/rustdoc'),
                   CARGO_HOME=str(self.tool / 'rust/cargo-home'), CARGO_NET_OFFLINE='true',
                   CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', RUSTFLAGS='-C panic=abort',
                   NEWT_DEV_BUILD_ID=build_id, NEWT_DEV_BUILD_DATE='host')
        return env

    def dependencies(self, extra_features=()):
        """Build the firmware's dependency crates; records their rlibs."""
        packages = tomllib.loads((FIRMWARE / 'Cargo.toml').read_text())['dependencies']
        target = self.out / 'target'
        result = self.run([self.cargo, 'build', '--lib', *[v for p in packages for v in ('-p', p)],
             '--features', ','.join((*BASE_FEATURES, *extra_features)),
             '--target', HOST, '--manifest-path', ROOT / 'rust/Cargo.toml',
             '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', target,
             '--message-format=json'])
        for line in result.stdout.splitlines():
            item = json.loads(line)
            if item.get('reason') == 'compiler-artifact':
                for file in item['filenames']:
                    if file.endswith('.rlib'):
                        self.libraries[item['target']['name']] = Path(file)
        self.deps = target / HOST / 'debug/deps'

    def book_includes(self):
        sdk = ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
        return [v for p in (sdk / 'include', sdk / 'third_party/expat',
                sdk / 'third_party/miniz', sdk / 'third_party/libunibreak') for v in ('-I', p)]

    def parser_objects(self, board):
        """The pinned Expat/libunibreak/XML bridge natives plus `board`, the
        caller's Board fake source. The SDK bytes must match the Rust gate's pins."""
        # The phased reader gate's own pin file, read directly: importing its
        # harness would pull that harness's whole closure into every gate procedure.
        pins = json.loads((ROOT / 'rust/tests/phased_reader/native-inputs.json').read_text())
        for relative, sha in pins['sha256'].items():
            assert hashlib.sha256((ROOT / 'vendor/freeink-sdk' / relative).read_bytes()).hexdigest() == sha, \
                'native product source: ' + relative
        sdk = ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
        for name in (*NATIVE, 'bridge', 'board'):
            source = (ROOT / 'rust/target-expat/native/expat_bridge.c' if name == 'bridge'
                      else board if name == 'board' else sdk / 'src/vendor' / (name + '.c'))
            self.c99(name, source, *self.book_includes(), '-I', SALAMANDER / 'board/native')

    def c99(self, name, source, *flags, defines=()):
        obj = self.out / (name + '.o')
        self.run([self.clang, '--no-default-config', '-x', 'c', '-std=c99', *defines, '-O1', *flags,
                  '-c', source, '-o', obj])
        self.objects.append(obj)
        return obj

    def generate_sources(self):
        """Copy every firmware module (not lib.rs) under OUTPUT/source; returns
        {original: copy}. Edit copies, then call `wire_sources`."""
        generated = self.out / 'source'
        generated.mkdir(exist_ok=True)
        for source in FIRMWARE.glob('*.rs'):
            if source.name == 'lib.rs':
                continue
            dest = generated / source.relative_to(FIRMWARE)
            if dest.exists():
                dest.unlink()
            text = _module_paths(source.read_text(), source.parent / source.stem)
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(_includes(resolve_paths(text, source.parent), source.parent))
            self.replacements[source] = dest
        return self.replacements

    def copy(self, name):
        return self.replacements[FIRMWARE / name]

    def wire_sources(self):
        """Every copy names the other copies, not the live firmware files."""
        for generated in self.replacements.values():
            generated.write_text(resolve_paths(generated.read_text(), FIRMWARE, self.replacements))

    def native_io(self, edit=lambda text: text):
        """The actual POSIX native_io.c after `edit` (a fixture's hooks)."""
        native = edit((SALAMANDER / 'native_io/native/native_io.c').read_text())
        (self.out / 'io.c').write_text(native)
        return self.out / 'io.c'

    def posix_objects(self, io, ports, extra=()):
        """io.c, the caller's ports.c, the actual posix_directory.c, any extra
        C sources, and the fake SHA provider."""
        for name, source in [('io', io), ('ports', ports),
                             ('directory', SALAMANDER / 'native_io/native/posix_directory.c'), *extra]:
            self.c99(name, source, '-I', SALAMANDER / 'native_io/native', defines=('-D_DEFAULT_SOURCE',))
        obj = self.out / 'hash.o'
        self.run([self.clang, '--no-default-config', '-std=c++17', '-O1',
                  '-I', ROOT / 'rust/tests/reader_providers/fake', '-I', ROOT / 'rust/tests/include',
                  '-c', ROOT / 'rust/tests/reader_providers/fake/fake_mbedtls.cpp', '-o', obj])
        self.objects.append(obj)

    def root(self, host_root, *, features=('dev-usb', 'rust-app'), modules=()):
        """lib.rs without its target entry, then `modules` ((path, name)
        pairs) and the caller's host root source appended. With no
        `host_root`, lib.rs keeps its native entry newt_rust_app_run and the
        program has no Rust main: a C object's main() calls that entry, as
        the device's SDK entry (rust/firmware/native/entry.cpp) does."""
        original = (FIRMWARE / 'lib.rs').read_text()
        end = '/// # Safety\n/// Called once' if host_root else '// SAFETY: the SDK calls this exact callback'
        original = (original[:original.index('// SAFETY: IDF abort')]
                    + original[original.index('pub mod app_image;'):original.index(end)])
        original = original.replace('#![no_std]', '' if host_root else '#![no_main]').replace(
            '#[global_allocator]\nstatic HEAP: newt_sdk_heap::SdkHeap = newt_sdk_heap::SdkHeap;', '')
        original = resolve_paths(_module_paths(original, FIRMWARE), FIRMWARE, self.replacements)
        original += '\nfn stop() -> ! { std::process::abort() }\n'
        for path, name in modules:
            original += '\n#[path = ' + json.dumps(str(path)) + '] mod ' + name + ';\n'
        if host_root:
            original += resolve_paths(Path(host_root).read_text(), Path(host_root).parent)
        host = self.out / 'host.rs'
        host.write_text(original)
        self.host = host
        self.features = features
        return host

    def link(self, program):
        """Compile and link the host root; on the first link, unresolved
        natives become traps. Later links (controls) reuse that command."""
        if self.command is None:
            command = [self.rustc, *[v for f in self.features for v in ('--cfg', f'feature="{f}"')],
                       '--edition=2024', '--target', HOST, '-Copt-level=1', '-Cpanic=abort',
                       '-Clinker=' + str(self.clang), '-Clink-arg=--no-default-config',
                       '-Clink-arg=-fuse-ld=/usr/bin/x86_64-linux-gnu-ld.bfd',
                       '-Ldependency=' + str(self.deps)]
            for name, library in self.libraries.items():
                command += ['--extern', name + '=' + str(library)]
            command += ['-Clink-arg=' + str(obj) for obj in self.objects]
            command += ['-Clink-arg=-lcrypto', '-Clink-arg=-lstdc++']
            linked = self.run([*command, self.host, '-o', program], succeeds=False)
            if linked.returncode:
                missing = sorted(set(re.findall("undefined reference to `([^']+)'", linked.stderr)))
                assert missing and all(re.fullmatch(r'_?(?:nb|np|nsc|nh|newt)_[A-Za-z0-9_]+', name)
                                       for name in missing), linked.stderr
                assert not any(name.startswith('newt_io_') for name in missing), linked.stderr
                (self.out / 'traps.c').write_text('extern void fixture_unexpected(const char*);\n' + '\n'.join(
                    'void ' + name + '(void) { fixture_unexpected("' + name + '"); }' for name in missing))
                trap = self.out / 'traps.o'
                self.run([self.clang, '--no-default-config', '-x', 'c', '-c', self.out / 'traps.c', '-o', trap])
                command += ['-Clink-arg=' + str(trap)]
                self.command = command
                self.run([*command, self.host, '-o', program])
            else:
                self.command = command
            return program
        self.run([*self.command, self.host, '-o', program])
        return program


def source_hashes(names):
    return {name: hashlib.sha256((FIRMWARE / name).read_bytes()).hexdigest() for name in names}


def logged_runner(out, env, timeout=120):
    """A runner that keeps each command's output as NN.stdout/NN.stderr and
    the list in commands.json."""
    commands = []
    def run(command, succeeds=True):
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                                capture_output=True, text=True, timeout=timeout)
        index = len(commands)
        (out / f'{index:02d}.stdout').write_text(result.stdout)
        (out / f'{index:02d}.stderr').write_text(result.stderr)
        commands.append({'command': list(map(str, command)), 'exit': result.returncode})
        (out / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        if succeeds and result.returncode:
            raise RuntimeError(result.stderr[-14000:])
        return result
    run.commands = commands
    return run
