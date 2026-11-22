# Approved project-local firmware toolchain

Status: installed and the real X3 target compiles. The owner approved the exact bundle, then authorized continued necessary reputable project-local development dependencies. The 1.2 GB download /4 GB storage caps remain. No hardware, global installation, host security/network changes or public publication was authorized or performed.

Existing Apple Clang21/Python3.14.6 run the host gate. Existing `/opt/homebrew/bin/python3.12` (3.12.6) bootstrapped the embedded environment. No global package/profile/cache was changed.

## Locked inputs and footprint

`toolchain-lock.json` records63 acquired archives/wheels totaling **614,665,932 bytes**, with publisher URLs, computed SHA-256 and published digests where provided. All51 PyPI wheels were checked against their exact official version metadata before execution and pinned in `toolchain-python.lock`. Core fork source tag v6.1.19 resolves to `d3321155afaadd4e1d53373910c047132d48374b`. The esptool and Core wheels were built locally from verified source with locked setuptools/wheel, without build isolation, dependency resolution or network.

| Component | Exact version | Recorded archive size |
| --- | --- | ---: |
| PlatformIO PyPI bootstrap |6.1.19|421,133 B|
| pioarduino Core source |v6.1.19|538,731 B|
| pioarduino platform |55.03.37|1,678,517 B|
| Espressif Arduino source |3.3.7|20,697,240 B|
| Espressif prebuilt Arduino/IDF libraries |Arduino3.3.7 /IDF v5.5.2-729-g87912cd291|272,921,836 B|
| Espressif arm64 RISC-V GCC xz |14.2.0_20251107|288,327,120 B|
| pioarduino installer |5.3.4|54,015 B|
| pioarduino esptool source |5.1.2|506,890 B|
| SCons local source |4.8.1|483,365 B|
| piohome assets |3.4.4|922,895 B|
|51 Python wheels (includes bootstrap above)|Exact lock|28,531,638 B|

The pinned official descriptor **already chooses the smaller arm64 GCC xz archive**. The earlier proposal's gzip-selection assumption was corrected from that descriptor; no alternate compiler selection was needed.

All-target framework libraries expand to1,882,017,505 B and GCC to2,132,686,573 B. The standard installer would retain extra compiler copies and exceed the disk cap. We materialized GCC once directly into its canonical package directory, with the descriptor's official manifest. Framework libraries retain **only the208,080,086 B C3 directory and root metadata** from the unchanged verified archive; other processor directories are omitted. Arduino source is full. No compiler ABI, C3 libraries, platform code or FreeInk source were patched.

Pinned Core `ToolPackageManager.build_metadata`, `PackageSpec` and `PackageItem.dump_meta` registered those already materialized package directories against their exact original source URLs. This avoids expanded package copies; the platform's ordinary version checks recognize the installation. Tool descriptors' `tools.json` is omitted after their actual materialization, as in the upstream completed-install path. Source notices remain with the packages. Installed/build storage is approximately **3.40 GB**, within cap; the verification receipt records the measurement.

Espressif supplies GCC/framework vendor artifacts; PlatformIO supplies its official PyPI bootstrap; pioarduino is the distinct community-project publisher of the reference integration, installer, fork and descriptors. Helper projects retain their own PyPI licenses; the receipt does not call them all Espressif products.

## Scope and rebuild

Everything is under ignored `.toolchain/` and `.pio/`: bootstrap venv, platform build venv, packages, caches, temp, source archives and outputs. The framework builder also directs IDF tools and its uv cache to the project Core directory. Pip/uv use verified local wheel files with no indexes/offline resolution; an explicit child environment excludes all ambient overrides. Core update/prune checks are disabled locally. Fork Core has no telemetry setting and its inspected source has no bootstrap telemetry sender.

The build venv deliberately retains both `platformio6.1.19` and `pioarduino-core6.1.19` distribution metadata: upstream checks the former package name, while the latter supplies the actual shared `platformio` module tree. A read-only independent review compared every module file with the locked fork wheel; all match. Loaded `platformio.__title__` is `pioarduino core`, version6.1.19. Both graphs pass `pip check`.

```sh
python3 tools/check.py
python3 tools/firmware-build.py
python3 tools/firmware-build.py --clean
```

The wrapper accepts no upload target. It verifies the fixed Arduino/C3 platform/configuration, local paths, package versions/registration and SDK symlink dependencies before invoking PIO. A config change introducing debug tools, a remote dependency or scripts stops before PIO. Notably this pinned platform treats even `build_type=release` or an explicit `upload_protocol` as a debugger request, so both are absent and rejected. Mutation tests guard that concrete failure class, executable flag expressions, variable expansion and ambient overrides. SCons site scripts are disabled.

These are reproducible **build commands with the installed approved toolchain**, not a turnkey fresh-clone installer. Acquisition/materialization was deliberately staged and audited through task-local scripts before execution; those scripts are not a public bootstrap feature. A new machine must recreate the exact receipt's official inputs, verify hashes, materialize the C3-only package shape, register it using Core APIs and preseed the exact Python graph. Agent setup owns that plumbing. Do not fall back to a global installer or a differently versioned SDK.

## Sharing one toolchain among checkouts

Local checkouts can share one installed toolchain instead of each holding a
4 GB copy. `python3 tools/toolchain-share.py migrate --store DIR` stores this
checkout's `.toolchain/platformio` and `.toolchain/llvm` once in DIR. Each
entry is named by the SHA-256 of a manifest of every path, link target,
executable bit, size and content hash, and every stored file and directory is
read-only. The tool then replaces the checkout's copies with views: real
project-local directories of hardlinks to the stored files. Every path and
byte that tools, receipts and resource identity read is unchanged, so no
identity, ownership or toolchain-location rule changes. The identity hash
covers the same files and yields the same values.

The pinned platform rewrites its MCU build script in place (with identical
bytes). It also keeps `.cache`, `appstate.json` and `penv/.lock` beside its
packages. Each view holds private writable copies of exactly those. Every
other view directory and every shared file is read-only, which stops the
platform's ordinary writes. In a view, `idf_component.yml` is read-only, and
the platform ignores its failed rewrite. Modes do not stop the owning user,
who can make a file writable through any view and so change the store. A
health build therefore checks every recorded view before it captures its
identity. Each shared file must still be the stored inode, not modified since
its entry was stored, with the exact read-only mode (`0555` or `0444`) that
its recorded executable bit gives it; `verify` checks the same modes in the
store. Each read-only directory must still be read-only, and each private
rewritten copy must still hold the stored bytes and executable bit.
The check reads file metadata only, apart from those copies, and costs well
under a second. Only `verify` sees a write whose mode and modification time
were both put back. Views need a non-root user: root ignores read-only modes,
so a root build's rewrite of `idf_component.yml` would reach the store, and
every checkout's next health build would refuse the view. PlatformIO runs still take turns at the checkout's
toolchain lock (`tools/native_toolchain.py`), because the build script's
restoration truncates and rewrites it. The previous copies, or
symlinks, move aside to `.toolchain/<component>.before-share-<time>`. The tool
never deletes anything; remove them yourself once satisfied.

`python3 tools/toolchain-share.py verify` also rehashes every stored byte.
The store must be on the checkouts' filesystem. Another checkout migrates
with the same command and store: a matching entry is verified and reused.
Running `migrate` or `store` again in a migrated checkout only verifies its
views; neither stores a view's bytes as a new entry, or in another store.
Build a toolchain once before storing it: its first build writes backups into
directories that a view keeps read-only. A component that is a symlink into
another checkout is read through that link without the other checkout's lock.
Once that checkout is migrated, `link` this one to the same digest instead.
OpenSSL stays per checkout, because its archive must remain a single-link
file. Stored links are kept verbatim, and `store` lists those that leave the
component. One is the platform environment's absolute interpreter link, so a
store depends on that bootstrap interpreter just as a copy does.

## Evidence and physical boundary

`artifacts/firmware-build.log`, `firmware-image-info.txt` and `verification.json` record the clean real build, valid417,072 B C3/DIO/16MB image, ELF/map hashes and both RAM summaries. The upstream precompiled app-description embeds `arduino-lib-builder` and its own version/date; use the Newt source and artifact receipt for this build's identity. The platform's generated partition table is not the X3's observed table.

No flash or device reads were performed. Hardware session permission and verified recovery/duplicate private backups remain prerequisites in `BRINGUP.md`. Image compilation proves neither correct panel behavior nor runtime heap/power/bootloader compatibility.
