# Newt install page

A static page that installs Newt, or another X3 firmware, into the X3's spare
app slot over Web Serial. No backend, no tracking, no analytics. Design and
safety rules: [docs/092-install-page.md](../docs/092-install-page.md).

Everything stays in this folder: `node_modules/` here, the pnpm store under
the worktree's `.build/pnpm-store` (`pnpm-workspace.yaml`). Versions are exact
and locked (`pnpm-lock.yaml`).

## Set up

```sh
cd installer
pnpm install --frozen-lockfile
```

## Test

```sh
pnpm test          # vitest: slot rule, table, images, the state machine on a fake X3
pnpm typecheck     # tsc --noEmit
```

The gate runs both through `tests/installer_test.py`, which also checks that
`test/x3-slot-vectors.json` still matches `tools/x3_slots.py` and, with
`--negative-controls`, breaks the slot rule in a scratch copy and expects
vitest to fail. After a reviewed change to the Python rule or its vectors:

```sh
python3 tests/installer_test.py --write
```

## Build the page

The page serves its own `newt.bin`, so a build needs a Newt release folder
holding `newt.bin`, `newt.manifest` and the firmware's `NOTICES.txt` (as
`tools/update_publish.py` writes them). The build refuses a pair whose size,
SHA-256 or identity disagree.

```sh
NEWT_RELEASE=/path/to/release/v0.1.0-alpha.11 pnpm build   # writes dist/
```

`dist/` is the whole site: `index.html`, `assets/`, `firmware/newt.bin`,
`firmware/newt.manifest`, `firmware/NOTICES.txt` and the page's `NOTICES.txt`.
A browser cannot download GitHub Release assets (no CORS header), so the site
carries its own copy. `.github/workflows/install-page.yml` builds it the same
way from each published release and deploys it to the public repository's
GitHub Pages (docs/RELEASING.md, "Install page").

## Try the site locally

The same site Pages serves, from any release folder: one the release tools
wrote (`.build/release/vVERSION`), or the latest published release's three
files (`gh release download --repo VargasDevelopment/newt-releases --dir DIR
--pattern newt.bin --pattern newt.manifest --pattern NOTICES.txt`).

```sh
NEWT_RELEASE=/path/to/release pnpm build
pnpm preview       # http://127.0.0.1:4173 (Web Serial needs a secure context; 127.0.0.1 counts)
```

Stop it when done. Never bind another address.

## Every screen

```sh
pnpm gallery    # writes dist-gallery/gallery.html; open it from disk
```

`gallery.html` lists every screen from fixtures; `gallery.html?state=consent`
shows one.

## The wordmark

`src/newt-wordmark.png` is cut from the approved brand source, never edited
by hand (provenance: docs/092-install-page.md, "The wordmark"):

```sh
python3 tools/make-install-wordmark.py           # regenerate
python3 tools/make-install-wordmark.py --check   # the gate's check
```

## Device checks (by hand)

Web Serial asks a person to pick the port, so B1-B3 in
docs/092-install-page.md are owner hands-on checks, with the X3 on USB and
the page served as above.
