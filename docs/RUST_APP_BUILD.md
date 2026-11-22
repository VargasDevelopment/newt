# Rust app image build

Use the approved, installed project-local toolchains described in
[toolchain-proposal.md](toolchain-proposal.md), plus the pinned Rust components
in `rust/toolchain.json`. The build does not acquire dependencies. Check out
the pinned `vendor/freeink-sdk` submodule before building; no other Newt
checkout or its generated files are inputs.

```sh
python3 tools/rust-app-build.py --clean
```

This compiles Rust and links the native image under
`.build/rust-app/native/build/newt_x3`. `--rust-only` builds just the Rust
archive; `--native-only` relinks against an existing archive. The older
`--native` flag remains accepted. Build the full product feature set with
`--features wifi-update,embedded-sample`; the default bench feature set omits
those two features.

The compiler and Cargo home come from `.toolchain/rust`; PlatformIO comes from
`.toolchain/platformio`. The build prepares its SDK copy from the local pinned
submodule, derives its linker script from the local pinned IDF inputs, and
generates its TLS bundle from the local pinned CA list. Rust source paths are
remapped to relative paths in the image and debugger information.

Run the three linked checks against this build, using
`.toolchain/platformio/packages/toolchain-riscv32-esp/bin` as `TOOLCHAIN`:

```sh
python3 tools/verify-nvs-guard.py --rust-app --elf .build/rust-app/native/build/newt_x3/firmware.elf --toolchain-bin "$TOOLCHAIN" --output .build/rust-app/linked-nvs.json
python3 tools/verify-wifi-link.py --rust-app --elf .build/rust-app/native/build/newt_x3/firmware.elf --map .build/rust-app/native/build/newt_x3/firmware.map --toolchain-bin "$TOOLCHAIN" --output .build/rust-app/linked-wifi.json
python3 tools/verify-next-boot.py --toolchain-bin "$TOOLCHAIN" --elf .build/rust-app/native/build/newt_x3/firmware.elf --image .build/rust-app/native/build/newt_x3/firmware.bin --sdkconfig .toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3/sdkconfig
```

Compilation and linked guards are host evidence. Installing or testing an
image still requires the separate bench review and device lease.
