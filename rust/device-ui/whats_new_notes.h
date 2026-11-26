#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 26, 2026",
     {"Mark as finished works right after you open a book.",
      "Switching to landscape and back keeps your page.",
      "Waking no longer jumps back to an older page.",
      "4-digit page numbers no longer get cut off.",
      "Choose which holidays Newt dresses up for in Sleep screen settings.",
      "Various smaller enhancements."}},
};
}  // namespace newt
