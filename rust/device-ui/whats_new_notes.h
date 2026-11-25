#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 25, 2026",
     {"Choose your text size.",
      "Landscape works both ways.",
      "Covers show when they're being made, or why a book has none.",
      "Reader settings survive a power-off.",
      "After an update, Newt still wakes straight to your page.",
      "Newt dresses up for Thanksgiving."}},
    {"Nov 24, 2026",
     {"Book covers appear faster.",
      "Various smaller enhancements."}},
};
}  // namespace newt
