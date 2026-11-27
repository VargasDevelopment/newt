#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 27, 2026",
     {"Pictures Newt can't show now leave a note in their place.",
      "Small caps and all-caps styles now show in books.",
      "Very large books that wouldn't open now open, with covers.",
      "Faster page turns now work with every progress bar and refresh setting.",
      "Large, raised or lowered text is no longer cut off.",
      "Other small fixes and improvements."}},
};
}  // namespace newt
