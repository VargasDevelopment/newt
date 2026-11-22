"""Compile the patched facade and both X3 controllers with recording buses."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from host_common import COMPILER

ROOT = Path(__file__).resolve().parents[1]
STUBS = ROOT / 'vendor/freeink-sdk/libs/display/FreeInkDisplay/test/host/pro_stubs'
BUILD_ROOT = ROOT / '.build'
SDK = LIB = BUILD = None


def run_suite():
    BUILD.mkdir(exist_ok=True)
    for folder in ('src/driver', 'src/lut', 'src/bus', 'include'):
        (BUILD / folder).mkdir(parents=True, exist_ok=True)
    for name in ('FreeInkDisplay.h', 'GrayscaleCapabilities.h'):
        shutil.copy2(LIB / 'include' / name, BUILD / 'include' / name)
    shutil.copy2(LIB / 'src/FreeInkDisplay.cpp', BUILD / 'src/FreeInkDisplay.cpp')
    for name in ('PanelDriver.h', 'Uc8253X3Driver.h', 'Uc8253X3Driver.cpp', 'Uc8279Driver.h', 'Uc8279Driver.cpp'):
        shutil.copy2(LIB / 'src/driver' / name, BUILD / 'src/driver' / name)
    for name in ('Uc8253X3Luts.h', 'UltraChipDirectGrayLuts.h', 'Uc8279X3Luts.h'):
        shutil.copy2(LIB / 'src/lut' / name, BUILD / 'src/lut' / name)
    for name in ('Arduino.h', 'SPI.h'):
        shutil.copy2(STUBS / name, BUILD / name)
    board = (STUBS / 'BoardConfig.h').read_text()
    board = board.replace('displayWidth=800, displayHeight=480', 'displayWidth=792, displayHeight=528')
    board = board.replace('MAX_FRAMEBUFFER_BYTES=48000', 'MAX_FRAMEBUFFER_BYTES=52272')
    board += '\n#define FREEINK_DRIVER_UC8253_X3 1\n#define FREEINK_DRIVER_UC8279 1\n#define FREEINK_FB_PSRAM 0\n'
    (BUILD / 'BoardConfig.h').write_text(board)
    bus = (STUBS / 'EpdBus.h').read_text()
    bus = bus.replace('class EpdBus {\n public:', '''class EpdBus {
 public:
  inline static EpdBus* recording = nullptr;
  EpdBus() { recording = this; }
  void sendPlaneFlipped(uint8_t c, const uint8_t* p, uint16_t h, uint16_t wb) {
    cmd(c); beginTxn();
    for (int y = h - 1; y >= 0; --y) rawWriteBytes(p + y * wb, wb);
    endTxn();
  }
  void sendPlaneFlippedInverted(uint8_t c, const uint8_t* p, uint16_t h, uint16_t wb) {
    cmd(c); beginTxn();
    for (int y = h - 1; y >= 0; --y) for (unsigned x = 0; x < wb; ++x) {
      const uint8_t value = static_cast<uint8_t>(~p[y * wb + x]);
      rawWriteBytes(&value, 1);
    }
    endTxn();
  }''')
    (BUILD / 'src/bus/EpdBus.h').write_text(bus)
    api_flags = [COMPILER, '-std=c++17', '-Wall', '-Wextra', '-Werror', '-pedantic',
                 '-fsyntax-only', '-DARDUINO=1', '-I' + str(BUILD),
                 '-I' + str(BUILD / 'include'), '-I' + str(BUILD / 'src'), '-x', 'c++', '-']
    api_prefix = '''
#include "FreeInkDisplay.h"
using freeink::BinaryGrayRegion;
using freeink::BinaryGrayRegionPlan;
using freeink::FreeInkDisplay;
using freeink::GrayscaleMode;
'''
    positive = subprocess.run(api_flags, input=api_prefix + '''
constexpr BinaryGrayRegion regions[]={{568,374,8,108}};
void use(FreeInkDisplay& display,const unsigned char* plane) {
  display.displayGrayscaleBaseWithBinaryRegions<regions,1>(GrayscaleMode::Direct);
  display.displayGrayscaleBaseWithBinaryRegions<regions,1>(GrayscaleMode::Overlay);
  display.displayBinaryGrayRegions<regions,1>(plane,1);
  display.displayGrayscaleBaseWithBinaryRegions<regions,1,true>(GrayscaleMode::Direct);
  display.displayGrayscaleBaseWithBinaryRegions<regions,1,true>(GrayscaleMode::Overlay);
  display.displayBinaryGrayRegions<regions,1,true>(plane,1);
}
''', text=True, capture_output=True, cwd=ROOT)
    if positive.returncode:
        raise AssertionError('static binary-region binding must compile:\n' + positive.stderr)
    rejected = {
        'automatic regions': '''
void use(FreeInkDisplay& display) {
  constexpr BinaryGrayRegion regions[]={{568,374,8,108}};
  display.displayGrayscaleBaseWithBinaryRegions<regions,1>(GrayscaleMode::Direct);
}
''',
        'automatic plan': '''
void use(FreeInkDisplay& display) {
  constexpr BinaryGrayRegion regions[]={{568,374,8,108}};
  const BinaryGrayRegionPlan plan{regions,1};
  display.displayGrayscaleBaseWithBinaryRegions(GrayscaleMode::Direct,plan);
}
''',
        'temporary plan': '''
constexpr BinaryGrayRegion regions[]={{568,374,8,108}};
void use(FreeInkDisplay& display) {
  display.displayGrayscaleBaseWithBinaryRegions(
      GrayscaleMode::Direct,BinaryGrayRegionPlan{regions,1});
}
''',
        'mutable static regions': '''
BinaryGrayRegion regions[]={{568,374,8,108}};
void use(FreeInkDisplay& display) {
  display.displayGrayscaleBaseWithBinaryRegions<regions,1>(GrayscaleMode::Direct);
}
''',
        'neutral gaps with different X': '''
constexpr BinaryGrayRegion regions[]={{568,374,8,1},{576,376,8,1}};
void use(FreeInkDisplay& display) {
  display.displayGrayscaleBaseWithBinaryRegions<regions,2,true>(GrayscaleMode::Direct);
}
''',
        'neutral gaps exceeding strip checkpoint': '''
constexpr BinaryGrayRegion regions[]={{568,0,8,1},{568,527,8,1}};
void use(FreeInkDisplay& display) {
  display.displayGrayscaleBaseWithBinaryRegions<regions,2,true>(GrayscaleMode::Direct);
}
''',
    }
    for name, source in rejected.items():
        result = subprocess.run(api_flags, input=api_prefix + source, text=True,
                                capture_output=True, cwd=ROOT)
        if result.returncode == 0:
            raise AssertionError(f'{name} unexpectedly satisfies static binary-region binding')
    for uc8279 in (False, True):
        for enabled in (False, True):
            for single in (False, True):
                binary = BUILD / f'regional-uc8279-{int(uc8279)}-{int(enabled)}-single-{int(single)}'
                command = [COMPILER, '-std=c++17', '-O1', '-g', '-Wall', '-Wextra', '-Werror',
                           '-Wno-unused-parameter', '-Wno-unused-function', '-pedantic',
                           '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-DARDUINO=1',
                           '-I' + str(BUILD), '-I' + str(BUILD / 'include'), '-I' + str(BUILD / 'src')]
                if enabled:
                    command += ['-DFREEINK_EXPERIMENTAL_REGIONAL_GRAY=1']
                if uc8279:
                    command += ['-DTEST_UC8279=1']
                if single:
                    command += ['-DEINK_DISPLAY_SINGLE_BUFFER_MODE=1']
                command += [str(ROOT / 'tests/home_refresh_regional_test.cpp'),
                            str(BUILD / 'src/FreeInkDisplay.cpp'), str(BUILD / 'src/driver/Uc8253X3Driver.cpp'),
                            str(BUILD / 'src/driver/Uc8279Driver.cpp'),
                            '-o', str(binary)]
                subprocess.run(command, check=True, cwd=ROOT)
                subprocess.run([str(binary)], check=True, cwd=ROOT)

    # Existing upstream real-driver tests exercise the unchanged default-off
    # behavior. Keep their temporary compilation directories project-local.
    environment = dict(os.environ, TMPDIR=str(BUILD.parent))
    for runner in ('run_uc8253_power.py', 'run_uc8279.py', 'run_pro.py'):
        subprocess.run(['python3', str(LIB / 'test/host' / runner)], check=True, cwd=ROOT, env=environment)


def main():
    global SDK, LIB, BUILD
    BUILD_ROOT.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='freeink-regional-', dir=BUILD_ROOT) as temporary:
        work = Path(temporary)
        SDK = work / 'sdk'
        SDK.mkdir()
        archive = work / 'sdk.tar'
        subprocess.run(['git', '-C', str(ROOT / 'vendor/freeink-sdk'), 'archive',
                        '--format=tar', '-o', str(archive), 'HEAD'], check=True)
        subprocess.run(['tar', '-xf', str(archive), '-C', str(SDK)], check=True)
        subprocess.run(['patch', '--batch', '--forward', '-p1',
                        '-i', str(ROOT / 'patches/freeink-regional-gray.patch')],
                       check=True, cwd=SDK)
        LIB = SDK / 'libs/display/FreeInkDisplay'
        BUILD = work / 'build'
        run_suite()


if __name__ == '__main__':
    main()
