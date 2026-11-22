> Historical first-spike decisions. The separate reader experiment is recorded in [Decision 002](002-bounded-offline-reader.md); the hardware baseline remains `44c454d`.

# X3 as Newt's proving ground

Accepted direction: 2026-10-01. Source brief: the owner's explicit X3 pivot.

Newt will make small apps approachable through **describe an idea → agent creates it → preview/test → install → share**. The agent deals with internal app plumbing. The platform owns presentation and semantic actions. The constrained X3 is the first real device, while the broader interpersonal software vision remains useful research.

## Decisions that affect code now

- Consume official FreeInk as a pinned SDK submodule, not a reader firmware fork. The first target is X3 only, including batch-specific panel selection through the pinned SDK's source implementation.
- Start one compiled, hardcoded offline reading nook. A shared C++ renderer/state runs on both the host and ESP32. This is a screen/state boundary, not a generalized app runtime or package sandbox.
- Platform owns the small drawing/text controls; app state receives only `Previous`, `Next`, `Activate`, `Back`, and `Sleep`. No guest HTML, scripts, arbitrary machine control, manifests or permissions syntax in the human experience.
- Implement monochrome first. A 528×792 portrait packed frame maps to the 792×528 native panel buffer. Framebuffer cost is explicit; grayscale and font-engine changes must justify their memory.
- No network payload yet. Offline behavior, working buttons, truthful telemetry and safe sleep are the first hardware gate. A bounded optional network resource can follow those measurements.
- No SD writes, file importer, package installer or OTA updater in this build. That makes it inappropriate for USB-locked units. Recovery must be established outside the app before any first flash.
- Reading is a core product promise. The sample reader is only a hardware proof: EPUB, real book positions, fonts and persisted reading stats remain required work. Official FreeInkBook is the first engine to evaluate, rather than maintaining a new EPUB implementation.

## Keep the earlier work

The earlier checkout remains `~/Documents/Codex/2026-09-30/task-3/wayfinder`, accepted Yjs checkpoint `a6ca7ad14088290cc3d7e6c4ea1ade933ab7e85d`. Its separate preservation worker completed a clean checkpoint `d6f9f824d60a9ce809b83f61651d5f08910d35c4` and [reuse handoff](../../../task-3/wayfinder/docs/research/freeink-reuse-handoff.md). This spike did not edit it or operate its services/previews (4173/4180). The preservation handoff reports no current preview listeners; it does not authorize restarts. The owner cancelled the old Screen Sharing test.

Reusable **concepts**: platform-owned controls; semantic actions; app/signer/agent authority separation; recipient-owned permissions; immutable package/version lineage; offline local state; deterministic build/test evidence. Reusable **engineering patterns**: pure state transitions, trusted render contracts, action tests and source/binary provenance. No old source was copied into this spike.

Old browser shell, Wasmtime guest execution, desktop broker/storage, Yjs shared checklist, signing and Linux viewer tooling are **R&D evidence**, not ESP32 components. Their APIs and tests may inform later contracts, but their runtime/heap/storage assumptions do not fit an ESP32-C3 merely because the concepts are useful. Do not discard or rewrite that work.

## Daily-reader typography and stats

The owner specifically likes CrossInk's default reader font and reading stats. Current-source CrossInk defaults to **Lexend Deca at 14pt**, with Bitter alternate serif and Inter UI; the owner's installed version/settings are not confirmed. Do not equate current upstream default with his exact configuration. Pin and inspect font files/OFL license before reuse. The initial original compact bitmap font is temporary hardware-proof typography, not the final reader font.

Reading stats must follow actual active reading and saved book position, behave offline, survive reboot/power loss, and avoid counting the sleep screen or menus as reading. Current CrossInk stats research reports forward page turns with ≥2s dwell, time intervals ≤300s idle cutoff, sessions ≥60s and reader-exit time ≥10s. Portable MIT stats/date/serialization modules are candidates; activity hooks need adaptation. Verify exact revision before code reuse, valid RTC/timezone, crash-safe Newt storage and compatibility semantics. This proof does not claim those stats exist. Font OFL notices are separate from the reader repository's MIT license; generated fallback sets may combine multiple fonts.

## Normal app installation is over Wi-Fi

The owner's later product clarification is authoritative: apps should follow **describe → generate `.newt` → validate/preview/package → queue install → X3 wakes/syncs → downloads/verifies/atomically installs to SD**. SD is device storage, not an agent mount interface. A future Newt Hub/CLI should let an agent say `newt install weather.newt --device x3-dev` without knowing an SD mount point. Installation UI should explain real authority in plain language, such as “Uses current book + time. No network. Can change sleep screen.” A requested capability is not a grant.

The same package may eventually travel as a file, private share, friend share or community listing. Local/shared state, HTTP data and notifications are future bounded resources. This does not specify a runtime, signing architecture, mandatory cloud, package schema or persistent Hub now. Implement the smallest queued install experiment after the hardcoded app/hardware proof exposes requirements. Preview mockups may explore a fresh aesthetic in parallel; do not inherit the old Newt theme. Existing pixel typography here is functional evidence, not a settled visual identity.

## Sharing and deployment have separate owners

The owner's subsequent architecture clarification separates four concepts:

| Concept | Responsibility |
| --- | --- |
| Firmware | X3 execution, trusted rendering, capability enforcement and device sync |
| Portable `.newt` package | App content that agents create and people share; independent of a particular device/control plane |
| Registry | Package hosting/discovery, version history and universal share links |
| Control plane | Desired state for a person's devices and queued deployment; hosted or optionally self-hosted |

Sharing a package does not deploy it to a device. The intended loop is agent creates app → instant unlisted universal link (no public listing) → optional `newt publish` adds a Registry version → recipient opens preview/“Send to my Newt” → **their** control plane queues → **their** device syncs. Self-hosting must retain package/link interoperability. An unlisted bearer link is not confidential/access-controlled sharing; those guarantees need separate authorization and implementation.

On-device **Apps → app → Share → QR** points at that universal app/share URL. A phone performs the recipient's installation; the X3 does not need a messaging system. An agent may fork/remix (“remove weather, show my streak”), validate and deploy without making the person author a manifest.

These are ownership boundaries for the next local package/queued-install fixture, not permission to build or deploy a production Registry/control plane now. No public hosting, credentials, auth/service setup or publication follows automatically. Keep package identity separate from device desired state in future code. This spike remains one hardcoded app with actual firmware compilation verified and the physical hardware gate pending.

## Next gates

The real C3 target now compiles using the approved local toolchain. Prove backup/recovery for this exact cracked X3, then test display orientation/controller, buttons and repeated sleep/wake with battery telemetry. Add FreeInkBook and the preferred font after measured memory and reliable position persistence. Externalize one app's data/components only after a second useful app makes the contract obvious.
