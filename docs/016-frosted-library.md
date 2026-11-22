# Frosted Library implementation

Library is a four-cover, row-major browser over the existing immutable catalog.
It shares approved Home's canonical wordmark, Inter atlas, background and exact
four-button footer. This change owns presentation/navigation and optional cover
preparation only. Reader admission, source lifetime, progress and device/card
integration remain the runtime owner's responsibility.

Previous/Next visits each book in sorted catalog order, clamping at either end.
Crossing a four-book boundary changes the page. Back returns Home; entering Books
again preserves the highlighted catalog index. Open issues the existing explicit
intent; it never authorizes a source through cover metadata. Sleep retains the
existing Home-return contract. A black three-pixel frame, arrow and bold tile
label identify focus without color. The separate selected-title area wraps at
24 px, then the established 21 px size, with an explicit ellipsis if necessary.
Only the display suffix `.epub` is removed; the original filename stays intact.
The header says `43 of 61`, never multiselect wording.

| Physical input (existing firmware contract) | Library footer | Action |
| --- | --- | --- |
| Back / Home's Read button | Back | Return Home |
| Confirm | Open | Open highlighted book |
| Left/Up | Up | Previous book in row-major order |
| Right/Down | Down | Next book in row-major order |
| Power | No bottom label | Existing sleep intent |

Footer centers remain 95, 201, 331, 437; baseline 779, Inter R28. Home's footer
and all approved four-gray Home pixels remain identical. The shared background
now deduplicates immutable 8x8 tiles: 13,068 B map + 23,984 B dictionary replaces
104,544 B, saving 67,492 B. Sampling remains constant-time, with no runtime heap,
decoded cache, new artwork, quantization or framebuffer. Independent reviews
reconstructed every original byte; existing canonical PNG tests also pass.

Optional cover caches are fixed, fully regenerable decoration. One read-only
handle and one 18-byte packed row serve all four slots sequentially. No catalog
pointer or raster survives rendering. Null/released catalogs, missing covers,
wrong headers, corrupt CRC, short/trailing files, stat mismatches and late read
failures fall back to truthful placeholders; late failures clear all partial
artwork. Loading cannot open/move selection and remains Back/Sleep accessible.
The current synchronous scanner need not expose a separate loading paint; the
shared renderer contract and authored fixture exercise it.

| NLC2 field | Bytes | Encoding |
| --- | ---: | --- |
| Magic | 4 | ASCII NLC2 |
| Width, height | 4 | Little-endian u16: 144, 188 |
| Source book size | 8 | Little-endian u64 |
| Source modified time | 8 | UTC epoch seconds, rounded down to two seconds |
| Payload CRC32 | 4 | Little-endian u32, standard IEEE CRC32 |
| Packed raster | 3,384 | Row-major, MSB first, 1=black |
| Exact total | 3,412 | No trailing bytes or variable dimensions |

Runtime lookup is `/newt-sd/Newt/Covers/<admitted filename>.nlc`. Cheap size/time
binding detects ordinary stale caches; equal-size/equal-timestamp replacement
can retain old decoration. This is explicitly not a content identity: the
existing full-digest reader selection remains authoritative. Prepare from the
exact local/card-copy metadata that the runtime owner will integrate; validate
FAT timestamp behavior before relying on cover availability. No cover failure
changes catalog status, book bytes, reading position or admission limits.

The adapter validates CRC while streaming rows into the unpresented logical
frame. A failed final checksum wipes the entire slot before its placeholder is
drawn. Four covers read 13,648 cache bytes in 756 reads (one payload pass plus
headers), plus four source-stat calls. It allocates no image/ZIP arena or stdio buffer; filesystem
handle internals still belong to the existing bounded mount. Measured C3 static
frames: renderer 304 B, cover admission 448 B, row read 16 B. These are individual
compiler frames, not measured physical peak heap/stack or a latency guarantee.
See [navigation latency evidence](041-navigation-latency.md) for the measured
host improvements and unchanged physical acceptance gates.

Automatic host preparation uses existing macOS ImageIO, one book at a time:

```sh
python3 -B tools/prepare-library-covers.py --books /explicit/local/book-copies --output .build/library-covers
python3 -B tools/library-redesign-preview.py
python3 -B tools/check.py
```

Preparation supports unambiguous EPUB3 cover-image or legacy OPF cover metadata,
JPEG/PNG, bounded ZIP/XML/image input and read-only source files. It preserves
every book byte, creates thumbnails atomically, and removes only identifiable
obsolete NLC2 caches. Missing/unsupported artwork stays absent. No dependency
installation or upload is needed. Host-only error diffusion avoids a fixed
checkerboard pattern; firmware consumes the same fixed 1bpp rows. Private books
and their artwork/caches must stay out of Git. Only the runtime owner may write
a card. This task performed no hardware, USB, SD or private-book action.

`tools/library-redesign-preview.py` adds original CC0 cover art to local copies
of the authored reader fixture, automatically prepares all four thumbnails,
and renders through the actual production cover adapter. It also exports
no-cover/partial-failure, 43-of-61, final sparse page, long title, empty, error,
loading, Home, device, reading/menu/settings and comparison PNGs. Those images
prove shared pixels and fixture state, not optical/device usability.

All four clean native resource variants fit the existing ceilings, with static
RAM and RTC bytes unchanged from the reviewed base. The app-image reduction is
65,536 B for default/regional and 65,552 B for development variants. No baseline,
reader memory reserve, catalog capacity or admission policy was relaxed.

Standalone correctness and visual/maintainability reviews were clean in round
one. Runtime integration must preserve nullable catalogs outside browsing and
the owner's catalog release before opening. Real-card thumbnail preparation,
physical focus/latency/ghosting, reading, return and resume remain the runtime
owner's acceptance gates. The implementation-preview Library upload route was
unavailable; local PNGs remain the authoritative deliverables until saved.
