# 088: Installing updates over Wi-Fi from GitHub (#103 phase 4)

**Status:** implemented (#173, flash record 79). The TLS frame loan ("Memory")
adds flash record 80 (reviewer pending). Resident RAM is unchanged. API Checks
and a release-asset download ran on the bench X3 ("Memory"); an install from a
published release is still pending. Design: [082](082-ota-updates.md) (its owner
decision 3 and "design" item 8 now name GitHub Releases). Builds on
[083](083-ota-sd-install.md) (the card installer), [084](084-wifi-memory.md)
and [085](085-wifi-setup.md) (Wi-Fi, the saved network and its join).

## Flow

Settings → **Update** opens update mode on its list. Below the card's offers
is one more row, **Check GitHub for updates**. Select says **Check** with a
saved network and a card, on USB or battery ([082](082-ota-updates.md), rule 8,
2026-10-31); otherwise the note under the list says what is missing.

1. **Update** shows "Connecting to Wi-Fi…", then "Looking for an update on
   GitHub…". Every button waits, and presses made meanwhile are dropped when
   the check ends: only a fresh Select installs what it found.
2. A newer Newt downloads to the card: "Downloading…" with a bar and "Do not
   turn off the X3 or remove the card." The bar grows at each fifth; only the
   bar changes, because TLS holds the page's frame meanwhile (see Memory).
   If a card write fails, the download starts again from its first byte, on
   an empty bar, up to three tries in all (see Card writes).
3. When the download is whole and its SHA-256 matches the release's manifest,
   Wi-Fi is off again and the list shows the new **Newt** offer, focused, with
   **Install**. From there it is a card offer: the same confirm rules,
   progress and result as 083.
4. Otherwise one result shows; Back returns to the list on Check, Sleep leaves
   update mode.

Before the result shows, with Wi-Fi off, the X3 clock is set from the first
GitHub response's Date, if it needs it
([066](066-read-only-home-clock.md#setting-the-clock-261), #261).

| Result | Text |
|---|---|
| Not newer | No newer Newt on GitHub. This X3 has *version*. |
| No saved network | No Wi-Fi network is saved. Save one in Settings, Wi-Fi. |
| Cannot join | Newt could not connect to the saved Wi-Fi network. |
| Cannot reach GitHub | Newt could not reach GitHub. Try again later. |
| No release | GitHub has no Newt update to download. |
| Wrong file | The update on GitHub is not a valid Newt file. Nothing was installed. |
| Interrupted | The download did not finish. Nothing was installed. |
| Card failed | Newt could not save the update to the card. Nothing was installed. |

List notes on the Check row: "No card found. Insert the card, then open Update
again.", "Downloads a newer Newt from GitHub to the card with the saved Wi-Fi
network.", then "Or copy newt.bin and newt.manifest to /Newt/Update/. Make the
folder if it isn't there." when the card offers nothing. With a card but no
saved network the row reads "Set up Wi-Fi" with "Save a Wi-Fi network to check
GitHub for updates.", and Select opens Wi-Fi networks, as Settings, Wi-Fi does
(#365).

**Power.** Check, the download and the install need no USB since 2026-10-31
(owner decision, [082](082-ota-updates.md) rule 8): the install is power-loss
safe, so there is nothing to gate: a battery that dies mid-install loses
only that attempt. Battery current, gauge accuracy
and brownout at low charge are not measured (a known gap, not a gate). A
Check and an install on battery are a separate hands-on check, still
pending.

## One installer input

The network source never writes a slot. It downloads the release's
`newt.manifest` and `newt.bin` to `/Newt/Update/`, the pair a person copies
there, and the unchanged card installer (`newt::SlotInstaller`) checks and
installs it. This differs from a literal "stream newt.bin into the slot
writer", on purpose:

- **The fence.** While Wi-Fi may run, the NVS guard refuses every partition
  write and erase (084 "Guards"; #157). Writing a slot during the download
  would mean lowering that fence with the radio on.
- **082's design** item 2: "Its input is always a file on the card: you copy
  it, or Newt downloads it."
- **Interruption is harmless.** A download that stops leaves both slots as
  they were; only the card's Newt pair is gone.
- **The installer does not know the source.** It checks size, SHA-256, image
  structure, chip, Newt mark and version again from the card, as for any copy.

Card order (`firmware/update_card.h` `CardDownload`): the old
`newt.manifest` is removed first, `newt.bin` is written, synced and closed,
then `newt.manifest` is written and synced last. A manifest therefore names
only a whole, hash-checked image. A stopped download removes `newt.bin`.
CrossInk's files are never touched.

## Source

- `GET https://api.github.com/repos/<repo>/releases/latest` with
  `Accept: application/vnd.github+json` and `X-GitHub-Api-Version: 2022-11-28`.
  `<repo>` is one constant, `kUpdateRepository` in
  `include/newt/update_online.h`: `VargasDevelopment/newt-releases`, a public
  releases-only repository. It has no releases yet, so a check today shows
  "GitHub has no Newt update to download." No token is needed or used.
  `/releases/latest` returns the newest published release that is neither a
  draft nor flagged pre-release. Newt's versions are SemVer pre-releases
  (`0.1.0-alpha.N`), so a published Newt release is never flagged
  pre-release on GitHub; one that was would be skipped.
- The release JSON is read as a stream, kept in no buffer
  (`newt::ReleaseJson`): only the top-level `assets` array's `name`,
  `browser_download_url` and `size` are read. Every other value, the notes
  included, is checked as JSON and skipped. Nesting deeper than 16 fails. Kept
  strings take plain ASCII only (`\/`, `\"`, `\\` and printable `\u` escapes
  are their characters); anything else makes that value unusable, never cut.
- The assets must be named exactly `newt.manifest` and `newt.bin`. A name
  that appears twice is refused, not picked from.
- The manifest (≤512 B, read whole) must parse, be kind `newt`, have a size
  within the slot and equal to the release's size for `newt.bin` (when
  given), and be newer than the running version, else nothing more downloads.
- `newt.bin` must answer 200, its Content-Length (when sent) must equal the
  manifest's size, and the body must be exactly that size with that SHA-256.
- Redirects (301, 302, 303, 307, 308) are followed by Newt, at most 5 per
  file, each to an `https://` URL that fits 2,048 bytes. GitHub sends asset
  downloads to a signed `release-assets.githubusercontent.com` URL.
- 404 on the release is "No release"; any other status, no connection, a TLS
  failure or a timeout is "Cannot reach GitHub".

## Bounds

| What | Bound |
|---|---|
| Socket step (connect, TLS, each wait) | 10 s (`kHttpsTimeoutMs`), less when the file's limit is nearer |
| Release JSON | 512 KiB, 30 s |
| Manifest | 512 B, 15 s |
| Image | the manifest's size (≤ the usable slot), 300 s |
| Headers per response | 16 KiB counted; more fails the request |
| URL, redirect Location | 2,048 B; longer fails, never cut |
| Asset URL in the JSON | 512 B |
| Redirects | 5 per file |
| Attempts | one per Select; the image up to 3 tries when a card write fails, no other retry |

Each file's time limit starts before its first request and covers its
redirects, connects, headers and body together (`Http::limit`). A socket
timeout alone would not bound a file: `esp_http_client` waits again after each
byte a server trickles, in TLS, headers and every read. So `EspHttps` runs a
timer once a second; past the limit it shuts every lwIP socket (update mode's
only ones are the client's; lwIP allows it from another task), which wakes
the blocked call, and the client refuses every later open and read. A DNS
lookup or a TCP connect cannot be shut: a connect waits at most what was
left of the limit when its request began, and lwIP ends a lookup after its
retries (7 s for each of up to 3 DNS servers). So a file ends at most about a
second, plus a lookup under way, past its limit; with answering DNS, about
a second. Without the timer no request starts. The results are the same as
any timeout: "Cannot reach GitHub", or "The download did not finish".

A body ends only when the client saw all of it (Content-Length reached or
the last chunk read). A short read at a timeout or a closed connection is a
failure. A response with neither length nor chunks is refused; GitHub sends
one or the other.

Limit: ESP-IDF's HTTP parser stores a header's value before Newt's handler
sees it, so one oversized header costs heap up to the parser's own limit
before the count refuses it. Only a server that passed the certificate check
can send one.

## HTTPS

`firmware/update_https.h` (`EspHttps`) uses ESP-IDF's `esp_http_client` over
esp-tls and mbedTLS, as the pinned framework builds them (TLS 1.2, 16 KB
record buffers each way, no dynamic buffers). Every request attaches the
certificate bundle (`crt_bundle_attach = esp_crt_bundle_attach`); there is no
path without it, and no option skips the hostname check. The framework's
mbedTLS has no date checks (`CONFIG_MBEDTLS_HAVE_TIME_DATE` off): the X3 has no
trusted clock, so expired certificates are not refused.

**The bundle.** The pinned framework enables ESP-IDF's certificate bundle
(full default list, up to 200 roots) but ships no bundle data. 
`tools/crt_bundle.py` writes it during `prepare_native_lane` from Mozilla's CA
list for TLS servers as the pinned toolchain's certifi 2026.7.22 carries it
(MPL-2.0; the file's SHA-256 is pinned and any other copy is refused), in
ESP-IDF 5.5's layout (read from the linked `esp_crt_bundle.c.obj`): a
little-endian u32 offset per root, then each root's subject Name and
SubjectPublicKeyInfo DER, sorted by subject. 121 roots, 56,069 bytes.
`update_https.h` embeds it with `.incbin` as `_binary_x509_crt_bundle_start`
and `_end`. The full list keeps working when GitHub changes CAs, and Airspace
(#166) can share it.

## Card writes (#221)

The card is mounted by Arduino-ESP32's SD library, whose SPI driver
(`libraries/SD/src/sd_diskio.cpp` in the pinned core) fails a sector write
when the card stays busy past a fixed 500 ms (selecting the card, before each
data block and after a multi-block write), when the card does not accept a
block (its data response is not "accepted": a CRC or write error), or when its
status after the write is not clean. It does not retry these: its CRC-retry
branches compare the masked response with values it can never take. FatFs
turns that into `FR_DISK_ERR`, ESP-IDF's VFS into `EIO`, and FatFs marks the
open file failed, so every later write to it fails too. Only a new file can
go on, so a failed image write closes and removes newt.bin and downloads the
image again from its first byte, up to `kImageTries` (3) tries. Three cover
the worst run an owner has seen (two failed first tries, then a whole one)
for at most two more downloads. Every other ending is final, and each try
checks its bytes against the manifest's SHA-256 as before. Newt carries no
patch for the core, so the 500 ms wait stays.

## Wiring

- `checkUpdateOnline` (firmware/update_mode_runtime.h) takes the saved
  network's password as Test does (085), and joins once
  (`joinWifiNetwork`). Only with an address does the join call
  `fetchUpdateOnline`, which allocates the check's work (8,860 B) and runs
  `newt::fetchOnlineUpdate`, then returns before Wi-Fi stops.
- Wi-Fi stops and deinitializes as for every join. If it may still run, update
  mode restarts into Newt before anything else (its one exit wipes the
  password). Only then is the card's Newt offer read again.
- The flash fence stays up for the whole download: nothing writes flash while
  Wi-Fi runs. The card is SD, not flash.
- DEV rows (no URL, name or address): `Newt update online: result R, join J,
  reason N, error E, join-ms, stop-ms, fenced-opens, fenced-writes`, and heap
  rows `online-before`, `online-joined`, `online-fetched`, `online-stopped`
  ("heap free, largest block, lowest").
- DEV builds only: the Card failed page adds one line, `Card step: S (error
  E) at B; tries T`, so a photo of the page names the cause (#221). S is the
  first card step that failed: `make folder`, `remove manifest`, `open
  image`, `write image`, `save image` (its sync), `close image`, `open
  manifest`, `write manifest`, `save manifest` or `close manifest`. E is its
  errno, 0 when a write took no bytes. B is the bytes that try had written
  to the card by then (newt.bin's, then the manifest's), and T the image
  downloads begun. Builds without DEV keep no record and show the page
  unchanged. The row `Newt update mode online-card: tries T, slowest-write-ms
  W, step S, error E, at B` follows every download, also one that recovered:
  W is the slowest image write, S the step's number (0: none failed).
- `tools/verify-wifi-link.py`: `joinWifiNetwork`'s callers are Test and Check;
  `fetchUpdateOnline` is a library entry whose only caller is Check, so the
  HTTP, TLS and certificate code it alone reaches counts as Wi-Fi code;
  Check is reached from `runUpdateMode` alone. TLS therefore runs only inside
  update mode's join, while the network holds an address.
- `tools/wifi-sections.py` (084's lent range) gains three things:
  - `WIFI_FILES` adds `esp_http_client`, esp-tls, both mbedTLS archives and
    mbedcrypto's `esp_ds_common.c.obj`, so their `.data`/`.bss` sit in Wi-Fi's
    lent range. Every boot still reloads the range; a reading boot lends it to
    the heap.
  - `WIFI_SECTIONS` places two named locks of a shared file,
    `esp_crypto_lock.c.obj`'s `s_crypto_ds_lock` and `s_crypto_hmac_lock`
    (used only by the DS peripheral code), in the range. Its other locks stay
    resident.
  - `WIFI_CONSTRUCTORS` takes `esp_ds_common.c.obj`'s constructor out of
    boot's list into `_newt_wifi_init_start`/`_end`. It creates a heap mutex
    that only a TLS handshake uses. `runWifiConstructors()`
    (firmware/wifi_memory.h) runs it once as update mode starts; the link
    check accepts it only from `runUpdateMode`.
  - `FLASH_FUNCTIONS` moves `heap_caps_malloc_prefer` (84 B) from IRAM to
    flash. Its one caller is `esp_ds_rsa_sign`, flash code reached from
    esp-tls. The link check's IRAM rule fails the build if IRAM code ever
    calls it.

## Memory

Resident RAM is unchanged (see "Measured"), so reading pays nothing: the
TLS and HTTP state lives in the lent range, which a reading boot gives to the
heap, and no constructor of theirs runs outside update mode.

**The frame loan.** Wi-Fi's join leaves too little heap for a TLS session
beside it (the first bench Check on #173's build could not reach GitHub).
Update mode draws nothing while a session is open, so Check lends the idle
52,272 B logical frame to mbedTLS (`firmware/update_mode_runtime.h`):

- Before its first session, Check makes the frame a standalone `multi_heap`
  and installs a calloc/free pair with `mbedtls_platform_set_calloc_free`.
  Calls from Check's own task are served from the frame first and fall back to
  mbedTLS's default heap pair; calls from any other task (Wi-Fi's supplicant
  also uses mbedTLS) always use the default. Free routes by address.
- After the last session closes, Check restores the default pair, but only if
  the frame heap holds no block. Then it clears the frame and may draw again.
- If a block is still held, the frame is never drawn: Wi-Fi stops, the saved
  password is wiped from RAM, and Newt restarts into update mode's list. The
  check ends there, with no result page.
- While the frame is lent, the only page change is the download bar, filled
  straight into the panel's own buffer (`fillNativeRect`) every 20 %. The
  "Downloading…" page is drawn once per try, before the image's session
  opens.
- No Wi-Fi or lwIP option changes. Nothing new is resident.

**Measured on the bench X3** (USB power, the saved network, DEV rows):

| Check | Build | Normal heap lowest | Frame lowest | Failed allocations |
|---|---|---|---|---|
| API Check ×7 (`NoRelease`, 2 with presses during it) | `009dbcaa` | 39,140–40,060 B | 3,300 B (1 session) | 0 |
| API Check + 3,301,376 B download in 14.4 s | `009dbcaa` + bench-only probe | 25,908 B | 344 B (3 sessions) | 0 |

- Update mode starts with 154,856 B free. The join leaves about 66,600 B free.
- The download used a public release asset on GitHub's release-asset host
  (esptool's, via the same redirect path an image takes), written to a scratch
  card file and deleted. The probe that requested it is bench-only, never
  merged. During it the frame came within 344 B of full, and the rest of
  TLS's need fell back to the normal heap: about 36,000 B free, largest block
  25,588 B, steady from the first 512 KB to the last.
- The frame heap was empty at every close (`kept 0`).
- A reading boot after the checks: 146,636 B free, largest block 114,676 B,
  the same as main `a7c63ca6`.
- Two earlier probe runs on the same loan did not finish cleanly, and neither
  has repeated. In one, the card write came back short after 1,095,680 B; the
  network and heap were fine. In the other, that probe polled DEV USB only
  every 512 KB, and the DEV protocol timed out after the result. The passing
  run polls on every chunk, as the real download does.

If memory still runs out:

- **The TLS session cannot open.** `esp_http_client` fails before any status,
  so the check shows "Newt could not reach GitHub. Try again later." Nothing
  is written to the card.
- **Memory runs out during the download.** The read fails, the check shows
  "The download did not finish. Nothing was installed.", and `newt.bin` is
  removed. The card's older Newt pair was already removed; CrossInk's stays.
- **Either way**, Wi-Fi stops, the slots are unchanged, and Back returns to
  the list. Reading is not affected: update mode is its own boot.
- **The DEV rows tell which.** `online-joined`, `online-fetched` and
  `online-frame` show how close the check came, and update mode prints a row
  for each failed heap allocation.
- **Not shown by host tests.** An allocation failure inside the Wi-Fi driver
  or lwIP is not modelled. The bench check covers it.

## Tests

- `tests/update_online_test.cpp` (host): the JSON reader in pieces of 0, 1, 7
  and 4,096 bytes; extra and look-alike asset names; missing assets; 300 KB of
  escaped notes; `assets` below the top level; every wrong type, an array in
  the asset array; escapes; long URLs; repeated names; every truncated prefix
  and malformed input refused; the depth bound. Then the whole check against a
  recorded GitHub (fixtures written by hand from GitHub's documented shape):
  redirects (moved repository, `http://`, empty or too-long Location, over and
  within the bound); release 404 and other statuses; truncated, broken, slow
  and oversized JSON; manifest same, older, CrossInk, garbage, too long,
  broken, or a size unlike the release's; image Content-Length mismatch,
  oversized, short, broken, slow, wrong SHA-256; card begin, write and finish
  failures. A server that trickles a byte a second (release, manifest, image)
  or whose headers never end is cut off at the file's limit, and one limit
  spans a file's redirects; each file, slow but inside its own limit, still
  downloads. The fake client cuts any call at the limit, as the firmware's
  timer does; the timer itself runs only on the X3 and has no host test.
  Each failure leaves no card offer and never calls the installer.
  Each of its 27 checks (JSON rules, redirects, bounds, each file's limit,
  manifest, size, digest, abort) was removed once on a fresh build, and the
  test failed each time; that run is review evidence, not part of the gate.
- `tests/update_card_test.cpp`: a whole download becomes the offer, a stopped
  one leaves no offer and no image, CrossInk is untouched. Update mode's own
  download step on a scratch card that refuses newt.bin past 256 B on its
  first one or two tries leaves the served image, exactly, as the offer; one
  that keeps refusing ends Card failed with no image and no manifest, its
  page naming `write image`, its errno, byte 256 and 3 tries. Controls
  remove the retry and the byte.
- `tests/update_screen_test.cpp`: the Check row's place, what it needs, every
  button waiting while it runs, each result's own page, Back and Sleep, and a
  downloaded Newt installing from the list. The download's bar grown on the
  panel's buffer alone (`fillNativeRect`) equals the page drawn at each fifth,
  pixel for pixel.
- `tests/wifi_setup_test.py`: the actual `checkUpdateOnline`, join and dispatch
  against the IDF model: GitHub is read only while the network holds an
  address and the fence is up; no address requests nothing; on battery
  networks scan, Check runs and a fresh Select installs (the old USB gate put
  back fails it); a teardown that fails restarts before the card is read; the card is
  read again only with Wi-Fi off; a Select pressed while a check finds a newer
  Newt is dropped, so the list shows it focused and nothing installs.
  Controls: the join's callback removed, the restart after a failed teardown
  removed, and the drain after the check removed (the queued Select
  installs). The frame loan: each session's allocations land in the frame,
  a supplicant-task allocation does not, mbedTLS has its default pair back
  after the check, the frame is never drawn while lent, and a session that
  leaves a block in the frame stops the check and restarts into the list
  without a paint. Controls: the lend removed, the task check removed, the
  restore removed, the bar painted through the frame, the empty-heap check
  removed, the paint before the image ignoring a kept frame, and the restart
  for a kept frame removed.
- `tests/update_online_test.cpp` also checks that each try's first progress
  call, done 0, comes once and with no request open, that a card write
  failing on one or two tries still ends with the exact image and its
  manifest, that one failing on every try ends Card failed after the third,
  and that a try ending any other way is final.
- `tests/wifi_link_test.py`: a lock of a shared file other than the two
  named is refused in the range; IRAM calling `heap_caps_malloc_prefer` is
  refused; the moved function's file keeps its other IRAM code; boot running
  the deferred constructor is refused; `runWifiConstructors` is accepted only
  from `runUpdateMode`.
- `tests/crt_bundle_test.py`: the bundle read back as ESP-IDF reads it, every
  root kept, sorted, each part whole; another CA list, a damaged certificate
  and an empty list refused.

## Owner decisions

1. **The source is `VargasDevelopment/newt-releases`** (public, no releases
   yet). Today's check shows "GitHub has no Newt update to download." No token
   or credential is added anywhere.
2. **Releases must be published, not drafts, and never flagged
   pre-release**, with assets named exactly `newt.bin` and `newt.manifest`
   (the card pair). Publishing is the reviewed release step
   (docs/RELEASING.md), not this PR.
3. **Download to the card, then the unchanged card installer** (accepted).
   A download replaces the card's Newt pair (CrossInk's stays). An older
   pair is removed first; a stopped download leaves none, never a partial
   `newt.bin`.
4. **No cancel during the download.** Buttons wait until it ends or times out
   (300 s at most for the image, about a second more; see Bounds). Follow-up
   if wanted.
5. **No date check on certificates** (no trusted clock).

**After an install** the downloaded pair stays in `/Newt/Update/`, as a
copied pair does. Opening Update again offers that same Newt. Install then
asks "This version is already installed. Install it again?" and does nothing
unless confirmed. It never installs itself, and the next Check replaces it.
Removing the pair once the new Newt confirms its first boot would put card
writes on the reading boot's first-paint path, so it is a follow-up, not part
of this change.

## Bench checks for the Lead

1. Heap: done, see "Memory".
2. Against today's empty `newt-releases`: done on `009dbcaa` (result row
   `NoRelease`).
3. Once a newer release is published there: download, Install, restart,
   "Updated to"; then reopen Update and see the same-version offer above.
4. Same version on GitHub: "No newer Newt".
5. Pull the router's uplink mid-download: "The download did not finish";
   card has no `newt.bin`, both slots unchanged.
6. USB unplugged before Check: no join.
7. DEV rows carry no URL or network name.

## Measured

The clean health build of `470a7492` (`regional-dev-usb`) passes the NVS,
Wi-Fi fence, Wi-Fi link, console and next-boot guards and the resource
ratchet with flash record 79 (CODE_HEALTH.md).

| Measure | Main `d4698e9d` | This branch | Change |
|---|---|---|---|
| Flash | 3,025,280 | 3,228,624 | +203,344 (record 79) |
| Flash, frame loan (`8aa1ef86`) | 3,228,624 | 3,229,728 | +1,104 (record 80) |
| Resident RAM (ratchet) | 125,296 | 125,296 | 0 |
| Wi-Fi lent range | 22,528 | 22,528 | 0 |
| Lent range content | 20,672 (at `728f549f`) | 22,448 | +1,776 (80 B left) |
| RTC (`.rtc.*`) | 300 | 300 | 0 |

The lent range is now 80 B from full. The next Wi-Fi feature that adds
static state needs a reviewed raise of the range's ceiling
(`wifi_lent_bytes`).

What moved into the range or out of IRAM (first measured on round 1's build,
where it grew resident RAM by 512 B):

| Symbol | Bytes | From | Now |
|---|---|---|---|
| `supported_ciphersuites` | 488 | mbedTLS SSL | lent range |
| `s_crt_bundle`, `global_cacert`, `supported_init`, `s_timeout_ms` | 16 | certificate bundle, esp-tls, mbedTLS | lent range |
| `s_ds_data`, `s_ds_lock`, `s_crypto_ds_lock`, `s_crypto_hmac_lock`, `s_esp_ds_hmac_key_id` | 20 | DS peripheral, linked by esp-tls | lent range |
| `heap_caps_malloc_prefer` | 84 | heap, called by `esp_ds_rsa_sign` | flash |
| `esp_ds_conn_lock` (constructor) | one heap mutex | DS peripheral | runs in update mode only |
