"""Run the actual heap clear (rust/firmware/native/heap_clear.h) through IDF's startup heap
registration (tests/heap_clear_model.cpp), linked with platformio-rust-app.ini's wraps:
each step registers only cleared RAM, the live startup stack survives the first
step, and reserved data stays. Each control changes one line of the firmware
or the wraps."""
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from host_common import COMPILER
from host_objects import compiler_environment
sys.path.insert(0, str(Path(__file__).resolve().parent))
from source_fixture import patched_firmware, replace_once

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / '.build/heap-clear'
MODEL = ROOT / 'tests/heap_clear_model.cpp'
FLAGS = ['-std=c++17', '-O1', '-g', '-Wall', '-Wextra', '-Werror', '-pedantic', '-fsanitize=address,undefined',
         '-fno-omit-frame-pointer', '-isystem', str(ROOT / 'tests/heap_clear_idf')]
PARTS = ('HEAP_CLEAR_LAYOUT', 'HEAP_CLEAR_HEAP', 'HEAP_CLEAR_STARTUP')


def wraps(platformio):
    return re.findall(r'(?m)^\s*(-Wl,--wrap=\w+)\s*$', platformio)


def build(name, firmware, platformio):
    directory = BUILD / name
    directory.mkdir(parents=True, exist_ok=True)
    (directory / 'firmware.cpp').write_text('#include "heap_clear.h"\n')
    environment = compiler_environment(ROOT)
    objects = []
    for source, part in [(directory / 'firmware.cpp', None), *((MODEL, part) for part in PARTS)]:
        objects.append(directory / f'{part or "firmware"}.o')
        subprocess.run([COMPILER, '--no-default-config', *FLAGS, '-iquote', str(firmware),
                        *([f'-D{part}'] if part else []), '-c', str(source), '-o', str(objects[-1])],
                       check=True, cwd=ROOT, env=environment)
    binary = directory / 'heap-clear'
    linked = subprocess.run([COMPILER, '--no-default-config', *FLAGS, *map(str, objects), *wraps(platformio),
                             '-o', str(binary)], cwd=ROOT, env=environment, capture_output=True, text=True)
    return binary, linked


def run(name, firmware=ROOT / 'rust/firmware/native', platformio=None, expected=None):
    binary, linked = build(name, firmware, platformio or (ROOT / 'platformio-rust-app.ini').read_text())
    if expected == 'link':
        if linked.returncode == 0 or '__real_' not in linked.stderr:
            raise AssertionError(f'heap clear control linked {name}: {linked.stderr}')
        print(f'PASS negative control: {name}: the build refuses a missing wrap', flush=True)
        return
    if linked.returncode:
        raise AssertionError(linked.stderr)
    result = subprocess.run([str(binary)], cwd=ROOT, capture_output=True, text=True,
                            env=compiler_environment(ROOT))
    if expected is None:
        if result.returncode:
            raise AssertionError(result.stderr)
        print(result.stdout, end='', flush=True)
    elif result.returncode != 1 or f'FAIL heap clear: {expected}' not in result.stderr:
        raise AssertionError(f'heap clear control accepted {name}: {result.stderr}')
    else:
        print(f'PASS negative control: {name}: {expected}', flush=True)


STACK = ('  newt::firmware::clearHeapRegions(regions, __real_soc_get_available_memory_regions(regions), true);\n'
         '  __real_heap_caps_enable_nonos_stack_heaps();\n')
CONTROLS = (
    ('  newt::firmware::clearHeapRegions(regions, count, false);\n', '',
     'heap_caps_init registers only cleared RAM'),
    (STACK, ''.join(reversed(STACK.splitlines(keepends=True))), 'main_task registers only cleared RAM'),
    ('regions[i].startup_stack == startupStack', '!startupStack || regions[i].startup_stack',
     'the live startup stack survives heap_caps_init'),
    ('regions[i].size);', 'regions[i].size + 64);', 'ROM and static data stay untouched'),
)


def main():
    shutil.rmtree(BUILD, ignore_errors=True)
    run('heap-clear')
    for index, (anchor, replacement, expected) in enumerate(CONTROLS):
        name = f'heap-clear-negative-{index}'
        run(name, patched_firmware(BUILD / f'{name}-firmware', 'heap_clear.h', anchor, replacement), expected=expected)
    platformio = (ROOT / 'platformio-rust-app.ini').read_text()
    for index, flag in enumerate(('soc_get_available_memory_regions', 'heap_caps_enable_nonos_stack_heaps')):
        run(f'heap-clear-unwrapped-{index}', platformio=replace_once(platformio, f'\t-Wl,--wrap={flag}\n', ''),
            expected='link')


if __name__ == '__main__':
    main()
