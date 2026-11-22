# 085: Wi-Fi setup (#103 phase 3, slices A and B)

**Status:** slices A and B are on main (PRs #161 and #162). The owner's
hands-on check on the bench X3 passed with a real network on USB and on
battery ("Bench, owner hands-on"). Settings' Wi-Fi row: PR #164. Since
2026-10-31 Wi-Fi needs no USB ([082](082-ota-updates.md), rule 8); a
hands-on check of a scan, Save and Test on battery is still pending.
Design: [082](082-ota-updates.md) section 4. Wi-Fi itself:
[084](084-wifi-memory.md).

Slice A lets the owner pick a network and type its password on the X3, and
keeps it, split between the card and NVS. Slice B tests the saved network:
it joins once and turns Wi-Fi off again.

## Flow

Settings → **Wi-Fi** works like Settings → **Update**: it runs the reader's
exit save (an unconfirmed save keeps Newt open and says so on the row), then
restarts into update mode, which opens on **Wi-Fi networks**. Update lists
only updates. Wi-Fi still runs in update mode only (a reading boot never sees
it). Back on **Wi-Fi networks** leaves update mode; the restart lands on Home.

1. **Wi-Fi networks** lists what one scan heard, below **Scan** (**Scan
   again** after a scan). The page scans as it opens, on USB or battery. Until
   2026-10-31 Wi-Fi needed USB to a computer; the owner removed that rule
   ([082](082-ota-updates.md), rule 8). When a network is saved, two last rows **Test <name>** and **Forget <name>**
   show its name. Select says Scan, Pick, Test or Forget.
2. **Pick** an open network: it saves at once, with no password.
3. **Pick** a secured network: **Wi-Fi password** shows the name, a masked
   field (one `*` for each byte typed), "N characters. At least 8." and
   FreeInk's QWERTY keyboard (`libs/ui/FreeInkUI` `components/keyboard`). The
   side buttons move between keyboard rows; the front buttons move along a row.
   Shift stays on until pressed again; `?123` switches to symbols. **Save**
   works from 8 characters.
4. **Wi-Fi** shows "Saving..." or "Forgetting...", then one of:
   "Network saved." / "Network forgotten.", "Not saved. Nothing was changed.",
   or "Not confirmed. After restart the X3 may use the old or the new setting."
   Back returns to the list.

Back on the password page wipes the password. Every restart out of update
mode (Back, Sleep, ten idle minutes, a finished install, a scan that may leave
Wi-Fi on) goes through one exit, `leaveUpdateMode`, which wipes the password
and the scan's entropy pool first: `esp_restart()` runs no destructor, and RAM
survives it.

A panic skips that exit. Three facts keep its leftover secrets out of the
next boot's output; `tools/verify-next-boot.py` checks each after every
firmware build (`tests/next_boot_test.py` has a failing control for each):

- **Reloaded range.** The app image's load segments cover all of
  `.dram0.newt_wifi`, so every boot rewrites it before a reading boot lends
  it to the heap.
- **Filled stacks.** FreeRTOS fills each new task stack with `0xa5`
  (`prvInitialiseNewTask` calls `memset` before it lays out the stack).
- **No DRAM in core dumps.** The pinned sdkconfig keeps
  `CONFIG_ESP_COREDUMP_CAPTURE_DRAM` disabled: a dump holds task stacks only.

Slice B adds a fourth: every boot clears the heap's RAM before it registers
it ("Slice B, as built").

Leaving the password page refreshes the panel fully. DEV FRAME refuses the
networks list and the password page. DEV lines carry counts and results only.

## Storage, as built

- **Card record** `newt_net` (`/Newt/State/newt-net.nrs`, the SD state
  backend's A/B slots): `NWTN` v1, 146 bytes, the durable record store's
  generation and checksum. Kind 0 is an explicit "no network" (all zeros
  after it). Kind 1 holds a 12-byte nonce, the AES-256-GCM ciphertext of a
  fixed 98-byte plaintext (name length, name, password length, password, zero
  padding, so the length never shows) and the 16-byte tag. The header (magic,
  version, kind) is authenticated.
- **Key** NVS namespace `newt_wifi`, blob `key`, 32 bytes, through the NVS guard.
  docs/080's Newt budget: 4 slots, 165 in all.
- **Random bytes.** Each scan mixes 32 bytes from `esp_fill_random` into a
  RAM pool while the radio runs (the C3's RNG is a true source then). A key or
  nonce is SHA-256 of the pool, a counter and one more `esp_random` word. With
  no scan this boot, nothing is sealed.
- **Lent memory.** mbedTLS's allocator hooks and AES DMA state sit in Wi-Fi's
  range, which every reading boot lends to the heap. `tools/verify-wifi-link.py`
  allows the cipher's `seal` and `open` only from update-mode code
  ([084](084-wifi-memory.md) "Guards"). No memory moved.
- Code: `firmware/wifi_network_store.h` (rules, host-tested with fakes),
  `firmware/wifi_network_device.h` (mbedTLS GCM, the random source, the key's
  NVS port), `include/newt/secret.h` (the password and key type: no copy, no
  print or text path, wiped on clear, move and destruction).

## Commit rules, as built

- **Save:** read the key. A missing key is made, written and verified by
  readback before anything is encrypted. A key read that fails saves nothing.
  Then seal with a fresh nonce, commit the record with the durable commit
  and publish the network only when it is verified.
- **Forget:** commit "no network" and verify it, then clear the runtime
  choice, then erase the key (only if one exists). Erasing needs no free NVS
  space, where a replacement needs room for a copy (a full NVS refused it on
  the bench); the next Save makes a fresh key. An unconfirmed erase changes
  nothing visible.
- **Results:** Saved after a verified write or a readback that finds the new
  record. Not saved when nothing was written, or a trusted readback finds the
  old record or none. Otherwise Unconfirmed. The key follows the same rule.
- **Restore** (each update-mode boot, from the card and NVS only): no
  network for a missing or corrupt record, a missing or short key, NVS that
  will not open, a record another key sealed (another X3's card) or a changed
  byte. Restore writes and repairs nothing.
- **Damaged record:** the owner's explicit Save or Forget may replace a
  damaged record of this schema, committed and verified before it shows. The
  commit writes one slot, so the other keeps the damaged bytes. A card read
  error or a record of another schema still refuses.
- **Not bound to a card.** A record opens on any card while this X3 keeps its
  key. After Forget, an old record copied back opens nothing only if the key
  erase succeeded (as docs/082 designed).
- **Fence:** the runtime commits only while the NVS guard's Wi-Fi fence is
  down; the key's read-write open goes through the guard too. A scan whose
  Wi-Fi may still run restarts into Newt before any page that could save.
- RTC holds no part of the network or key.

## Tests

- `tests/wifi_network_store_test.cpp`: the rules above with fakes: key
  first, fresh nonces, refused, lost and unsynced writes, Forget order, every
  restore default, another card, a changed byte, a damaged record replaced by
  Save and by Forget (a read error and another schema still refuse), and the canary (no name or
  password in plain on the card or in NVS).
- `tests/wifi_setup_test.py` + `tests/wifi_setup_fixture.h`: update mode's
  actual runtime from `paintUpdate` to `enterRequestedUpdateMode`, the actual
  scan, button map and NVS guard. Buttons walk the real pages through a
  reading boot (loads nothing), save, cold boot, refused and unsynced saves,
  an open network, a fenced save, Forget then restart, a missing key, a
  corrupt record (then a save that replaces it and survives a cold boot) and
  no card. At each restart, before anything unwinds, the password and the
  entropy pool must be wiped. Canary over DEV lines, card bytes and NVS.
  Two static checks: only `runUpdateMode` reaches this code, and only
  `leaveUpdateMode` restarts. Negative controls, each one changed firmware
  line: restore removed, fence check removed, a restore in a reading boot, a
  restart that skips `leaveUpdateMode`, full refresh removed, password wipe
  after the commit removed, DEV FRAME flag removed, a name printed, the exit's
  entropy wipe removed, the exit's password wipe removed, the damaged-record
  opt-in removed, and no scan draw.
- `tests/update_screen_test.cpp`: rows, picking, typing, Delete, Shift and
  symbols, Save at 8, masking (same length, same frame), and every way out
  wipes the password. `tools/gate_previews.py` renders each page.

## Measured

Clean `--health-report` build at `4eafc4da`, merged with #157 (flash record
76, CODE_HEALTH.md "Wi-Fi setup"). Flash 2,858,944 → 2,878,512 B (+19,568 B).
Resident RAM (125,296 B), the lent range (22,528 B), RTC (300 B) and every selected stack
ceiling are unchanged. Compile evidence only.

## Slice B, as built

- **Test.** On the list, **Test <name>** joins the saved network once:
  "Connecting...", then one of "Connected. Wi-Fi is off again.", "Wrong
  password. Pick the network to type it again.", "Network not found.",
  "Connected, but the router gave no address.", "No answer in time. Try
  again." or "Newt could not connect." Back returns to the list. The row is
  "Test", not "Connect": the result is a check, and Wi-Fi is off after it.
- **One join** (`joinWifiNetwork`, `firmware/update_wifi.h`): the fence up,
  station on with esp_netif's default station and its DHCP client, one
  `esp_wifi_connect`, then up to 20 s for an address or a disconnect. The
  disconnect's 802.11 reason gives the result: 14, 15, 23, 202 and 204 are
  a wrong password; 201 and 210-212 not found; any other a failure. No
  retry. Then disconnect, an empty config in place of the network's, stop,
  deinit, the netif and handlers gone, the event loop deleted. If Wi-Fi may
  still run, update mode restarts into Newt. Saves stay as in slice A: only
  with Wi-Fi off and the fence down. An open network joins with no password.
- **Secrets.** Test restores the network from the card and key into the
  page's password buffer, copies it into the driver's config (RAM only:
  Wi-Fi's NVS stays off and the fence refuses any write), and wipes the
  buffer and the config copy before the driver starts. `leaveUpdateMode`
  now first replaces the driver's config with an empty one, stops and
  deinits Wi-Fi (each refuses harmlessly when Wi-Fi is already off), then
  wipes Newt's buffers and restarts. The DEV row `Newt Wi-Fi join: result
  R, reason N, error E, ...` carries codes and times only; the name,
  password and address never print.
- **Heap cleared before it is registered** (`firmware/heap_clear.h`). IDF
  registers the heap in two steps; a `--wrap` before each clears exactly the
  regions that step registers. `heap_caps_init` takes its regions from
  `soc_get_available_memory_regions`: the wrap clears all but the startup
  stack's. `main_task`, on its own stack, then registers the startup stack's
  regions: the wrap of `heap_caps_enable_nonos_stack_heaps` clears them
  first. ROM and static data stay untouched. So no boot's allocations see a
  panicked update-mode boot's freed blocks. `tools/verify-next-boot.py`
  (fact 4) fails the build if a clear is removed, moved after its
  registration, or called from anywhere else.
- **Link-check precision.** lwIP's code now starts on a 64 KiB boundary
  that one word of the reader font's pixels equals by chance. In Newt's own
  objects, `tools/verify-wifi-link.py` now counts a data word that equals a
  function start as a pointer only where the object relocates it
  (`tools/object_relocations.py`). Archive members stay as before, and an
  object it cannot read stops the check.
- **Tests.** `tests/wifi_setup_test.py` drives Test through the real runtime
  for each result, a save after a join, an open network and a teardown
  fault (nine new negative controls: each wipe, the empty configs, the exit's
  Wi-Fi stop, the wait, the netif and handler teardown, a printed name).
  `tests/heap_clear_test.py` links `heap_clear.h` with `platformio.ini`'s
  wraps against a model of IDF's two registration steps (controls: either
  clear removed or moved, the wrong regions, an overrun, a missing wrap).
  `tests/next_boot_test.py` and `tests/wifi_link_test.py` break each new
  linked-image rule.
- **Memory** (flash record 77, re-measured at `777de18c` on the source merged
  with main `c77f0644`; [084](084-wifi-memory.md)). Flash 2,878,512 →
  3,025,200 B (+146,688 B). lwIP and esp_netif data sit in the lent range,
  which stays 22,528 B: its pad shrinks from 5,104 to 1,856 B. Resident RAM
  (125,296 B), RTC (300 B) and every selected stack ceiling are unchanged.
- **Bench** (2026-10-29, USB power only; [084](084-wifi-memory.md)). Reading
  boots were no worse than alpha.1. A bench-only image saved a made-up
  network that the scan never heard, and Test's DEV row gave "not found"
  with Wi-Fi off after (no frame was captured). Back restarted into a reading boot that lent
  22,528 B, and the NVS bytes did not change. The heap clear cost 0.82 ms
  per boot. No real network was joined.
- **Bench, part 2** (2026-10-30, USB power only, the source merged with
  main `c77f0644`). On the bench X3's full NVS (503 of 504 entries written),
  the product image's Forget erased the key (`key-written 1, key 0`): the
  next backup had no `newt_wifi` key, and its three entries were erased.
  The next Save wrote a fresh key (`key-written 1, key 0`), and a last
  Forget erased it the same way. Forced crashes: 082. Reading boots: 084.
- **Bench, owner hands-on** (2026-10-30, main `72cad783`, the owner's real
  network). On USB: Save, then Test connected. After USB sleep and wake the
  network was restored and Test connected (3.4 s, Wi-Fi off after). After
  battery sleep and wake (a cold power-on on battery) it was restored, and
  Test connected once USB was back (3.5 s). On USB the Wi-Fi row only opened once a computer
  program held the port open; the Settings row's change fixes that check.
- **Bench, Settings row** (2026-10-30, `728f549f`, USB power, DEV USB).
  Settings ends with Wi-Fi. Update opened on its two card offers with no
  Wi-Fi row. Settings → Wi-Fi opened update mode and scanned at once. Back
  from either page restarted into Home with the same free heap (146,276 B).
  The battery entry is the owner's check.

## Follow-ups

- Hidden networks (type a name). Enterprise, WEP and OWE networks: only
  `WIFI_AUTH_OPEN` counts as open today.
- DIAG `wifi=saved|none`.
- esp_netif's lwIP task keeps running after Test until update mode restarts
  (lwIP has no deinit); it holds no network or password.
