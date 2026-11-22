# Guard shared NVS before starting the reader

October 1, 2026. Arduino startup can format NVS on initialization errors; pinned IDF can also erase pages or invalidate entries during repair. This slice protects shared storage before Newt's `setup()` runs, preserving the FreeInk/toolchain pins and prior archives.

Project-owned linker wrappers surround `nvs_flash_init` and lower partition erase/write/raw-write entrypoints. Initialization may read but cannot mutate NVS. A failed initialization, or rejected housekeeping even if IDF reports success, leaves storage unavailable. Namespace opens then fail before calling NVS. Whole-partition formatting and explicit `nvs_flash_erase*` calls are always refused. Non-NVS calls retain their native argument/error behavior; no updater or other write operation is added.

Home, embedded short story, device information and sleep remain available. The device screen explains unavailable storage; the EPUB screen explains why reading storage cannot open and that Back returns Home. Runtime progress-save failure marks the footer `UNSAVED` and adds a device-info warning. Preferences retain explicit Save/error/draft behavior. No formatting or fresh record replaces unreadable state.

After mutation-free successful initialization, normal NVS writes/page GC remain available for scoped `newt_reader`/`newt_prefs` records. Shared physical pages can change while IDF preserves unrelated logical entries. This does not guarantee runtime bytes or power-loss/corruption safety. Startup states requiring IDF repair intentionally become unavailable. Native fault injection and interrupted-write durability remain untested.

## Evidence and exact candidate

The full host gate passes strict warnings, ASan/UBSan, existing reader/package/store/UI checks and 12 isolated guard lifecycles: full/incompatible storage, erase/write/raw-write repairs, ignored failures, exact Arduino whole-range fallback/no retry, healthy runtime maintenance, error propagation, gated namespace opens and visible failure rendering.

Clean C3 compile/link passes. Mandatory post-build disassembly proves Arduino startup calls wrapped initializer/erase, NVS partition methods reach wrapped mutations, direct real mutations are guarded passthroughs, and whole-NVS formatting implementations are absent. Independent bounded source/link review reports no actionable findings.

- Image: **658,720 bytes**, SHA-256 `fd06a29aee2c2bbc63c31dc69553ca3e17bc7fff074f1f66aafc7c498274b582`; C3 DIO/80 MHz/16 MB.
- Static RAM 71,396 bytes; linked DRAM 131,488 bytes. The reader's 69,968-byte contiguous workspace and Expat transient allocations need native measurements.
- Scope remains **161 sectors / 659,456 bytes**, `[0x780000, 0x821000)`, at observed active app1. Payload end `0x820D20`, erased padding 736 bytes. Original/baseline restoration slices are unchanged and live-validated. No low-flash/OTA/table/other-app writes.

Source/image identity, archived ELF/map, logs and preservation hashes are in `artifacts/nvs-reader-verification.json`; actual linked edges are in `artifacts/nvs-guard-link-verification.json`. Software evidence and executed hardware receipts remain separate.

```sh
python3 tools/check.py
python3 tools/firmware-build.py --clean
python3 tools/verify-nvs-guard.py
```

Use the final archive, not current `.pio` output or the historical unguarded typography image. The owner conditionally approved this bounded upgrade after safety/build/review gates, provided risk/sector scope stayed unchanged. The private proposal records exact arguments and restoration hashes. No restoration, SD/network install or bootloader/partition/OTA changes are authorized by this record.

## Historical execution blocker, subsequently resolved

The final pre-write ROM identity, first 64 KiB and exact 161-sector app range matched the private baseline backups and restoration slice. Automatic approval review rejected attempts because forwarded/assistant-reported approval was insufficient for this hardware write. None of those rejected processes started. No alternate execution path was attempted; the device was returned to the baseline.

The owner subsequently supplied direct approval in this execution conversation: “That was me approving it btw. DO IT”. Fresh identity/security/low-flash/app checks passed. The exact archived image was then written successfully, fully read back and normally booted. [The separate executed hardware receipt](010-guarded-reader-hardware.md) supersedes the historical blocked hardware status in `artifacts/nvs-reader-verification.json`; it does not change the source/build evidence or imply physical reader acceptance.
