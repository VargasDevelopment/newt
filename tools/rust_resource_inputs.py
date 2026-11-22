"""Compiler inputs and generated archive consumed by the single Rust image."""
import configparser
from pathlib import Path
import re

from daily_firmware import SOURCE_CONFIGURATION, require_configuration

ARCHIVE = '.build/rust-app/target/riscv32imc-unknown-none-elf/release/libnewt_app_image.a'


def native_sources(root):
    require_configuration(root)
    config = configparser.ConfigParser(interpolation=None)
    config.read(Path(root) / SOURCE_CONFIGURATION)
    filters = config['env:newt_x3']['build_src_filter'].split()
    names = [re.fullmatch(r'\+<([A-Za-z0-9_./-]+\.(?:cpp|c))>', word) for word in filters]
    if not names or any(name is None for name in names):
        raise ValueError('Daily native source filters must name exact translation units')
    sources = [Path(name[1]) for name in names]
    if (len(set(sources)) != len(sources)
            or any(path.is_absolute() or '..' in path.parts for path in sources)):
        raise ValueError('Daily native sources must be unique project-relative paths')
    return sources


def archive_dependencies(root):
    """Rustc's archive rule, source phony rules and documented env-dep comments."""
    root = Path(root)
    archive = root / ARCHIVE
    depfile = archive.with_suffix('.d')
    dependencies, found = set(), False
    for line in depfile.read_text().replace('\\\r\n', '').replace('\\\n', '').splitlines():
        if not line.strip() or line.startswith('# env-dep:'):
            continue
        tokens = re.findall(r'(?:\\.|[^\s\\:])+|:', line)
        if tokens.count(':') != 1 or tokens.index(':') != 1:
            raise ValueError('Invalid Rust archive dependency rule')
        tokens = [re.sub(r'\\(.)', r'\1', token).replace('$$', '$') for token in tokens]
        target = (root / tokens[0]).resolve()
        if target == archive.resolve():
            if found or len(tokens) < 3:
                raise ValueError('Missing or repeated Rust archive dependency closure')
            found = True
            dependencies.update((root / name).resolve() for name in tokens[2:])
        elif target == depfile.resolve():
            continue
        elif len(tokens) != 2:
            raise ValueError('Unexpected Rust archive dependency target')
    if not found or not dependencies:
        raise ValueError('Missing Rust archive dependency closure')
    return dependencies


def generated_inputs(root):
    """The sole product build.rs emits only this readonly shared-notes leaf."""
    root = Path(root)
    paths = [path for path in archive_dependencies(root) if path.is_relative_to(root / '.build')]
    pattern = (r'\.build/rust-app/target/riscv32imc-unknown-none-elf/release/build/'
               r'newt-device-ui-[0-9a-f]{16}/out/whats_new\.rs')
    if len(paths) > 1 or any(not re.fullmatch(pattern, path.relative_to(root).as_posix()) for path in paths):
        raise ValueError('Unbound generated Rust compiler input')
    return paths
