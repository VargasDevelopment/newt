<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="newt-wordmark-dark.png">
    <img src="newt-wordmark-light.png" alt="Newt" width="314">
  </picture>
</p>

# Install Newt on your X3

Newt is firmware for the Xteink X3. It installs into the X3's spare slot, so
the firmware you run now stays on the X3 and you can go back to it (see
[Going back](#going-back)). Your books and card stay as they are.

You can install it from the stock Xteink firmware, CrossPoint, CrossInk or
another, on an X3 with its usual storage layout (see
[If the page refuses](#if-the-page-refuses)).

**Which X3.** Newer X3 units with the UC8279d screen controller (sold from
about July 2026) are tested. Earlier units with the UC8253 controller are
untested. If you have one, please
[tell us](https://github.com/VargasDevelopment/newt/issues) what works and
what doesn't.

## With the install page

You need a computer with Chrome or Edge, and a USB cable that carries data.

1. Open the install page:
   [vargasdevelopment.github.io/newt](https://vargasdevelopment.github.io/newt/).
2. Connect your X3 and wake it. Click **Install Newt**.
3. Pick your X3 in the list. It shows as "USB JTAG/serial debug unit".
4. The page checks your X3 and shows what stays and what it replaces. Click
   **Install**.
5. Keep the cable on until Newt's Home appears.

Safari, Firefox and phones can't use the page. On the stock firmware, use
Chrome or Edge on a computer. On CrossInk you can also install from the
card (untested from CrossPoint). If something stops, the page says whether
your X3 changed.

**On Linux**, if Chrome sees your X3 but can't open it, add yourself to the
group that owns the serial port (usually `dialout`, or `uucp` on Arch), then
log out and back in.

**To report a problem**, open the page as
[vargasdevelopment.github.io/newt/?debug](https://vargasdevelopment.github.io/newt/?debug).
It adds a **Save debug log** button. Put the error message in your issue
and keep the log in case we ask for it. Don't post the log publicly: it
holds your X3's hardware address.

## From the card

If your X3 runs CrossInk or CrossPoint:

1. Download `newt.bin` from the
   [latest release](https://github.com/VargasDevelopment/newt/releases/latest).
2. Copy it to your X3's card.
3. On the X3, open Settings, then System, then SD Card Firmware Update, and
   pick `newt.bin`.

This is tested from CrossInk and untested from CrossPoint. If you try it
from CrossPoint, please tell us how it went. On the stock firmware, use the
install page.

Copy EPUB books anywhere on the card.

## Updates

- **Over Wi-Fi:** save your network in Settings, Wi-Fi. Then open Settings,
  Update, and pick Check GitHub for updates. Newt downloads the update to the
  card, then offers it to install.
- **From the card:** copy `newt.bin` and `newt.manifest` from the latest
  release to the `Newt/Update` folder, which Newt creates on the card, then
  open Settings, Update.

Your books, reading progress and settings stay as they are. Reading never
needs Wi-Fi.

## Going back

Your old firmware stays in the spare slot until Newt's first update. An
update installs into that slot, so it replaces your old firmware (Newt asks
first). To go back after that, you need your old firmware's `.bin` file, so
get it before you update.

- **Restart into your other firmware** (while it is still installed): Settings, Restart into (its name).
  Newt stays in its own slot. If that firmware updates itself, it replaces
  Newt. Install Newt again the same way.
- **Install another firmware:** on the install page, click **Install another
  firmware** and pick its `.bin` file. Or copy the file to the `Newt/Update`
  folder, which Newt creates on the card, open Settings, Update, and
  pick it. It replaces the spare; Newt stays in its own slot.
- **Come back to Newt** from another firmware the same way you installed it.

## If the page refuses

- **"This X3 is set up differently":** its storage layout is not the X3's
  usual one, which some other installers write. Installing from the card is
  the other way in, but it is untested on that layout: nobody has tried it
  yet. If you do, please tell us how it went.
- **"Your X3 is in the middle of a change":** restart it, wait for its home
  screen, then try again.

Something else wrong? [Open an issue](https://github.com/VargasDevelopment/newt/issues).
