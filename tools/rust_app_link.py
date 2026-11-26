"""Link the Rust app archive into the bench image's existing native SDK build."""
from pathlib import Path

Import('env')
root = Path(env.subst('$PROJECT_DIR'))
linker = root / '.build/rust-app/ld/rust_cfi.ld'
linker.parent.mkdir(parents=True, exist_ok=True)
# Rust aborts across the SDK boundary; keep its unwind rules offline in the ELF.
linker.write_text('''SECTIONS {
  .debug_frame 0 : {
    __newt_rust_debug_cfi_start = .;
    *libnewt_app_image.a:newt_app_image-*.rcgu.o(.debug_frame)
    __newt_rust_debug_cfi_end = .;
    *(.debug_frame)
  }
  /DISCARD/ : { *libnewt_app_image.a:newt_app_image-*.rcgu.o(.eh_frame) }
}
INSERT BEFORE .flash.rodata;
ASSERT(__newt_rust_debug_cfi_end > __newt_rust_debug_cfi_start,
       "Rust debugger CFI missing; rebuild Rust before linking")
''')
env.Append(LIBS=[env.File('$PROJECT_DIR/.build/rust-app/target/riscv32imc-unknown-none-elf/release/libnewt_app_image.a')],
           LINKFLAGS=['-Wl,-T,' + str(linker)])


# tools/verify-wifi-link.py reads code's addresses from the linker's
# relocations. A second link of the same inputs keeps them, in
# firmware.relocs.elf with its own map, so firmware.elf, firmware.map and
# firmware.bin stay exactly as they are. The verifier stops unless both
# ELFs load the same bytes. It is its own target in the default build, so
# SCons relinks it whenever it is missing or out of date.
side = env.Clone(PROGNAME='firmware.relocs')
side.Append(LINKFLAGS=['-Wl,--emit-relocs'])
env.Alias('buildprog', side.Program(side.subst('$PROGPATH'), env['PIOBUILDFILES']))
