# SPDX-License-Identifier: MIT
"""Native fake-mbedTLS and libunibreak object builder checked by rust/tests/expected_test.py."""
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PIN = 'bc9ffda52b85f277e46c63718ac88d62fba5d368'
SDK_PIN = 'cd6f5b831c8fad899723b6d406586f00c575f021'
HOST = 'x86_64-unknown-linux-gnu'
SANITIZE = ['-fsanitize=address,undefined', '-fno-sanitize-recover=all']


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def native(run, support, closure, crypto):
    """Only genuine shared fake-mbedTLS and native libunibreak objects."""
    load('native_crypto', ROOT/'rust/tests/native_crypto.py').verify(ROOT)
    sdk = ROOT / 'vendor/freeink-sdk'
    sdk_provenance = json.loads((ROOT / 'rust/tests/page_renderer/PROVENANCE.json').read_text())
    support.require(sdk_provenance['reference'] == PIN and sdk_provenance['commit'] == SDK_PIN,
                    'pinned SDK identity')
    load('provider_native_inputs', ROOT/'rust/tests/phased_reader/native.py').verify_native_inputs(support, sdk)
    book = sdk / 'libs/book/FreeInkBook'
    unibreak = book / 'third_party/libunibreak'
    common = ['-O1', '-g', *SANITIZE, '-fno-omit-frame-pointer', '-ffunction-sections', '-fdata-sections']
    paths = [HERE / 'fake', ROOT / 'rust/tests/include', book / 'include',
             sdk / 'libs/font/FreeInkFont/include', book / 'third_party/expat',
             book / 'third_party/miniz', unibreak]
    cxx = ['-std=c++17', '-DFREEINK_BOOK_SMALL=1', '-DNEWT_DEV_USB=0', *common,
           *[value for path in paths for value in ('-I', path)], *crypto[:2]]
    units = {'fake-mbedtls': (HERE / 'fake/fake_mbedtls.cpp', cxx),
             'unibreak': (book / 'src/vendor/unibreak_impl.c', ['-x', 'c', '-std=c99', *common, '-I', unibreak])}
    objects = {}
    for name, (unit, flags) in units.items():
        objects[name] = run.output / (name + '.o')
        run.run([run.tools['clang++'], '--no-default-config', *flags, '-c', unit, '-o', objects[name]], ROOT)
    shared = [objects['fake-mbedtls'], objects['unibreak'], *SANITIZE, '-lstdc++', *crypto[2:]]
    return None, shared
