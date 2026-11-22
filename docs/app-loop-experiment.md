# One app, from an idea to a fixture device

2026-10-01. Separate branch `spike/agent-app-loop`, based on reader checkpoint `4dd4252`. Both that reader image and the minimal first-hardware baseline `44c454d` remain preserved. This experiment adds no hardware installation path.

This note describes preserved checkpoint `66015de`. The later [firmware-loader experiment](firmware-app-loader.md) selects the app and connects the same decoder to private device storage; host queue/sync remain simulation.

Ask an agent: **“Make me a little app with a quiet reminder, a way back to my book, and device information.”** The agent-created Pocket pause example does that. People see the app and its preview; the agent owns the small source file and package details.

## Agent commands

```sh
python3 tools/newt.py build apps/pocket-pause.json
python3 tools/newt.py preview artifacts/pocket-pause.newt --actions activate
python3 tools/newt.py preview artifacts/pocket-pause.newt --actions next,activate
python3 tools/newt.py queue artifacts/pocket-pause.newt --device x3-dev
python3 tools/newt.py status --device x3-dev
python3 tools/newt.py sync --device x3-dev
```

`build` validates the source, compiles the actual trusted C++ decoder/renderer, runs package and platform tests, then produces an exact monochrome preview and portable `.newt` artifact. `validate`, `package` and `preview` also work separately. Use `--output` for a chosen artifact basename. The device commands return `"simulation": true`; they never enumerate or touch hardware and take no SD mount path.

The source lives under `apps/` so an agent can edit it without inventing packaging infrastructure. The example has three cards: show a note, request continued reading, and request device information. The note is a working local screen; reading/device actions currently emit host semantic intents. They do not open a real book or query a device in this host app. The existing reader firmware remains a separate candidate. No arbitrary app drawing, code, paths, font assets or network requests are accepted.

`python3 tools/check.py` runs the complete repository gate, including the package, renderer, interoperability, delivery and reader tests. `python3 tools/firmware-build.py --clean` still builds the real C3 target with the approved local tools. Tiny-app source translates for C3, but the firmware Home does not select it, load packages or sync a queue. Compiling it does not turn the host installation fixture into on-device installation.

## The smallest portable format

One fixed card component, one note, and three known actions earn a small data format. The prototype package is exactly **615 bytes**, with canonical zero-padded ASCII fields and a fixed three-card capacity. Its content includes app ID/version/title/subtitle/note and 1–3 cards. Source is limited to 8 KiB; title/card title to 32 characters, subtitle/detail to 64 and note to 160. Numeric versions are canonical `x.y.z`. Unsupported fields, actions, duplicate keys, control characters, noncanonical padding and oversized files are rejected by the host; C++ decoding follows the same wire contract.

This is an experimental representation for one bounded app shape, not a general SDK, permission taxonomy or stable format commitment. It avoids adding a JSON parser or filesystem extractor to the MCU. Python and C++ agreement tests protect the current boundary. Font glyphs remain provisional; some printable symbols use the existing `?` fallback, and Unicode is not yet supported.

Accepted visible text never loses a long-token suffix: the platform's app-text helper uses character splitting and prefers a word boundary only when the remaining text can still fit. Fixed slots accommodate the declared worst-case glyph width. Tests distinguish maximum-length suffixes in each visible field, alongside bounds/determinism. The app owns data; the platform owns layout and visual consistency. Final aesthetics remain open.

## Ownership and atomic delivery

The file-backed fixture has three small owners:

| Owner | Responsibility |
| --- | --- |
| Artifact store | Exact verified package bytes addressed by SHA-256; no recipient intent |
| Control plane | Desired revision/digest and acknowledgement per device alias |
| Fixture device storage | One activated record containing revision, digest and verified package bytes |

Producing or copying a package does not queue it. Adding an artifact to the store also has no device side effect. `queue` is the explicit recipient action that records intent. The same artifact can be queued for independent recipients without changing its bytes or identity. A desired revision orders one device's intents; it is separate from the app's version. Intent can intentionally choose an older app version using a newer desired revision.

Delivery fetches bounded bytes through the artifact interface, checks their canonical form and expected digest, stages one combined installed record, verifies the staged bytes and rechecks the latest desired state before replacing it. Receipt and content commit together. A failure before replacement preserves the old installation; a restart after replacement but before acknowledgement verifies the new record and acknowledges it without reinstalling. Corrupt/missing artifacts, corrupt staged bytes, superseded intent, revision rollback and equal-revision/different-content conflicts do not replace valid installed state.

The transport is local files hidden under tooling's `.build/app-loop/`, with strict fixture aliases such as `x3-dev`. Tests use temporary roots through the same interfaces. This is **serialized, single-writer simulation**, not a concurrent Hub, Registry, Wi-Fi client or SD installer. Atomic replacement and host process-restart evidence do not prove filesystem behavior under machine power loss or X3 storage durability. There is no signing/authentication promise or universal URL service here. SHA-256 identifies bytes; it does not grant recipient authority.

## Evidence and next useful step

See `artifacts/app-loop-verification.json` for exact package/preview hashes, C3 source/object limits, host tests and the preserved firmware receipts. The data model is **607 bytes** and interaction state **8 bytes** on host and C3; rendering uses the existing 52,272-byte frame. Decoder/dispatcher/renderer contain no heap allocation calls. Standalone C3 `-Os` stack reports are static compiler estimates, not measured runtime peaks.

Failure tests cover download/validation/staging/before-commit/after-commit interruption, restart, acknowledgement loss, corruption, supersession, recipient isolation, identical requeue, revision conflicts and bounded files. End-to-end tests build the example, copy/share its bytes without queuing, explicitly queue/sync it, reopen installed bytes and reproduce the exact C++ preview. This proves a small agent loop today without requiring hardware or a visual-theme decision.

Next, after the actual X3's recovery/display/buttons/sleep/reader gates pass, connect this same bounded package decoder to device-owned storage and one wake/sync experiment. The physical storage and Wi-Fi transport must earn their own failure tests. Production Registry/control-plane deployment, accounts, signing and general application architecture remain outside this slice.
