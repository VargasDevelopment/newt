# Shared Newt UI canvas and Inter atlas

Safe `no_std` drawing canvas and immutable Inter font atlas shared by host
previews and the X3 firmware. Call sites lend one 52,272-byte logical frame
(portrait or landscape); the canvas holds only a borrowed reference, never a
second frame or heap allocation.

Integer geometry must stay within bounded signed arithmetic; host-only
paragraph accounting is omitted from target builds. Inter/OFL and Newt/MIT
notices are in `licenses/` and the repository `LICENSE`.
