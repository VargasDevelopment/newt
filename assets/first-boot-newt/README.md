# The first Newt

Keep this goofy little character exactly as it appeared on the owner's first
working Newt X3. It belongs in the polished firmware as an Easter egg, even
after the ordinary artwork changes.

[newt-mascot.cpp.txt](newt-mascot.cpp.txt) preserves the exact `newtMascot`
function from `src/render.cpp` at commit
`44c454df42d1ddaba566d18b3eaa1e88475ca45b`. Both its awake eyes and asleep eyes
are canonical. Its SHA-256 is
`ccdd7f2378258cdd05d4f5c1f5aadb8000acaee70d1401bc428addeb33de44c8`.

[render-44c454d.cpp.txt](render-44c454d.cpp.txt) is the complete, byte-for-byte
renderer from that commit. It preserves the original `Canvas::line`,
`Canvas::circle` and pixel behavior so a future redraw can reproduce the
actual artwork rather than just its coordinates. Its SHA-256 is
`1e8865c6616a7ff6d81fae1c3d246e19779d91dc2b723c7b4a9eed6737ab6bb9`.
The original Home call is `newtMascot(c, 328, 524, 1)`; the sleep call is
`newtMascot(c, 124, 310, 3, true)`, on a 528 × 792 portrait canvas.

This is original Newt project artwork, not an imported FreeInk/CrossInk asset.
Preservation does not introduce a separate license grant. The `.txt` source
archives are outside the firmware and host build inputs; they do not change
the deployed baseline or the current application.

In the firmware, five Select presses on About's Version row open it, and
Back returns to About (`rust/core/src/app.rs`). The page draws the original
awake mascot at its original place and scale (`rust/device-ui/src/first_newt.rs`).
There, Previous, Next, Previous, Next opens the hidden Developer page.
