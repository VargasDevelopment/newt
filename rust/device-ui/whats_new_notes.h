#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 24, 2026",
     {"Book covers appear faster.",
      "Various smaller enhancements."}},
    {"Nov 24, 2026",
     {"More books show a cover, from PNG and progressive JPEG too.",
      "Books with no cover get one more try.",
      "Chinese, Japanese and Korean books break lines in better places.",
      "Install apps from the card (experimental)."}},
};
}  // namespace newt
