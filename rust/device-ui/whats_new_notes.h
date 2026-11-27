#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 27, 2026",
     {"Skipped pictures now show a note and alt text.",
      "Small caps and text styles now show in books.",
      "Long books with over 1,000 files now open and show covers.",
      "Faster page turns work with any progress bar or refresh.",
      "Oversized or shifted words no longer clip.",
      "Various smaller enhancements."}},
};
}  // namespace newt
