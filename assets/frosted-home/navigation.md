# Home navigation

Morning Page was an unimplemented daily-overview placeholder sharing the
Pocket Pause route with Apps. The owner approved its removal on 2026-10-02.
Home keeps the current-book card and three named destinations: Books, Apps,
and Settings (named Your X3 until NEWT-32). Read continues the current book independently of menu focus.

The original approved source package, source manifest, and raster bundle remain
immutable historical design inputs. The Home resource generator derives the
current navigation from those inputs: three focus stamps, label baselines
474, 555, and 636, and no Morning Page or daily-overview supporting copy.
The existing card shell remains at (28, 417), 472 by 280 pixels, with radius
4 and its one-pixel gray-85 outline. Header, current-book card, and footer
pixels are unchanged from the approved fixture.

All cursor movement stays inside the existing portrait binary island
[48, 64) by [432, 672). Native regional activation and the nine quiet
moves followed by tenth-move cleanup are unchanged. The three navigation
rows route to the real book, bundled app, and Settings destinations. Settings
has no annotation: its title names the route.

The inspected current-layout snapshot is `navigation-settings-fixture.png`
(NEWT-32: the third row reads Settings, with no note). It differs from the
earlier `navigation-fixture.png` and `navigation-generic-title-fixture.png`
only in that row and the measured generic title lettering. Host tests
pin its SHA-256 and compare every fixture pixel, and independently compare the
unchanged card shell, header, book card, and footer against the frozen original.
Host preview and sanitizer checks verify composition and state transitions.
They do not establish physical contrast, ghosting, or device sleep acceptance.
