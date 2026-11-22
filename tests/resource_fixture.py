"""Shared production-backed resource-gate fixture; no test cases live here."""
import hashlib
import copy
import json
from pathlib import Path
import struct
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import code_health_resources as resources
from python_runtime_fixture import install_runtime_fixture

# Independent authored layout: fixtures must not inherit the parser's allowlist.
X3_PARTITIONS = (
    (1, 2, 0x9000, 0x5000, b"nvs", 0),
    (1, 0, 0xe000, 0x2000, b"otadata", 0),
    (0, 16, 0x10000, 0x770000, b"app0", 0),
    (0, 17, 0x780000, 0x770000, b"app1", 0),
    (1, 130, 0xef0000, 0x100000, b"spiffs", 0),
    (1, 3, 0xff0000, 0x10000, b"coredump", 0),
)


def partition_table(entries=X3_PARTITIONS):
    """Authored ESP partition bytes, including native MD5 record and padding."""
    body = b"".join(struct.pack("<HBBII16sI", 0x50aa, *entry) for entry in entries)
    table = body + b"\xeb\xeb" + b"\xff" * 14 + hashlib.md5(body).digest()
    return table + b"\xff" * (0xc00 - len(table))


SIZE_OUTPUT = """section size addr
.iram0.text 60 1077411840
.dram0.dummy 60 1070071808
.dram0.data 20 1070071868
.dram0.bss 80 1070071888
.flash.text 700 1107296256
.debug_info 900 0
"""
STACK = "rust/firmware/native/main.cpp:10:6:void loop()\t32\tstatic\n"
MEMORY_MAP = """Memory Configuration
iram0_0_seg      0x40380000         0x0004e710         xr
dram0_0_seg      0x3fc80000         0x0004e710         rw
"""
ACTUAL_SIZE_OUTPUT = """section size addr
.rtc.text 20 1342177280
.rtc.force_fast 0 1342177300
.rtc_noinit 80 1342177300
.rtc.force_slow 32 1342177380
.rtc_reserved 40 1342185432
.iram0.text 60164 1077411840
.dram0.dummy 60416 1070071808
.dram0.data 9634 1070132224
.noinit 2 1070141858
.dram0.bss 62328 1070141872
.flash.text 370310 1107296288
.flash_rodata_dummy 393216 1006632992
.flash.appdesc 256 1007026208
.flash.rodata 755348 1007026464
.eh_frame_hdr 0 1007781812
.eh_frame 34864 1007781812
.flash.tdata 0 1007816676
.iram0.text_end 252 1077472004
.iram0.data 0 1077472256
.iram0.bss 0 1077472256
.dram0.heap_start 8 1070204200
.debug_info 4743719 0
"""


# Authored names, independent of implementation constants.
BUILD = "regional-dev-usb"
# The current owner-set floors, authored here so a source edit trips tests.
FLOORS = {"reading_heap_min_free_bytes": 65536, "reading_heap_exception_min_free_bytes": 50468,
          "loop_stack_min_free_bytes": None}


def record(before, after, **extra):
    return {"source_revision": "b" * 40, "before": before, "after": after,
            "reason": "Authored fixture measurement", "reviewer": "Authored fixture reviewer", **extra}


def recorded(baseline):
    """Give every ceiling and floor a record that ends at its current value."""
    baseline.setdefault("floors", copy.deepcopy(FLOORS))
    budgets = baseline["resources"][BUILD]
    values = {**{name: budgets[name] for name in ("flash_bytes", "ram_bytes", "wifi_lent_bytes") if name in budgets},
              **budgets["stack_bytes"], **baseline["floors"]}
    baseline["records"] = {name: record(value, value) for name, value in values.items()}
    return baseline


class ResourceFixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for directory in ("rust/firmware/native", "assets", "apps", "patches", "vendor/freeink-sdk", "artifacts"):
            (self.root / directory).mkdir(parents=True)
        (self.root / "rust/firmware/native/main.cpp").write_text("void loop() {}")
        (self.root / "rust/firmware/native/nvs_guard.cpp").write_text("void guard() {}")
        (self.root / "rust/firmware/native/generated.h").write_text("#define VALUE 1")
        (self.root / "rust/firmware/native/firmware_version.h").write_text('#define NEWT_FIRMWARE_VERSION "0.1.0"\n')
        (self.root / "rust/firmware/native/reader.ipp").write_text("inline void reader() {}")
        (self.root / "rust/firmware/native/inline_impl.inl").write_text("inline void impl() {}")
        partitions = "rust/firmware/x3_existing_partitions.csv"
        (self.root / partitions).write_bytes((Path(__file__).resolve().parents[1] / partitions).read_bytes())
        (self.root / "rust/firmware/native/extensionless").write_text("inline void extra() {}")
        (self.root / "vendor/freeink-sdk/pinned.h").write_text("// vendor")
        for variant in resources.VARIANTS:
            sdk = self.root / resources.reader_sdk(variant)
            for library in resources.SDK_LIBRARIES:
                marker = sdk / library / "route.h"
                marker.parent.mkdir(parents=True, exist_ok=True)
                marker.write_text(f"// {variant} {library}\n")
            (self.root / resources.reader_sdk_provenance(variant)).write_text(
                json.dumps({"lane": variant, "fixture": "composed reader SDK"}))
        (self.root / "platformio.ini").write_text("[env:newt_x3]\n")
        project = Path(__file__).resolve().parents[1]
        (self.root / 'platformio-rust-app.ini').write_bytes((project / 'platformio-rust-app.ini').read_bytes())
        self.native_sources = resources.native_sources(self.root)
        for source in self.native_sources:
            path = self.root / source
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('// daily native source fixture\n')
        (self.root / 'rust/firmware/main.rs').write_text('// Rust source fixture\n')
        rust_tools = self.root / '.toolchain/rust/toolchain/bin'
        rust_tools.mkdir(parents=True)
        (rust_tools / 'rustc').write_bytes(b'pinned Rust compiler')
        (self.root / '.toolchain/rust/installed.json').write_text('{"lock":"fixture"}')
        self.procedure_names = resources.PROCEDURE_NAMES
        (self.root / "tools").mkdir()
        for name in self.procedure_names:
            (self.root / "tools" / name).write_text("# authored build procedure")
        png_helper = self.root / "assets/frosted-home/source/png_tools.py"
        png_helper.parent.mkdir(parents=True)
        png_helper.write_text("# imported PNG helper")
        package = self.root / ".toolchain/platformio/packages/toolchain-riscv32-esp"
        (package / "bin").mkdir(parents=True)
        (package / "package.json").write_text('{"version":"pinned"}')
        for tool in ("g++", "gcc", "as", "ld", "size", "objcopy"):
            (package / "bin" / ("riscv32-esp-elf-" + tool)).write_text("pinned executable")
        backends = package / "libexec/gcc/riscv32-esp-elf/14.2.0"
        backends.mkdir(parents=True)
        for tool in ("cc1", "cc1plus", "collect2", "lto1", "lto-wrapper", "liblto_plugin.so"):
            (backends / tool).write_text("pinned backend")
        standard = package / "riscv32-esp-elf/include/c++/14.2.0"
        standard.mkdir(parents=True)
        (standard / "vector").write_text("// pinned standard header")
        runtime = package / "riscv32-esp-elf/lib"
        runtime.mkdir(parents=True)
        (runtime / "libstdc++.a").write_bytes(b"pinned runtime archive")
        platform = self.root / ".toolchain/platformio/platforms/espressif32"
        platform.mkdir(parents=True)
        (platform / "platform.json").write_text('{"version":"pinned"}')
        (platform / "platform.py").write_text("# pinned recipe")
        (platform / "boards").mkdir()
        (platform / "boards/esp32-c3-devkitm-1.json").write_text('{"board":"pinned"}')
        (platform / "builder").mkdir()
        (platform / "builder/main.py").write_text("# pinned builder")
        frameworks = self.root / ".toolchain/platformio/packages"
        core = frameworks / "framework-arduinoespressif32/cores/esp32"
        core.mkdir(parents=True)
        (core / "esp32-hal-gpio.c").write_text("void gpio() {}")
        libraries = frameworks / "framework-arduinoespressif32-libs/esp32c3"
        (libraries / "lib").mkdir(parents=True)
        (libraries / "include").mkdir()
        (libraries / "ld").mkdir()
        (libraries / "lib/libheap.a").write_bytes(b"pinned precompiled C3 archive")
        (libraries / "include/sdk.h").write_text("#define SDK_PIN 1")
        (libraries / "ld/sections.ld").write_text("/* pinned linker */")
        self.build_driver_files = (
            "platforms/espressif32/__pycache__/platform.cpython-312.pyc",
            "platforms/espressif32/builder/__pycache__/main.cpython-312.pyc",
            "penv/lib/python3.12/site-packages/platformio/builder/main.py",
            "penv/lib/python3.12/site-packages/click/core.py",
            "penv/lib/python3.12/site-packages/platformio/__pycache__/main.cpython-312.pyc",
            "packages/tool-scons/scons.py",
            "packages/tool-scons/scons-local-4.8.1/SCons/Script/Main.py",
            "packages/tool-scons/scons-local-4.8.1/SCons/Script/__pycache__/Main.cpython-312.pyc",
            "packages/tool-esptoolpy/esptool.py",
            "packages/tool-esptoolpy/esptool/bin_image.py",
            "packages/tool-esptoolpy/esptool/__pycache__/bin_image.cpython-312.pyc",
            "packages/tool-esp_install/esp_install.py",
            "packages/tool-esp_install/__pycache__/esp_install.cpython-312.pyc",
            "penv/bin/pio", "penv/bin/platformio", "penv/pyvenv.cfg",
        )
        for name in self.build_driver_files:
            path = self.root / ".toolchain/platformio" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"pinned build driver implementation")
        for name in ("tool-scons", "tool-esptoolpy", "tool-esp_install"):
            (frameworks / name / "package.json").write_text('{"version":"pinned"}')
        self.interpreter = self.root / ".toolchain/python-runtime/bin/python3.12"
        self.interpreter.parent.mkdir(parents=True)
        self.interpreter.write_bytes(b"pinned Python interpreter executable")
        (self.root / ".toolchain/platformio/penv/bin/python").symlink_to(self.interpreter)
        install_runtime_fixture(self)
        self.sdk_validation_patch = patch.object(resources, "validate_reader_copy")
        self.sdk_validation = self.sdk_validation_patch.start()
        self.addCleanup(self.sdk_validation_patch.stop)
        self.size = patch.object(resources, "subprocess")
        self.command = self.size.start().run
        self.addCleanup(self.size.stop)
        self.command.return_value.stdout = SIZE_OUTPUT
        from rust_open_stack import OPEN_PROFILE
        frame = patch.object(resources.rust_image, 'functions', return_value={OPEN_PROFILE: [
            '42000000: 0001 addi sp,sp,-112', '42000002: 0001 addi sp,sp,112', '42000004: 8082 ret']})
        frame.start()
        self.addCleanup(frame.stop)
        self.budget = {"resources": {variant: {"ram_bytes": 160, "flash_bytes": 96,
                                                  "stack_bytes": {"rust/firmware/native/main.cpp::void loop()": 32}}
                                     for variant in resources.VARIANTS}}
        recorded(self.budget)
        self.directory = resources.build_directory(self.root, BUILD)
        self.report = self.build()
        self.save()

    def build(self, variant=BUILD, flash_bytes=96):
        before = time.time_ns()
        config = resources.configuration(self.root, variant)
        config.parent.mkdir(parents=True, exist_ok=True)
        config.write_text("// generated fixture config with its build ID")
        identity = resources.build_identity(self.root, variant)
        directory = resources.build_directory(self.root, variant)
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "firmware.elf").write_bytes(b"elf")
        prefix = directory.relative_to(self.root).as_posix()
        (directory / "firmware.map").write_text(MEMORY_MAP + f"LOAD {prefix}/src/rust/firmware/native/main.cpp.o\nLOAD {prefix}/src/rust/firmware/native/nvs_guard.cpp.o\n")
        with (directory / 'firmware.map').open('a') as stream:
            stream.write('LOAD ' + resources.ARCHIVE + '\n')
        archive = self.root / resources.ARCHIVE
        archive.parent.mkdir(parents=True, exist_ok=True)
        archive.write_bytes(b'fresh Rust archive')
        archive.with_suffix('.d').write_text(resources.ARCHIVE + ': rust/firmware/main.rs\n')
        for index, source in enumerate(self.native_sources):
            obj = directory / 'src' / source.with_suffix(source.suffix + '.o')
            obj.parent.mkdir(parents=True, exist_ok=True)
            obj.write_bytes(b'compiled native object')
            obj.with_suffix('.d').write_text(obj.relative_to(self.root).as_posix() + ': ' + source.as_posix() + '\n')
            obj.with_suffix('.su').write_text(f'{source}:1:1:void native_{index}()\t16\tstatic\n')
        (directory / "firmware.bin").write_bytes(b"f" * flash_bytes)
        (directory / "partitions.bin").write_bytes(partition_table())
        stacks = directory / "src/rust/firmware/native"
        stacks.mkdir(parents=True, exist_ok=True)
        (stacks / "main.cpp.su").write_text(STACK)
        (stacks / "nvs_guard.cpp.su").write_text("rust/firmware/native/nvs_guard.cpp:2:6:void guard()\t16\tstatic\n")
        for name in ("main", "nvs_guard"):
            (stacks / f"{name}.cpp.o").write_bytes(b"compiled object")
            sdk_routes = "" if name != "main" else " " + " ".join(
                next(path for path in sorted((self.root / resources.reader_sdk(variant) / library).rglob("*"))
                     if path.is_file()).relative_to(self.root).as_posix()
                for library in resources.SDK_LIBRARIES)
            (stacks / f"{name}.cpp.d").write_text(
                f"{prefix}/src/rust/firmware/native/{name}.cpp.o: rust/firmware/native/{name}.cpp rust/firmware/native/generated.h{sdk_routes}\n")
        return resources.build_report(self.root, variant, config, before, identity)

    def save(self):
        (self.root / f"artifacts/code-health-{BUILD}.json").write_text(json.dumps(self.report))

    def check(self):
        return resources.check_resources(self.root, self.budget, BUILD)
