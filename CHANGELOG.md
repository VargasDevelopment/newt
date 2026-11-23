# Newt firmware changelog

At adoption, no firmware release had been tagged. `0.1.0-alpha.0` was introduced
as an unreleased label, replacing the About screen's M1 UX candidate text. Existing candidate
images and acceptance evidence retain their original identities.

Generated entries begin after the reviewed adoption baseline
`1e352f752979ec3cff8e28e21f6a0bd6e6716744`; earlier commits are not reconstructed
as releases. Version numbers describe classified changes, not acceptance.

The closed release-adoption boundary is `1e352f752979ec3cff8e28e21f6a0bd6e6716744`; it preserves the existing private history and the selected runtime snapshot without changing their source.

Source-import closure for PR #16: immutable checkpoint
`fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d` preserves the separate runtime branch
authorship and subjects through an explicit bounded frozen-source import exception. New integration commits remain strictly conventional;
no firmware version, tag or installed image is changed by that closure.

## v0.3.0

### Feat
- **update**: offer test builds behind a hidden Developer page (NEWT-99, NEWT-113, #15)
- **release**: stamp and publish nightly test builds (NEWT-99)
### Fix
- **release**: list What's new lines the repository's published notes have not shown (NEWT-77, #9)

## v0.2.0

### Feat
- **update**: read updates from the public source repository
### Fix
- **installer**: read in 256-byte packets and really reset the X3, so macOS Chrome can check it (NEWT-77, #4)
- **ci**: gate a pull request's own head, not GitHub's merge commit (NEWT-77, #3)
- **copy**: send newcomers to the install page and say what is untested (NEWT-69, #396)
- **gate**: run the gate from a stock Linux shell (NEWT-69, #394)

## v0.1.0-alpha.22

### Fix
- **reader**: treat a press that stops the wake open as a pause, not a failure (NEWT-98, #388)
- **reader**: treat a press that stops the wake open as a pause, not a failure (NEWT-98)
- **reader**: treat a press that stops the wake open as a pause, not a failure
- **health**: accept a pre-record base only when no limit moved (NEWT-69, #298)
### Refactor
- **health**: keep only current limits and check changes against the base commit (NEWT-69, #385)
- **health**: keep only current limits and check changes against the base commit (NEWT-69, #298)
- **health**: keep only current limits and check changes against the base commit (NEWT-69, #298)

## v0.1.0-alpha.21

### Feat
- **preview**: run the real firmware on the host for every preview (NEWT-89)
- **preview**: drive the firmware host build from the newt-preview command line
- **preview**: build the real firmware for the host over board, input, NVS and card fakes
- **preview**: radio, HTTPS and partition fakes for the firmware host build
- **preview**: render the firmware's own Rust screens on the host (NRUST-85)
### Fix
- **update**: refuse a card firmware file with bytes after its image (NEWT-96, #373)
- **update**: refuse a card firmware file with bytes after its image (NEWT-96, #368)
- **update**: allow erased padding after a card firmware image, as the install page does (NEWT-96, #368)
- **update**: refuse a card firmware file with bytes after its image (NEWT-96, #368)
- **quick-start**: drop the false claim that Newt/Books is listed first (NEWT-97, #374)
- **quick-start**: drop the false Newt/Books listed-first claim (NEWT-97, #363)
- **quick-start**: drop the false Newt/Books listed-first claim (NEWT-97, #363)
- **display**: power the panel off after every gray image (NEWT-91, #370)
- **display**: power the panel off after every gray image
- **display**: power the panel off after every gray image
- **preview**: pass the preview CLI to the UI suite and restore the unavailable-card frame
- **preview**: run the firmware's own Settings, update entry, Quick Start and sleep owners (NRUST-85)
- **gate**: give the landing test its own temporary folder (#353)
- **gate**: give the landing test its own temporary folder (NRUST-104)
- **gate**: lock the shared Rust target and show a failed check's output (NRUST-104)
- **gate**: give each Rust check its own lock and temporary folder (#345)
- **gate**: keep newt-preview out of the landing's portable test count (NRUST-85)
- **preview**: paint the sleep Home's clock without a fresh read (NRUST-85)
### Refactor
- **salamander**: name the device layer, gate natives and derive geometry from the device (NEWT-94, #377)
- **salamander**: name the device layer and keep natives in it (NEWT-94)
- **salamander**: name the device layer and keep natives in it
- delete C++ Newt; one Rust firmware (NEWT-90, #359)
- delete C++ Newt; one Rust firmware (NEWT-90)
- **firmware**: move the native sources the Rust image links under rust/
- **preview**: enter the firmware from a C main like the device; read native pins directly
- **preview**: delete the Rust preview crate; pin the firmware host build by allowlist
- **test**: share the firmware host build recipe (NEWT-89)
### Perf
- **preview**: keep each build's first boots and copy them into every world
- **gate**: run the Rust checks in parallel and clean their temp folders (#352)
- **gate**: count each Rust check as one gate job (NRUST-104)
- **gate**: run the Rust checks in parallel within the gate's jobs (NRUST-104)

## v0.1.0-alpha.20

### Feat
- **home**: report Home's one cover read as a DEV row (NRUST-94)
- **dev-usb**: keep the number-only COVER, COVER_CACHE and HOME_COVER rows (NRUST-94)
### Fix
- **home**: show the current book's cover on Home again (#350)
- **gate**: keep the safety profile and producer patch suite running (NRUST-103)
- **home**: pass the cover source the terminated book name (#316)

## v0.1.0-alpha.19

### Fix
- **reader**: open a book only with C++'s heap headroom (NRUST-101)
- **reader**: keep the admission sizes free of a panic site (NRUST-101)
- **storage**: publish save and storage notices at boot and before sleep (NRUST-99)
- **sleep**: repaint the sleep image when the destination commit warns (NRUST-99)
- **home**: mark progress unavailable when a progress load fails (NRUST-99)
- **storage**: publish the NVS guard's issue and save notice at boot (NRUST-99)
- **library**: follow the card in Books and show why an open failed (NRUST-97/98)
- **library**: Books follows the card and names why an open failed (NRUST-97, NRUST-98)
- **reader**: credit a bound session with C++'s allocator allowance (NRUST-101)
- **startup**: show a start failure's reason and halt instead of restarting (#324)
- **startup**: show a start failure's reason and halt instead of restarting (NRUST-100)
- **reader**: open a book only with C++'s heap headroom (NRUST-101)
- **home**: show a finished book at 100% on Home and the sleep screen (#319)
- **cover**: draw the JPEG covers C++ drew and retry old no-cover records (NRUST-94)
- **cover**: read pre-#316 no-art records once more (NRUST-94)
- **jpeg**: accept the SOS, Huffman and DC inputs TJpgDec draws (NRUST-94)
- **home**: follow the exit fold's finish on Home and sleep (NRUST-96)
- **home**: show a finished book at 100% on Home and the sleep screen (NRUST-96)
- **home**: load Home's title and progress from card records at boot (NRUST-93)
- **home**: load Home's title and progress from card records at boot (NRUST-93)
- **stats**: read book records while a matching check is pending (NRUST-93)
### Refactor
- **reader**: keep the Books card note with the Books work (NRUST-97)
- **home**: keep one copy of the record code for #317 (NRUST-93)

## v0.1.0-alpha.18

### Fix
- **storage**: reclaim legacy NVS after durable SD preservation (NRUST-90)
- **storage**: reclaim admitted legacy namespaces independently (NRUST-90)
- **storage**: reclaim legacy NVS after durable SD preservation (NRUST-90)
- **ci**: prevent native fixture writes to shared Python (NRUST-91)
- **ci**: prevent native fixture writes to shared Python
- **ci**: isolate and migrate the Rust raw archive cache (NRUST-86)
- **ci**: migrate legacy Rust archives before raw cache restore
- **ci**: separate Rust archives from the native raw cache
### Refactor
- **storage**: reuse the mounted state router during reclaim (NRUST-90)

## v0.1.0-alpha.17

### Feat
- **firmware**: prepare the full Rust daily release (NRUST-86)
- **rust**: land the self-contained Rust X3 app (NRUST-83)
- **rust**: land self-contained X3 app with bounded parity checks
### Fix
- **rust**: take the packet Python from the shared runtime owner (NRUST-84)
- **rust**: use the shared Python runtime in packet checks (NRUST-84)
- **gate**: walk shared Rust input dependencies (NRUST-84)
- **rust**: make suite ownership and controls fail closed (NRUST-84)
- **rust**: scan raw fixture bytes before landing
- **rust**: close landing privacy and guard gaps
- **rust**: preserve canonical third-party licence references
### Refactor
- **rust**: remove retired clock and font bridge

## v0.1.0-alpha.16

### Feat
- **about**: what's new in 0.1.0-alpha.16
### Fix
- **reader**: serve a whole chapter over 128 KiB from its cache at open and wake (#287)
- **reader**: serve a whole chapter over 128 KiB from its cache at open and wake
- **pause**: report a lost-acknowledgement Pocket Pause save as unconfirmed (#284)
- **pause**: report a failed Pocket Pause save as unconfirmed

## v0.1.0-alpha.15

### Feat
- **about**: what's new in 0.1.0-alpha.15
- **quick-start**: first-run quick start, once per card (#272)
- **quick-start**: first-run quick start, once per card (NEWT-83)
- **quick-start**: run the quick start once per card on the X3 (NEWT-83)
- **quick-start**: first-run quick start screens and host preview (NEWT-83)
- **library**: list every book on the card (#269)
- **library**: list every book on the card
- polish Newt for newcomers (#265)
- polish Newt for newcomers (NEWT-81)
- **clock**: set the X3 clock from the update check's Date (#268)
- **clock**: set the X3 clock from the update check's Date (#261)
- **bench**: install another firmware into the slot that does not boot (#267)
- **bench**: install another firmware into the slot that does not boot (NEWT-82)
- **bench**: install another firmware into the slot that does not boot (NEWT-82)
- **stats**: say how the clock gets set (#261)
- **clock**: set the X3 clock from the update check's Date (#261)
- **reader**: read upright on new installs and keep landscape on cards that ran Newt
- **home**: say Read to begin for a book not yet opened
- **settings**: add an About row and name Newt for the X3
- **installer**: match the books line and warn before replacing an unnamed firmware
- **library**: list every book from a list on the card
### Fix
- **quick-start**: show each step only after its record, and enter Wi-Fi through the reader-save guard
- **quick-start**: keep the zone row focused after an unconfirmed save; Welcome says where Skip leads
- **bench**: preserve recovery safety for spare firmware installs
- **clock**: leave no plausible time after a partial write (#261)
- **reader**: decide the first orientation only from a card with no Newt history, before the first note
- **library**: show only shelves that read back; sweep leftover scratch
- **dev-usb**: decode the LIBRARY row of firmware before NEWT-80
### Refactor
- **health**: keep the state backend and update fixture under their line thresholds
- **reader**: let the card history name its records beside the store
- **reader**: pass the card history its records from their own constants
- **library**: pace long scans in SdLibrary, by the scan's own clock
### Perf
- **quick-start**: route its presses in the loop, keeping dispatchAction's frame

## v0.1.0-alpha.14

### Feat
- **about**: what's new in 0.1.0-alpha.14
- **library**: show EPUBs anywhere on the card (#258)
- **library**: show EPUBs anywhere on the card
- **library**: show EPUBs anywhere on the card
- **release**: first public release and install page hosting from the public repo (#257)
- **release**: first public release and install page hosting from the public repo
- **installer**: deploy the install page to Pages from each public release (NEWT-75)
- **release**: keep the source offer where the firmware reads releases (NEWT-75)
- **release**: publish where the release's firmware reads updates, with a launch hop (NEWT-75)
### Fix
- **library**: keep the hints load one measured frame
- **library**: keep rollback saves and hide only proven copies
- **library**: walk Newt/Books first and hide copies as they are found
- **release**: accept CI's tag in the source repository and hop only after it published (NEWT-75)
- **release**: drop the changelog's private start revision (NEWT-75)
- **oss**: commit every exported entry in the clean export's root (NEWT-75)
- **oss**: store only the historic files a reader consumes (#137)
### Perf
- **reader**: Back paints Home before its optional records (#259)
- **reader**: Back paints Home before its optional records
- **reader**: Back paints Home before its optional records (NEWT-76)

## v0.1.0-alpha.13

### Feat
- **about**: what's new in 0.1.0-alpha.13
- **gate**: split the host gate into shards from its own plan
- **oss**: let the external terms file carry exact exceptions (#137)
- **release**: let a clean export's root start the release history (#137)
- **oss**: add a clean export with a leak scan (#137)
### Fix
- **ci**: fail public CI's gate job on any unsuccessful part
- **update**: recover a Wi-Fi download from a failed card write (#221) (#249)
- **update**: recover a Wi-Fi download from a failed card write (#221)
- **oss**: print flagged names only by position and never a URL's userinfo (#137)
- **oss**: key each scan exception to its exact matched value (#137)
- **oss**: scan every exported name and byte or fail the scan (#137)
- **release**: accept a snapshot root only when its tree reproduces its trailer (#137)
- **oss**: rebind the Frosted handoff README's frozen hash (#137)
- **health**: spell receipt runtime paths relative to home (#137)
- **update**: start the image download again after a failed card write (#221)
### Perf
- **gate**: put phases that share a uses path in separate shards

## v0.1.0-alpha.12

### Feat
- **about**: what's new in 0.1.0-alpha.12
- **slots**: switch to and install any other X3 firmware (NEWT-71) (#243)
- **slots**: switch to and install any other X3 firmware (NEWT-71)
- **stats**: estimate CrossInk days, label the day axis and show book graphs at once (#239)
- **stats**: estimate CrossInk days, label the day axis and show book graphs at once
- **stats**: estimate CrossInk days and label the day axis
- **installer**: use the generic slot rule and name the switch back
- **installer**: install Newt from a web page over USB (#240)
- **installer**: install Newt from a web page over USB
- **installer**: show the approved Newt wordmark in the header
- **installer**: install Newt or another firmware into the X3's spare slot
- **firmware**: name Newt's image "Newt" in its app descriptor
- **slots**: switch to and install any other X3 firmware
### Fix
- **stats**: read no CrossInk source at paint; repaint the panel after the book's import
- **stats**: show the book panel's graphs at the first paint
- **update**: never say nothing changed after an unconfirmed settle
- **slots**: refuse images the bootloader rejects for segment placement
- **slots**: refuse an OTA sequence that would reach 0xffffffff
- **installer**: check the firmware that stays and refuse an exhausted sequence
### Refactor
- **update**: keep the install's status pages with the others
### Perf
- **stats**: keep the axis scale tables out of the stack

## v0.1.0-alpha.11

### Feat
- **about**: what's new in 0.1.0-alpha.11
- **reader**: mark a book as finished (#235)
- **reader**: mark a book as finished (#105)
- **stats**: graph reading days and time of day on both screens (#230)
- **stats**: graph reading days and time of day on both screens
- **stats**: keep every reading day per book and show three views on both screens
- **preview**: render the statistics graphs from invented fixtures
- **stats**: draw the days-read and time-of-day graphs in shared code
- **firmware**: commit the finished mark and restore it from the card
- **home**: show a finished book as 100% on Home and the sleep screen
- **reader**: add the Reading menu's Mark as finished row
- **stats**: mark a book finished, or not, in one verified ledger commit
### Fix
- **reader**: count a bound CrossInk finish once and keep undo for the open
- **stats**: sum a book's CrossInk history only after a complete import
- **wifi**: count a 4 KiB-aligned address the Wi-Fi link check sees as a move
- **preview**: take the statistics previews' directory as an argument
- **reader**: prefetch beyond bounded short-spine chains (#232)
- **reader**: prefetch beyond bounded short-spine chains
- **reader**: release the idle length-probe descriptor
- **reader**: prepare past bounded short-spine chains
- **reader**: say a book is still opening instead of not saved
- **reader**: read the book's last page as 100%
### Refactor
- **reader**: keep the #235 fixes inside the line ceilings
- **firmware**: keep main.cpp within its line ceiling after #230
### Perf
- **stats**: keep the statistics panel's view out of every reader paint

## v0.1.0-alpha.10

### Feat
- **about**: what's new in 0.1.0-alpha.10
- **update**: name the failed card step on the DEV Card failed page (#227)
- **update**: name the failed card step on the DEV Card failed page
- **update**: name the failed card step on the DEV Card failed page
- **update**: name the failed card call and its errno in a DEV row
### Fix
- **reader**: prefetch a long next chapter's first segment (#196) (#226)
- **reader**: prefetch a long next chapter's first segment (#196)
- **reader**: repair a long next chapter's evicted first segment (#196)
- **reader**: prefetch a long next chapter's first segment and keep its own tries
### Refactor
- **reader**: keep the revision session within its line ceiling

## v0.1.0-alpha.9

### Feat
- **about**: what's new in 0.1.0-alpha.9
### Fix
- **reader**: show the resume picture at every wake (#222) (#223)
- **reader**: show the resume picture at every wake (#222)
- **reader**: read a closed book's source hints by name (#222)

## v0.1.0-alpha.8

### Feat
- **about**: what's new in 0.1.0-alpha.8
- **reader**: show the last page from the card at wake and Home → Read (#215)
- **reader**: show the last page from the card at wake and Home → Read
- **reader**: show the last page from the card at wake and Home → Read
### Fix
- **reader**: scope the picture's gate and answer a held press before slow work
- **sleep**: count this month and last month for When you read (#217)
- **sleep**: count this month and last month for When you read
- **sleep**: show the live chapter and percent when sleeping from the reader
- **sleep**: show nothing while today's month is archived
- **sleep**: count this month and last month for When you read
- **reader**: read the resume picture before asking its gate
- **health**: an exception table's bytes are never a stored address
- **reader**: gate the resume picture on the ordinary open's rule
- **reader**: name the resume picture's panel pass in the panel ring

## v0.1.0-alpha.7

### Feat
- **about**: what's new in 0.1.0-alpha.7
- **sleep**: opt-in sleep screen extras and holiday hats (#212)
- **sleep**: opt-in sleep screen extras and holiday hats
- **sleep**: opt-in sleep screen extras and holiday hats
### Fix
- **health**: the sleep marks' run tables are glyph data to the crash output guard
- **sleep**: keep NWTS v1 and prove the sleep streak before showing it
- **sleep**: restore the pre-#202 sleep renderers verbatim for the all-Off path
- **sleep**: keep the sample sleep render's calls as before #202 with every extra Off
- **sleep**: finish the Stage sleep render without extras as before #202
- **sleep**: keep the Stage sleep render as deep as before with every extra Off
- **sleep**: keep the measured sleep paint and summary as the all-Off path
- **sleep**: pass the build identity before the Sleep screen choices
- **sleep**: take the preview output directory as an argument

## v0.1.0-alpha.6

### Feat
- **about**: what's new in 0.1.0-alpha.6
- **reader**: serve anchored reads from a chapter's segment map
- **stats**: import CrossInk reading history once at Home idle (#200)
- **stats**: import CrossInk reading history once at Home idle
- **dev-usb**: keep the CrossInk import's number-only row
- **stats**: note CrossInk history on the library page
- **stats**: import CrossInk history once at Home idle
- **stats**: CrossInk record formats and union streaks
### Fix
- **reader**: evict nothing for a chapter opened from its segments
- **dev**: let an anchor's map reason replace a native missing-file Attach
- **reader**: serve a cold anchored open from a segment over the opening cap
- **reader**: keep the identity size lookup inline with packed flags
- **reader**: remember a third absent cache name for an anchor's map
- **reader**: count a chapter's map only when no segment went unprobed
- **reader**: keep a chapter without a map to one lookup
- **ui**: say Wi-Fi in the system bar while Wi-Fi may run (#206)
- **ui**: say Wi-Fi in the system bar while Wi-Fi may run
- **ui**: declare the fence read alone so the bar needs no NVS seams
- **ui**: say Wi-Fi in the system bar while Wi-Fi may run
- **health**: an embedded file's bytes are never a stored address
### Refactor
- **reader**: keep the page cache header within 500 lines
- **reader**: keep the SD page storage within 500 lines
- **health**: fit the embedded-file exception in the guard's 500 lines
- **stats**: keep the Newt-only summary as its own body
- **stats**: walk the card for CrossInk books without recursion
### Perf
- **reader**: serve open, wake, jump and crossings from cached segments (#210)
- **reader**: serve open, wake, jump and crossings from cached segments

## v0.1.0-alpha.5

### Feat
- **about**: what's new in 0.1.0-alpha.5
### Fix
- **reader**: one attempt writes every segment on a crowded Pages/ (#193)
- **reader**: one attempt writes every segment on a crowded Pages/
- **reader**: the census frees entries only for missing segments it keeps
- **reader**: one attempt writes every segment on a crowded Pages/
- **ci**: classify release runs on a hosted runner and select controls by use
- **gate**: mark the scratch audit's start with a file time, not the fine clock
- **ci**: never let a push cancel a release merge's run
- **ci**: budget the CI job once, not its host gate too
- **reader**: zero the packed storage tail in its constructors
- **reader**: keep the merged storage tail within the session owner
- **health**: keep a one-operand jump as a lui page's use
- **health**: skip a register reload when resolving a lui page
- **reader**: bound producer files by the census entry reserve
- **reader**: keep the .fibp suffix on segment cache names
- **reader**: keep legacy record adoption out of a recursive call chain (#189)
- **reader**: keep legacy record adoption out of a recursive call chain
- **reader**: keep legacy record adoption out of a recursive call chain
- **reader**: keep derived book records out of the page cache's folder (#184)
- **reader**: keep derived book records out of the page cache's folder
- **reader**: keep derived book records out of the page cache's folder
### Refactor
- **reader**: admission derives the segment need from the shown page
- **reader**: give segment geometry and naming their own header
- **reader**: read a segment out of line from show()
- **reader**: name a segment from its chapter's name
- **reader**: recognize a segment from its file, not a parameter
### Perf
- **reader**: the census converges on a full Pages/ (#190)
- **reader**: the census converges on a full Pages/
- **reader**: the census converges on a full Pages/
- **gate**: run the gate and CI in minutes again, under a total time budget (#188)
- **gate**: run the gate and CI in minutes again, under a total time budget
- **gate**: start selected safety first in every tier, so it fails in minutes
- **gate**: budget the local gate from its measured push tier
- **ci**: budget the per-push job from its 8-core replay
- **gate**: start the longest remaining chain first
- **gate**: report a busy host's overrun without failing the gate
- **ci**: budget the per-push CI job for the runner's 8-CPU cap
- **gate**: read one shared producer SDK copy instead of a serial chain
- **ci**: pick the tier per run and reuse checked toolchains and objects
- **gate**: split the gate into push, local and full tiers under a time budget
- **reader**: cache chapters over 128 pages as 128-page segments (#175) (#177)
- **reader**: cache chapters over 128 pages as 128-page segments (#175)
- **reader**: spend producer card credit as bytes are written
- **reader**: cache chapters over 128 pages as 128-page segments

## v0.1.0-alpha.4

### Feat
- **sleep**: a witch hat for the sleeping newt on Halloween
- **dev**: keep the census and runtime rows on the DEV link (#182)
- **dev**: keep the census and runtime rows on the DEV link
- **dev**: keep the census and runtime rows on the DEV link
### Fix
- **sleep**: a bigger Halloween hat whose brim stands clear of the head

## v0.1.0-alpha.3

### Feat
- **airspace**: spotlight aircraft one at a time, with class silhouettes and landscape (#176)
- **airspace**: spotlight aircraft one at a time, with class silhouettes and landscape
- **airspace**: redraw the silhouettes with pylons, wing roots and small forms
- **airspace**: spotlight aircraft one at a time, with class silhouettes and landscape
### Fix
- **update**: use Wi-Fi and install updates on battery (#183)
- **update**: use Wi-Fi and install updates on battery
- **update**: use Wi-Fi and install updates on battery
- **airspace**: shrink the spotlight grid's gaps before cutting a fact
- **airspace**: fit a five-digit climb in the landscape detail
- **airspace**: keep the detail's prediction whole when the tags wrap
- **airspace**: show every status, climb rate and tag whole
### Perf
- **airspace**: clear cut windows in the scanline fill

## v0.1.0-alpha.2

### Feat
- **update**: install Newt over Wi-Fi from GitHub Releases (#173)
- **update**: install Newt over Wi-Fi from GitHub Releases
- **update**: check the public newt-releases repository
- **update**: check GitHub for a newer Newt from the Update list
- **update**: read GitHub's latest release and download Newt to the card
- **release**: define and tool the public GitHub release step (#170)
- **release**: define and tool the public GitHub release step
- **release**: add the GitHub release publish step
- **airspace**: add Airspace's model, ranking and screens (#169)
- **airspace**: add Airspace's model, ranking and screens from fixtures
- **preview**: render every Airspace screen and state from invented fixtures
- **airspace**: add Airspace's screens, its keyboard entry and the Apps page
- **airspace**: add the aircraft snapshot model and pass-overhead ranking
- **settings**: give Wi-Fi its own Settings row and detect a computer's USB by its frames
- **settings**: give Wi-Fi its own Settings row
- **settings**: give Wi-Fi its own Settings row
- **update**: test the saved Wi-Fi network with one join in update mode (#162)
- **update**: pick a Wi-Fi network and save its password in update mode (#161)
- **wifi**: test the saved network with one join in update mode
- **wifi**: pick a network and type its password in update mode
- **wifi**: link Wi-Fi with its memory lent to reading, and scan networks in update mode
- **wifi**: count what the flash fence refuses in the DEV scan row
- **wifi**: show each big used heap block on every DEV boot
- **wifi**: scan from update mode with Wi-Fi's memory lent to reading
- **update**: add a Wi-Fi networks page to the Update screen
### Fix
- **update**: leave TLS room to reach GitHub in update mode (#178)
- **update**: leave TLS room to reach GitHub in update mode
- **update**: mark the frame private first in paintUpdate, lent or not
- **update**: grow the download bar through paintUpdate's one panel call
- **update**: lend the frame to TLS so Check reaches GitHub in update mode
- **health**: exempt only .eh_frame from the stored-address scan
- **health**: review glyph and call-frame bytes in the stored-address scan
- **update**: drop presses made during a check
- **update**: end each GitHub file at one time limit, even when trickled
- **release**: resume a draft only when its title, notes and flag match
- **release**: publish a verified draft and never delete a release
- **airspace**: rank by great-circle approach in the aircraft's frame
- **update**: start the host preview's update mode on its list
- **update**: count a computer's USB by its frames, not an open serial port
- **wifi**: review lwIP's IPv6 header word in the crash output guard
- **wifi**: erase the key on Forget so a full NVS cannot keep it
- **update**: name unnamed console-register words by their map section
- **update**: withhold crash and diagnostic output in update mode (#160)
- **update**: close every VFS console route and inventory console hardware
- **update**: gate ROM print and update mode's logs at the console channels
- **update**: gate all crash output at the panic handler's entry, fail closed
- **update**: include the crash output wraps through update mode's header
- **update**: write no core dump and print no registers or stack in update mode
- **boot**: clear the heap's RAM before IDF registers it
- **wifi**: wipe the password and entropy pool before every update-mode restart
- **wifi**: let an explicit save or Forget replace a damaged network record
- **wifi**: keep the fence's counts in padding the guard already has
- **wifi**: fence every flash write while Wi-Fi may run
- **wifi**: check every reference into the lent range by address
- **wifi**: round the FATFS context to the lent heap's TLSF classes
- **wifi**: pad the lent range to hold the SD card's FATFS context
- **wifi**: lend Wi-Fi's memory one priority below the RAM heap
### Refactor
- **update**: move the status pages and the IDF Wi-Fi model into their own files
- **wifi**: keep Wi-Fi setup within the code-health ratchet
- **wifi**: move the lent-heap check into its own tool module

## v0.1.0-alpha.1

### Feat
- **ota**: install Newt or CrossInk from an image on the SD card (#150)
- **storage**: let a note's durable store replace damaged records
- **ota**: approve the bench and main CrossInk images for card installs
- **ota**: add update mode, first-paint confirmation and the once-only Updated page
- **ota**: add the update publish step
- **ota**: add the identity block and the no-CrossInk slot rule to the tools
- **ota**: add the Update row and the shared update page
- **ota**: add the installer core, its manifest and fault harness
- **ota**: share the selection write and add the identity block
- **about**: merge the exact build and What's new
- **about**: show the exact build and a What's new page
- **home**: make Stage the only Home
- **dev**: print each book open's heap low
- **code-health**: record selected stack retirements explicitly
- **home**: make Stage the only Home
- **tools**: prepare the main X3 switch with safe resets and a way back
- **tools**: inventory NVS namespaces without exposing values
- **settings**: restart into CrossInk from Settings
- **ui**: share one status bar, list and button rail across system screens
- **display**: record DEV panel requests to diagnose the gray screen
- **ui**: draw every system screen as one shared SystemPage
- **settings**: add Restart into CrossInk
- **slots**: share the slot, selection and image rules with firmware
- **tools**: read X3 slot identity from the device before any bench write
- **stats**: record reading sessions and show reading statistics
- **stats**: show reading statistics in Settings and dates in the reader menu
- **stats**: fold at reader idle, Back, Sleep and a switch; seed at Home
- **stats**: store statistics on SD with classified loads, repairs, seed and fold
- **stats**: add the reading statistics ledger, codecs and fold
- **reader**: credit closed reading intervals and pause sessions in menus
- **reader**: show a working frame for slow presses
- **reader**: answer slow presses before their work
- **ui**: draw a quiet working frame for slow presses
- **tools**: install app images on the bench X3 with a shared lease and STOP latch
- **tools**: add identity-locked bench app installer
- **settings**: add a 12-hour or 24-hour clock format choice
- **settings**: add a 12-hour or 24-hour clock format choice
- **settings**: open device Settings with a time zone list
- **settings**: merge main's NEWT-31 reader verification
- **settings**: name the section Settings and pick zones from a list
- **settings**: open device Settings from Your X3
- **reader**: merge time zone settings with deferred opens
- **clock**: show local time with a time zone setting
- **clock**: show Home in a durable reader-chosen time zone
- **clock**: convert UTC clock readings to named local time zones
- **reader**: verify cached opens after first page
- **clock**: show the X3 clock time on Home
- **clock**: read the X3 clock without changing chip state
- **health**: admit measured RAM/stack growth records and owner-approved memory floors
- **ui**: spell text with the UI font and cut it with an ellipsis
- **reader**: prepare chapter caches after quiet Reading idle
- **reader**: reserve cache writes during library idle
- **reader**: add capped continuous-lease cache producer
- **reader**: bound producer parsing and cancellation
- **reader**: expand pinned four-face glyph coverage
- **reader**: extend pinned glyph coverage and scene separators
- **reader**: add canonical chapter menu and Home identity
- **reader**: unify canonical chapter identity
- **reader**: list book-declared chapters with the current entry in view
- **tools**: add bounded diagnostic EPUB structure probe
- **tools**: report the reader's own chapter style outcome
- **tools**: add a read-only EPUB structure probe for owner diagnosis
- **gate**: add a quick parallel pre-gate suite
- **toolchain**: share one pinned read-only toolchain via views
- **home**: integrate durable Stage and cover responsiveness on X3
- **home**: integrate Stage firmware and responsive cover production
- **home**: add the opt-in Stage Home style
- **covers**: port automatic cover extraction and the producer lifecycle
- **firmware**: produce covers while Home and the Library are idle
- **covers**: settle coverless books and decode without the image dispatch
- **covers**: decode baseline JPEG covers on the unpatched SDK
- **covers**: add bounded automatic cover producer components
- **home**: port the Home cover presentation adapter
- **home**: show the selected book's prepared cover on Home
- **home**: add the canonical Home cover snapshot and painter
- **reader**: port bounded EPUB styling and matching reading faces
- **reader**: bind bounded chapter stylesheets lazily for layout
- **font**: add matching bold, italic and bold-italic reading faces
### Fix
- **update**: offer CrossInk from the card by sizing card paths at compile time (#151)
- **update**: refuse a manifest identity that would be cut to fit
- **update**: build the card paths at compile time so CrossInk is offered
- **bench**: say why HELLO can be late in the readiness error
- **update**: confirm the image at the first finished screen of any kind
- **dev-usb**: let the bridge read requests without answering them
- **storage**: move the damaged-record opt-in onto the note's codec
- **update**: confirm a new image only after a verified finished screen
- **update**: ask for USB to a computer when power is refused
- **bench**: give every USB install a fresh selection and a verified fallback
- **ota**: refuse an install when the other slot's head cannot be read
- **reader**: preserve the measured sleep recovery boundary
- **stats**: merge the local midnight split
- **stats**: retain unsaved midnight boundaries and reject clock jumps
- **stats**: preserve midnight dating until open intervals close
- **stats**: anchor midnight before deferred source verification
- **stats**: split reading at local midnight without turn writes
- **preview**: match the firmware quiet Home default
- **home**: resolve the Stage backdrop without a style copy
- **safety**: describe the enabled firmware definitions in evidence
- **dev**: keep the background reopen's action out of its stack frame
- **dev**: watch the heap of a background changed-source reopen
- **safety**: analyze the real DEV build without recursion
- **bench**: accept clean release candidates without a DEV build ID
- **bench**: validate release identity through pinned receipts
- **nvs**: validate payload ownership and report physical capacity
- **bench**: preserve reset leases and recover confirmed Newt
- **install**: run the pinned reset sequence without reconnect retries
- **install**: select owner unit and protect boot confirmation window
- **acceptance**: retain both telemetry and slot procedure dependencies
- **storage**: report saved only after an effective sync of those bytes
- **ci**: pass fresh-checkout safety and keep accepted-open saves out of verification
- **reader**: reconcile an accepted selection once per save
- **reader**: close latent native safety diagnostic failures
- **ci**: recognize composed SDK library diagnostic routes
- **ci**: prepare native lane inputs before selected safety
- **display**: retain boot and physical panel request outcomes
- **identity**: bind panel telemetry into acceptance procedure
- **apps**: show Pocket Pause's Other only when another prompt can be picked
- **settings**: hide the zone list's Up and Down at its ends
- **ui**: keep each system page's bar out of the paint frame
- **ui**: keep the book opening page's selected stack signature
- **storage**: force a modified-handle barrier before confirming buffered state
- **stats**: defer the seed while a progress save is unconfirmed
- **storage**: make raw durable writes a capability only the commit creates
- **stats**: seed the open book from its acknowledged counters
- **storage**: sync SD files before reading or writing after a failed sync
- **selection**: ask about durability only for a matching selection
- **storage**: acknowledge a durable save only after its own sync
- **preview**: retain CrossInk switch uncertainty in the host ports
- **settings**: preserve unresolved CrossInk selections across retries
- **settings**: stay in Newt when the save or the switch is unconfirmed
- **slots**: never overwrite the confirmed fallback behind a pending selection
- **tools**: keep one serial connection through the reset and the HELLO
- **settings**: keep the CrossInk dispatch out of line
- **settings**: keep the CrossInk switch out of the press frame
- **settings**: hold no slot state while Restart into CrossInk is unused
- **tools**: bind the slot rule into the reading acceptance procedure
- **host**: give native fixtures short stable roots and reject per-run compile inputs
- **stats**: count the longest streak over every retained day
- **stats**: acknowledge a stats save only after its own fsync
- **reader**: end a reading session only on an exit from the book
- **stats**: keep each record's commit frame out of line
- **stats**: gate every statistics effect on one cancellation check and model SD durability
- **reader**: judge the end of a book from the page shown
- **reader**: keep the reading session across same-book rebuilds
- **stats**: one initializer for lease facts and a streak scan over all retained months
- **host**: reject scratch paths before object reuse
- **host**: preserve device headroom in fixture roots
- **reader**: let the refresh checks decide paints inside a press
- **reader**: keep one press record from acceptance to its destination paint
- **reader**: keep the input loop head stable for its cover hooks
- **reader**: keep the book check silent
- **host**: pass source to scoped preview reuse counters
- **host**: shorten remaining native storage fixture roots
- **host**: stabilize digest-work fixture includes
- **host**: give native fixtures short stable roots
- **gate**: finish r3 stop signal and scratch cleanup findings
- **cache**: exclude run scratch placement from host object keys
- **test**: keep native producer scratch ancestors canonical
- **test**: isolate native mount names from host scratch paths
- **identity**: bind gate scratch and CPU procedure helpers
- **gate**: own scratch before every phase and forbid fixed test roots
- **tools**: address final bench lease review findings
- **tools**: cover reading journeys with the shared bench lease
- **tools**: bind bench helpers into acceptance procedure identity
- **tools**: share bench identity and failure state across live tools
- **gate**: preserve shard controls and register children before signals
- **gate**: isolate user slots and allow measured cold CPU margin
- **gate**: reserve host capacity before local compiler slots
- **gate**: end measured prerequisites when interrupted
- **cache**: distinguish path failure from transient refusal
- **cache**: withdraw page leases after failed exit checks
- **settings**: keep the selected paint signatures for list moves
- **settings**: lend the display, not its buffer, to the Settings paint
- **settings**: keep an unconfirmed zone save reported until resolved
- **settings**: keep a default AppState all zero
- **cache**: keep helper attributes on its first declaration
- **cache**: declare query helper inline before its definition
- **cache**: release failed query descriptor generations
- **cache**: debit foreground page envelope write credit
- **cache**: bound retained admission by producer cancellation
- **cache**: require full validation before suppressing producer repair
- **reader**: finish optional metadata without rearming debt
- **reader**: save due progress across reader panels
- **reader**: trace metadata only after work admission
- **reader**: keep checkpoint loading on its measured body
- **reader**: complete paused-save and low-power regressions
- **reader**: preserve failed pause duties and share save tails
- **reader**: repair idle saves and consolidate storage admission
- **reader**: preserve explicit identity publication stack coverage
- **reader**: retain metadata debt after an input stop
- **reader**: bind deferred work and admission to publication boundaries
- **reader**: publish turn timeouts and retain saturated exits
- **reader**: honor queued exits at the turn verification boundary
- **reader**: preserve queued turns during foreground verification
- **reader**: retry only an open cache-only miss
- **reader**: keep verified turns independent of durable selection
- **reader**: complete pending uncached turns in the same action
- **reader**: resume source verification after final-probe input
- **clock**: scope the Settings note by row and keep main at its ceiling
- **clock**: commit an explicit zone choice even when it matches the shown zone
- **clock**: split DEV clock telemetry to keep the measured frame
- **reader**: retain live recovery when verification blocks saves
- **reader**: publish changed pages after bounded verification
- **reader**: preserve authority through deferred verification
- **reader**: centralize provisional page admission
- **reader**: keep deferred verification input-first
- **reader**: reverify retained opens and overview writes
- **reader**: include timing dependencies in scope-owning headers
- **reader**: preserve cover handoff before open timing
- **clock**: refresh time only during full Home composition
- **dev**: time complete idle owners including repaint
- **health**: pin the flash history with main's record 47
- **health**: bind every ceiling above its lowest appended value at adoption
- **health**: bind raised ceilings to the adopted build measurement
- **health**: bind cited receipts and pin the flash history
- **health**: admit an added stack guard only at its measured frame
- **reader**: preserve cached turns across interrupted admission
- **audit**: retain the four required image artifact hashes
- **reader**: use turn read authority for boundary cache hints
- **reader**: settle prefetch page caps before later input
- **reader**: preserve independent chapter prefetch budgets
- **reader**: label unsuccessful prefetch telemetry as stopped
- **ui**: spell Home and chapter labels with the UI font and show a plain percent
- **ui**: read a left-out title run as a space and guard a missing buffer
- **ui**: spell chapter labels and book titles with the UI font
- **home**: show an estimated percent as plain "N%" like the footer
- **reader**: keep Back and Sleep working when a save is unconfirmed
- **library**: keep refusal cause separate from shared notices
- **reader**: require verified route for Home save copy
- **reader**: retain distinct source failure classification
- **reader**: own typed storage outcomes and diagnostic retirement
- **reader**: keep footer labels legible and show the overall percent plainly
- **ui**: keep inkless characters from spending a waiting space
- **ui**: let spaces wait for ink when folding into a full buffer
- **ui**: fold text into the whole buffer and clear it on refusal
- **reader**: show an estimated overall percent without "~"
- **reader**: keep footer fields legible with a 14 px floor and ellipsis
- **reader**: keep live settings progress authoritative
- **reader**: bound recovery frames and refresh page-save state
- **reader**: classify footer save errors and bound startup restoration
- **reader**: retain save uncertainty across warm wake
- **reader**: preserve valid last-good checkpoints after chapter failures
- **reader**: keep failed reflow recovery separate from durable acknowledgement
- **reader**: capture live recovery state after unconfirmed checkpoints
- **reader**: complete warned exits within existing Home stack budgets
- **reader**: stage durable descriptor release and warned reader exits
- **cache**: retry chapter cache builds per chapter after interruption
- **cache**: preserve successful invalid-cache probe findings
- **cache**: retain protected validation stack frames
- **cache**: settle storage failures before mandatory cleanup
- **cache**: settle returned failures before later cancellation
- **cache**: settle pause failures before later cancellation
- **cache**: retain the reason for producer stops
- make producer cancellation authoritative for retries
- make terminal cache failures dominate concurrent input
- cancel cache probes while preserving corrupt-cache repair
- retry interrupted chapter cache production per visit
- **cache**: admit chapter production during quiet reading
- **cache**: settle known card loss before admission
- **cache**: deny admission when the card is removed
- **cache**: admit chapter production during quiet reading
- **reader**: budget optional metadata reads before publication
- **reader**: bound partial cache reads and trim metadata I/O
- **reader**: compose checked scopes without duplicate I/O
- **reader**: retain receipt authority across producer pauses
- **reader**: bound parser work and release pause resources
- **reader**: let the chapter cache producer run on device
- **cache**: retain the cover repaint flag
- **cache**: remove idle diagnostic gate row
- **cache**: preserve gate budget on reading reopens
- **cache**: rearm DEV gate at reading handoff
- **health**: guard indirect interrupted syscall retries
- **dev**: measure cache attempts from the idle quiet clock
- **dev**: retain the nonthrowing paint observation boundary
- **dev**: isolate display completion telemetry from paint frames
- **health**: retain the bounded retry stack measurement
- **cache**: preserve C3 types and identity stack measurement
- **cache**: admit checkpoint metadata and trace idle production
- **integration**: align producer proofs with shared navigation owner
- **reader**: poll cancellation at decoded chunk boundaries
- **probe**: use complete validated font coverage profile
- **reader**: unify font identity and bound lookup work
- **reader**: enforce reproducible font rendering and clean checks
- **reader**: preserve bounded CSS cascade fidelity
- **probe**: report integrated CSS issue vocabulary
- **gate**: give probe privacy snapshots exclusive ownership
- **reader**: preserve CSS tokens through projection
- **reader**: follow CSS tokenization for bounded style scanning
- **reader**: bound CSS admission and protect payload boundaries
- **reader**: preserve pinned SDK CSS admission semantics
- **reader**: preserve bounded paragraph CSS fidelity
- **reader**: bound interrupted SD syscall retries
- **reader**: keep progress independent of identity cache
- **reader**: order identity publications and trace durable state
- **reader**: preserve chapter identity across checkpoint crashes
- **reader**: keep chapter identity journal-only
- **reader**: migrate legacy chapter identity into journal
- **reader**: make chapter identity checkpoints atomic
- **reader**: persist identity after ownerless fallback
- **reader**: bind chapter identity to checkpoints
- **reader**: bound canonical Home chapter lookup
- **home**: keep cold identity resolution canonical
- **reader**: make chapter identity publication atomic
- **reader**: recompose after any Chapters menu retry
- **reader**: keep current-entry focus across a denied Chapters parse
- **reader**: keep a valid page ready when only the chapter menu is unavailable
- **tools**: keep the admission profile's first-page walk over every spine
- **reader**: keep selected stack entries measurable after the projection refactor
- **reader**: keep authored chapter order and compose before queued input
- **reader**: define the Chapters window once and guard an unready menu
- **reader**: route every Chapters jump outcome through one contract
- **reader**: resolve a Chapters destination before publishing anything
- **reader**: make Chapters rows exact destinations with usable titles
- **probe**: keep nested selector syntax opaque and claim book CSS before embedded
- **probe**: match elements by local name as every firmware reader does
- **probe**: fail closed when the EPUB changes during inspection
- **probe**: use firmware's ASCII whitespace sets instead of Python's Unicode defaults
- **probe**: report cover read failures, non-XHTML spine targets and late ambiguity records
- **probe**: bound style evaluation and align package, cover and inline-display facts
- **tools**: give probe facts the firmware's visible text stream
- **tools**: match probe attributes by each firmware reader's rule
- **tools**: give probe text one firmware visibility model
- **tools**: ignore display:none on the body root as firmware does
- **tools**: mirror firmware on rejected spines, CSS space, hidden local names and offset collisions
- **covers**: keep Session::xml's selected frame and result-only firmware path
- **tools**: take fallback-glyph and XML prolog facts from the reader
- **tools**: keep every firmware role on a shared ZIP record
- **tools**: inspect the package member the reader's parser chose
- **tools**: read CSS comments once, zero-width, and flag SDK-literal text
- **tools**: tokenize attributes like the SDK; flag duplicate manifest ids
- **tools**: count nested separator text once
- **tools**: keep the firmware cover record and exact element names
- **tools**: label the firmware-chosen cover member in ZIP facts
- **tools**: report the cover member firmware discovery chose
- **tools**: identify probe structure by namespace, not local name
- **tools**: take every firmware claim in the probe from shared C++
- **tools**: cap decoded probe input and report malformed structure as such
- **tools**: type every probe report value and verify the ZIP directory
- **tools**: charge one byte-derived work budget in every probe parser
- **tools**: type the probe output gate by field and walk XHTML once
- **tools**: gate every probe report field through one privacy boundary
- **tools**: report CSS indent facts without computing a cascade
- **tools**: keep the EPUB probe text-free and in firmware parity
- **covers**: align ZIP admission and reversible negative caching
- **covers**: charge interrupted flag migrations to the visit budget
- **covers**: leave negatives the flag migration cannot clear
- **covers**: share ZIP flag rules and retry affected negatives
- **covers**: keep the safe flag mask for members discovery reads
- **covers**: check only stored sizes beyond Reader rules for read members
- **covers**: give unrelated members Reader admission's ZIP rules
- **ui**: preserve battery footer digits and bounded atlas text
- **ui**: keep shrunk glyphs from vanishing and give each painter one job
- **ui**: area-average shrunk UI text so footer digits stay distinct
- **reader**: preserve italic overhang at justified line ends
- **identity**: bind every module each fingerprinted procedure runs
- **gate**: end a stopped gate within a bound, whatever its output
- **host**: identify the compiler again after the link
- **gate**: end every running phase when supervision fails
- **gate**: run every gate phase as its own process
- **toolchain**: enforce each recorded mode in store and view checks
- **gate**: fail a phase that raises SystemExit or an interrupt
- **gate**: claim each object key in fresh mode before checking its files
- **toolchain**: compare resolved store paths and document view limits
- **toolchain**: catch in-place view writes and bind the share tool
- **toolchain**: verify views on re-run and before each identity capture
- **gate**: compile fixture-local sources under the explicit environment
- **gate**: compile fixture-local sources fresh, outside the cache
- **native**: serialize PlatformIO runs and refuse a stale backup
- **gate**: harden the phase runner and bound compiled fixture runs
- **reader**: keep italic overhang at exact-fit line ends
- **tests**: bind ZIP codec before cover evidence starts (#43)
- **tests**: bind ZIP codec before cover evidence starts
- **home**: persist Stage across power loss and coalesce quiet focus
- **dev-usb**: tail-return accepted bytes through the transmit re-arm
- **dev-usb**: keep transmitter re-arm outside the write stack frame
- **dev-usb**: re-arm queued transmit tails without destructive flushing
- **covers**: prove fixed structural rejections before scanning large books
- **home**: preserve Stage on wake and use quiet binary focus regions
- **covers**: qualify runtime diagnostic observations
- **firmware**: scope refresh timers to paint transactions
- **firmware**: restore protected RAM and stack budgets
- **home**: keep a Stage cut's text before a malformed byte
- **home**: cut Stage title lines to whole codepoints
- **tests**: bind the native probes' extracts, tools and stack files
- **tests**: bind each cover evidence report through one helper
- **firmware**: tie the producer's shelf slots to the Library shelf size
- **tests**: bind cover evidence inputs on both sides of each compile
- **tests**: refuse header tests and unreadable names in the route tripwire
- **tests**: bind cover evidence to its environment and compiled inputs
- **tests**: refuse links and replaced records in the patched SDK copy
- **tests**: read route includes with a lexer, in every branch
- **tests**: check native probe build trees before compiling
- **tests**: take route includes from the preprocessor
- **tests**: verify the SDK bytes behind cover evidence reports
- **tests**: let concurrent writers create the same owned directory
- **tests**: pin fail-fast sanitizer options for gate and patch runs
- **tests**: write no native evidence from an optimized interpreter
- **covers**: keep the cover job report's checks under python -O
- **covers**: own the cover tests' fixed-name outputs
- **tests**: refuse an optimized interpreter in the host gate
- **covers**: verify SDK bytes and own outputs in the decode budget probe
- **covers**: keep the native size probe's checks under python -O
- **covers**: verify SDK bytes and own outputs in the native size probe
- **covers**: charge every unbound stop and pace the last budgeted attempt
- **covers**: settle idle cover work after input and screen changes
- **covers**: poll between producer path walks and probe caches like the painter
- **home**: remember a producer's no-art record as no cover
- **home**: keep the Home cover identity check inside its guarded frame
- **covers**: charge no book for a stop before its source is bound
- **covers**: key every interrupted attempt so a changed version starts its own count
- **covers**: count interruptions per book and set aside a book that keeps being interrupted
- **covers**: correct planned-peak comments and test the remaining paths
- **covers**: bound the decoder read-ahead, pin card syncs, test resumes
- **covers**: read books sequentially, resume interrupted books and sync once
- **covers**: never record a card fault as a coverless book
- **covers**: honor pinned FAT metadata and publication contracts
- **tools**: compile the latency benchmark per revision and syntax-check it
- **home**: hold the Home cover for the session so focus never reads
- **reader**: discard the style profile of a chapter with repeated heads
- **cache**: bump engine revision and prove face parity on every route
### Refactor
- **update**: check at compile time that the page's texts fit whole
- **acceptance**: keep transport projection within module budget
- **display**: bind panel telemetry at the firmware boundary
- **settings**: name each Settings row's Select action in one switch
- **storage**: move the SD state record codec into its own header
- **tools**: keep the bench handshake beside the held port
- **clock**: share the RTC register read and UTC decode
- **reader**: move page position math onto ReaderPageInfo
- **cache**: return checked freshness as an explicit result
- **cache**: report path failures without a closure
- **cache**: separate page query descriptor policy
- **cache**: isolate retained descriptor entry checks
- **cache**: keep query timers to one transient pointer
- **cache**: classify interrupted hashing at the verifier boundary
- **cache**: keep incremental block state in a scalar
- **clock**: keep the measured clock frames and publish the zone in homeClock
- **reader**: bound nested timing without per-scope storage
- **library**: share focus layout and isolate scratch stack
- **library**: bound focus paint ownership and stack
- **reader**: keep trace formatting off producer stack
- **reader**: isolate prefetch target trace frame
- **reader**: retain the measured cache query body
- **reader**: keep producer policy in its bounded owner
- **home**: name the SD book fallback title once
- **home**: share the chapter progress rule between themes
- **reader**: let final save own post-open recovery outcome
- **reader**: let checkpoint owner preserve failed live saves
- **cache**: remove unreachable backup refusal
- separate exhaustive retry boundary cases
- retain admission cancellation in the idle snapshot
- group idle producer outcomes with admission policy
- keep producer outcome with the selection interface
- **reader**: stage metadata queries with their results
- **reader**: keep checked read scopes within stack budgets
- **reader**: share one CSS scanner core
- **reader**: model navigation projections explicitly and drop dead paths
- **tools**: let the reader own navigation and give the probe one shape
- **tools**: take EPUB probe verdicts from the shared firmware C++
- **covers**: share Reader admission's ZIP record rules with discovery
- **ui**: keep Stage on the packed decoder and draw UI text in one loop
- **tests**: assert the overhang rule directly and share plan painting
- **covers**: keep early-verdict documentation within the health bound
- **firmware**: keep paint transactions within stack budgets
- **firmware**: bound the Home paint transaction
- **home**: use the shared Stage atlas face type
- **covers**: give the resume record its own header and name the read-ahead fit
- **firmware**: share the card paths, shelf layout and Home cover eligibility
- **reader**: share XML local-name and arena-mark helpers
- **reader**: move semantic progress into its own unit
- **font**: require an explicit face on every drawGlyph call
### Perf
- **build**: leave out the RGB LED path and its FreeRTOS timer task (#152)
- **build**: leave out the RGB LED path and its FreeRTOS timer task
- **reader**: merge the faster Sleep
- **reader**: record exact sleep timing resource costs
- **reader**: batch source checks and paint sleep before saving
- **home**: inline the linked Stage backdrop accessor
- **firmware**: free the boot-only app store before the first book open
- **firmware**: free the app package store before Home
- **host**: size gate jobs from CPU affinity and cgroup quota
- **host**: size gate jobs from affinity and cgroup quota
- **stats**: check the unconfirmed save before the Home quiet test
- **stats**: keep the fold's stack near the Back/Sleep save path
- **gate**: shard slow tests, keep object reuse and own gate scratch for every user
- **test**: split reader runtime and revision variants
- **test**: shard cache I/O control builds
- **gate**: share host capacity across checkouts
- **test**: reuse idle fixtures and split producer page cases
- **test**: shard turn parity and reuse storage harness
- **reader**: save reading progress when the reader is idle
- **cache**: check page cache blocks instead of the whole file
- **cache**: merge main's device Settings
- **settings**: redraw only the zone list card on a move
- **cache**: reuse page hints and defer query pathname checks to exit
- **cache**: verify page blocks in a borrowed query envelope
- **reader**: inline the session source liveness probe
- **reader**: preserve warm turns across optional metadata
- **reader**: record reviewed consolidated save resources
- **reader**: record reviewed idle-save resource costs
- **reader**: coalesce periodic chapter metadata into quiet follow-up
- **reader**: admit periodic checkpoints only while reading is quiet
- **reader**: trace checkpoint phases and slow storage calls
- **reader**: show the cached first page before verifying the book
- **storage**: share transaction peer storage
- **storage**: reuse verified checkpoint peer reads
- **reader**: record reviewed book-open attribution flash cost
- **reader**: attribute the complete Library book-open action
- **reader**: build the next chapter's page cache before the reader gets there
- **library**: repaint only focus and text on same-shelf Library moves
- **library**: record reviewed navigation cost and regression evidence
- **library**: repaint focus without rereading shelf covers
- **reader**: prefetch the next chapter during reading idle
- **reader**: make chapter cache reads fast and bounded
- **reader**: batch full CRC scans without stack growth
- **reader**: bound and batch checked cache reads
- **reader**: finish chapter cache production on device
- **reader**: batch producer I/O and pause for foreground duties
- **reader**: bound page cache production and turn costs
- **gate**: cut the gate plus four firmware builds from 28 to 5 minutes
- **gate**: compile fresh objects before claiming their key
- **native**: build firmware variants in parallel lanes
- **gate**: reuse shared objects in fixture builds and controls
- **gate**: run independent gate phases in parallel
- **gate**: parallelize host compiles and hash shared inputs once
- **home**: read Home's cover in one pass and paint the slot in whole bytes
- **reader**: skip ordinary text in bulk in the style watch

## v0.1.0-alpha.0

### Feat
- **reader,home**: label a chapter-size progress estimate
- **reader**: show the opening screen before cold opens only
- **home**: draw Home as Overlay gray with a quiet cursor and one refresh policy
- **dev-usb**: purge derived reader caches to measure genuinely cold opens
- **reader**: port declared section metadata onto canonical runtime
### Fix
- **reader,sd**: keep a chosen sample when the booted card is pulled
- **reader,sd**: keep the refusal message through a failed open; review fixes
- **reader,sd**: compare again after a card change; review fixes
- **reader,sd**: compare a held book with every card until one names it
- **reader,sd**: restore saving after a cardless wake and keep a sample wake
- **reader,home**: close no-card gaps and extract card and estimate pieces
- **home,library**: say when the SD card is missing
- **reader,sd**: reopen a held book once after a card swap
- **reader**: keep the cold opening screen out of the 20 s open budget
- **dev-usb**: close the reader before the cache purge frees its session
- **home**: make a due cleanup on Overlay Home a real GC from any screen
- **home**: spend the device refresh budget only for real paints and cleanups
- **reader**: keep warm reopens when the receipt session fits the reserve
- **storage**: keep reading state on SD so NVS stops growing
- **sd**: open library sources unbuffered to restore reader admission
- **tests**: merge empty-read SD fixture correction
- **tests**: skip null copies for empty SD fixture reads
- **reader**: merge reviewed Books recovery and warm verifier
- **reader**: integrate warm verifier and Books recovery with reviewed main
- **ui**: merge reviewed generic Home title rendering
- **ui**: render dynamic Home titles with generic glyphs
- **reader**: keep Books browsing while preserving pending recovery
- **ui**: render all dynamic Home titles with generic glyphs
- reserve actual reader trace length and shorten authored FAT paths
- **tests**: integrate reviewed host portability repair for section validation
- **reader**: stage canonical section metadata before publication
- **tests**: preserve runtime fault probes across supported hosts
- **tests**: keep runtime fault probes portable on Linux
- **tests**: preserve runtime fault probes across supported hosts
- **reader**: integrate reviewed runtime and opening-cost corrections
- **reader**: reconcile published runtime with current portable gates
### Refactor
- **reader**: give required-member admission its own stack frame
- **tests**: extract shared SD probe prefix
- **reader**: keep the warm binding frame explicitly measurable
### Perf
- **reader**: validate required resources before opening and others before use
- **reader**: check SD paths once per action and lend action storage
- **tests**: merge fresh preview compilation sharing
- **tests**: retain shared SD probe prefix extraction
- **tests**: align preview pilot with current fixture correction
- **tests**: preserve cached preview feedback mode
- **tests**: reuse frozen preview builds within one run
- **reader**: batch verified warm-selection reads in borrowed storage
- **reader**: integrate receipt reuse and canonical section metadata
- **reader**: integrate receipt reuse and canonical section metadata
- **reader**: integrate fresh-digest selection reuse with canonical metadata
- reuse admitted reader receipts after a fresh full source digest
- **reader**: defer closed Home metadata during immediate reader opens
