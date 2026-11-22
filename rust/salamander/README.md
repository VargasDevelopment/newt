# Salamander

Salamander is Newt's device layer: the code that talks to the X3's SDK and
hardware. Newt above it should not know which device it runs on (#292).

| Crate | Package | Role |
|---|---|---|
| `board` | `newt-c3-board` | Board acquisition, panel, SD, clock, DEV USB |
| `input` | `newt-input-policy` | Button decode policy |
| `input-owner` | `newt-native-input-owner` | Native input task owner |
| `native_io` | `newt-native-io` | SD file and directory natives |
| `nvs_guard` | `newt-nvs-guard` | Guarded NVS records |
| `sdk-heap` | `newt-sdk-heap` | Rust allocator over the SDK heap |
| `device` | `salamander-device` | What the device is, as plain data (no natives) |

`device` holds the panel's geometry. `newt_core::frame` derives Newt's portrait
frame from it, so every screen reads its size from one device-owned source.

## Natives stay here

`rust-health` fails any `extern` ABI, `#[link]`, `no_mangle` or `export_name`
outside the native owners (`baseline.json` `native_owners`). Files that declare
natives today are counted in `native_exceptions`; a count may only shrink.

## Not yet

Step 1 only grouped and named the layer. Still open:

- move the firmware crate's natives (Wi-Fi, TLS, update, fonts, NVS leaf,
  power) behind Salamander and shrink `native_exceptions`;
- a host adapter for the preview fakes;
- real device interfaces, once a second device exists.
