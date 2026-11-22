# Newt Rust workspace

Newt's firmware, shared core, screens and Salamander device layer live in this
Cargo workspace (`rust/Cargo.toml`). The canonical crate map is in
[AGENTS.md](../AGENTS.md#start-here); device-layer crates live under
[salamander/](salamander/README.md). Resource ownership invariants are in
[OWNERSHIP.md](OWNERSHIP.md).

## Build

Rust **1.93.1**, its Linux host tools and the `riscv32imc-unknown-none-elf`
target are pinned by official URLs and SHA-256 in [toolchain.json](toolchain.json).
All Cargo dependencies are local workspace paths; checks never download or
install dependencies. Run `python3 tools/rust-toolchain.py --verify` to verify
an existing installation.

The native SDK is the pinned `vendor/freeink-sdk` submodule at
`cd6f5b831c8fad899723b6d406586f00c575f021`. Native code supplies only SDK and
hardware ports, checked by `python3 rust-health/rust_health.py`.

Build the X3 firmware image and check resource ceilings:

```sh
python3 tools/firmware-build.py --clean --health-report
```

See [docs/RUST_APP_BUILD.md](../docs/RUST_APP_BUILD.md) for the underlying
Rust/PlatformIO build and linked ELF guards (`verify-nvs-guard.py`,
`verify-wifi-link.py`, `verify-next-boot.py`).

## Tests

Run the repository host gate from the checkout root (`--profile linux` on
Linux, default on macOS):

```sh
python3 tools/check.py --profile linux
```

For fast edit feedback, run `--suite quick` or `--suite ui`
([AGENTS.md](../AGENTS.md#check-your-work)).

The complete host gate runs `rust-health/rust_health.py`, the bounded landing
check (`rust/check.py --landing`), `rust/tests/landing_test.py` (contract and
source-privacy checks), the Rust test families, and `rust/check.py --target-probes`.
`rust/check.py --landing` runs:

- all portable workspace unit tests;
- the 60,368 foundation comparisons, including independent record, calendar,
  corruption, restart and publication assertions;
- 55 forwarding cases through the native boundary and retained witnesses;
- a full-feature RV32 application archive build.

Its `.build/rust/landing.json` receipt records `complete_rust_gate: false`
because an archive build is not a firmware link or hardware check. The
standalone Rust runner also supports:

```sh
python3 -B rust/check.py --landing
python3 -B rust/check.py --tier full
python3 -B rust/check.py --tier fast --control-family xml_session
python3 -B rust/check.py --positive-only
```

`--tier full` runs all positive cases and mutation controls. `--tier fast`
runs every positive case plus the selected control family (or `all`/`none`).
`--positive-only` is for edit feedback. Neither `--tier fast` nor
`--positive-only` is a complete Rust gate.

Tests read checked-in expected outputs. `expected.py` binds each fixture to
its complete ordered input corpus and rejects missing or mismatched data;
checks never regenerate fixtures.

All checks use frozen/offline Cargo with two jobs, project-local tools and
`.build/rust` outputs. Each run uses its own temporary folder under
`.build/rust` and removes it when done; receipts, landing logs and reused
Cargo targets stay. Sources are copied to a checked snapshot, and input and
tool identities are verified before publishing a receipt. Do not run two gates
at once.

## Evidence limits

Host tests, the host preview, target archive or firmware compiles, and checks
on a real X3 are separate facts. Target archives prove compilation, not linked
image size or runtime fit; clean firmware builds, linked guards and bench runs
provide separate evidence. Cold power-on and unplugged battery sleep/wake stay
separate from USB-powered checks.

Receipts and logs remain local under `.build`; do not commit observation
archives or proof bundles. Keep source provenance and licences in
[docs/provenance.md](../docs/provenance.md), [docs/NOTICES.txt](../docs/NOTICES.txt)
and [THIRD_PARTY.md](THIRD_PARTY.md).
