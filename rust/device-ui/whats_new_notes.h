#pragma once

// What's new on About: the owner's update notes, newest first, in plain
// words. Edit before each owner update (docs/RELEASING.md). Keep the last few
// updates; drop the oldest when About no longer fits them. rust/device-ui/build.rs
// reads this file (one to six changes per date); tools/release_publish.py too.
// Keep each quoted date/change on its own line.
namespace newt {
inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Nov 22, 2026",
     {"Newt's first public release. Welcome!",
      "Copy EPUB books anywhere on the card.",
      "Each book keeps its page, even through sleep.",
      "Updates come over Wi-Fi or from the card.",
      "Your other firmware stays. Settings can restart into it."}},
};
}  // namespace newt
