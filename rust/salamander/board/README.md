# X3 Board boundary (`newt-c3-board`)

Part of [Salamander](../README.md). `newt-c3-board` is the `no_std` device
boundary over the X3's display, input handoff, SD card mount, DS3231 clock,
DEV USB transport and retained RTC regions.

`Board::acquire_x3` requires exclusive live native display and input objects,
the shared SPI/card rail, initialized `Wire`, and the driver resources listed
in `native/board.h`. It runs fixed display/input startup and the SD mount
sequence; teardown after a partial acquisition remains the caller's obligation.

**Display startup order**, on cold boot and deep-sleep reset alike: select X3
geometry (`nb_select_x3`), release the shared SD rail, probe the panel, then
begin. Selecting X3 must come before the probe: the pinned SDK's probe
promotes only the display controller, while `setDisplayX3` resets the base
profile unless the board was set to the UC8279 sibling. The board is X3-only
on purpose: no I2C fingerprint (clock, battery) may pick another geometry, and
the panel's VER probe alone picks the controller. Keep that when adding a
boot-time sensor.

`acquire_x3_optional_sd` keeps Home and input usable when the SD mount returns
`Io`; other acquisition errors remain fatal. It exposes no filesystem view
until `ensure_sd_mounted` succeeds. Failed mounts can retry; a successful mount
is cached so later calls never reinitialize SPI beneath live descriptors. The
`sd_mounted` flag records VFS registration, not card presence or durability.

Acquisition also returns the sole `InitializedInput`, which splits into
`TaskPorts`, `UiQueue` and `UiControl`. Reader scopes lend panel RAM and
scoped native-I/O ports. Inside a scope, `ReaderPanel` alone offers blocking
refresh and the clock snapshot, only after every parse/frame loan has returned.
The `Board`-minted `MonotonicClock` reads the SDK-global timer without holding
a `Board` loan. Ports and their authority are `!Send`/`!Sync`; every
display/SD call must finish synchronously before the next call.

`clock_registers` and `clock_snapshot` perform one read-only DS3231 bus
transaction (`native/clock_transaction.h`), without writing chip registers or
acknowledging OSF. `rust/firmware/board_clock.rs` adapts them to Home's clock
source so neither this crate nor `newt-device-ui` depends on the other.

Wake arming, rail shutdown and sleep require exclusive input/GPIO access proven
by scoped quiescence. Keep the input task suspended and its owners live through
committed sleep or a rejection restart; an abandoned pre-commit attempt resumes
before repaint.

MIT. FreeInk and Newt notices remain in `FreeInk-MIT.txt` and the repository
`LICENSE`; see [docs/provenance.md](../../../docs/provenance.md).
