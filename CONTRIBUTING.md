# Contributing to Newt

Thanks for your interest in Newt!

Newt is an early-stage, independently developed e-reader firmware project.
It's built around a simple goal: create an excellent, offline-first reading
experience while laying the foundation for a platform where people can
eventually create and share small applications.

For the foreseeable future, most core development will be led by the project
owner. This is intentional. Newt's architecture and product direction are
still taking shape, and keeping development focused helps maintain
consistency and move the project forward.

That doesn't mean contributions aren't welcome! Small, focused improvements
from interested contributors are appreciated.

## What contributions are welcome?

The best contributions solve a specific, demonstrable problem without
significantly expanding the project's maintenance surface.

Generally welcome:

- **Bug reports:** Especially reproducible issues with reading, rendering,
  navigation, performance, or device behavior.
- **Small bug fixes:** Focused changes that address an existing issue without
  introducing unrelated behavior.
- **Tests and reproductions:** Test cases that demonstrate real defects or
  strengthen existing regression coverage.
- **Documentation improvements:** Corrections, clearer instructions, or fixes
  to outdated information.
- **Small quality-of-life improvements:** Minor, well-scoped changes that fit
  the existing design. Please discuss these first if they change user-facing
  behavior.

If you're unsure whether something is appropriate, open an issue before
starting work. If the work is already agreed, for example in an existing
issue, you don't need a new one.

## Changes that require prior discussion

Please do not open unsolicited pull requests for:

- Major new features or substantial changes to existing behavior.
- Architectural refactors or new frameworks and dependencies.
- Support for additional devices or hardware platforms.
- Changes to firmware installation, updates, recovery, or other sensitive
  device operations.
- Changes to the application runtime, package format, permissions,
  distribution architecture, or other foundational platform interfaces.

These areas are being developed as part of Newt's longer-term roadmap. Even
well-intentioned changes can introduce constraints that are difficult to
unwind later.

Ideas and proposals are welcome as GitHub issues. Implementation should wait
until we've agreed that the change fits the project's direction. Lasting
product and architecture commitments need the owner's agreement.

## Pull request expectations

If you'd like to submit a fix:

1. **Keep it focused.** One problem per pull request. Avoid unrelated cleanup
   or refactoring.
2. **Understand your change.** Contributors are responsible for the behavior
   and maintainability of the code they submit.
3. **Follow the existing architecture.** Start with [AGENTS.md](AGENTS.md) and
   respect the ownership boundaries documented there.
4. **Provide verification.** Include focused tests, reproduction steps, and
   relevant measurements.
5. **Use [Conventional Commits](https://www.conventionalcommits.org/).** For
   example, `fix(reader): restore chapter after wake`. Your pull request's
   title becomes its merge commit's subject, so make it one too; CI checks it.

### Checks

The GitHub gate must pass before merge. It runs the host gate and the
clean firmware build with its resource checks on every pull request, so you
don't need to repeat them locally.

To open a pull request, run focused checks for the area you changed:

- Most changes: `python3 tools/check.py --suite quick`.
- Screens: `python3 tools/check.py --suite ui`.
- One test family: that family's tests.
- Firmware sources: also `python3 tools/firmware-build.py`, a quick compile.

On Linux, add `--profile linux` to `check.py`. These checks need the pinned
project toolchain described in the [README](README.md#build-from-source).
A change to only `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, the pull
request template, or Markdown and images directly in `docs/` (not
`docs/toolchain-*`), needs no local check: CI's
[docs lane](docs/CI.md#docs-lane) checks it.

Some evidence CI can't give. Add it where it applies:

- When it applies, run the [Mac ImageIO cover gate](docs/CI.md#explicit-cover-coverage-boundary).
- Device behavior: testing on a real X3 (below).
- Releases and merges: the owner's release acceptance and merge checks
  ([docs/RELEASING.md](docs/RELEASING.md)).

Tests of persistence and cold-boot restore, memory limits, recoverable flash
writes, security bounds and release integrity also need a negative control:
a change that breaks the code on purpose and proves the test notices. Other
tests don't need one.

If you change a golden output or reviewed frame on purpose, give the reason
and the evidence in the pull request. A separate commit is optional.

Clearly distinguish host tests, firmware builds, host previews, and testing on
physical hardware. None of these proves that the others succeeded.

For physical X3 testing, include the display controller (UC8253 or UC8279d)
and whether the device was powered by USB or battery.

## AI-assisted contributions

AI coding tools are welcome.

Newt itself is developed extensively with AI assistance, supported by
automated testing, architectural constraints, and reproducible verification.

The important distinction is accountability, not authorship method.

Whether code was written manually or with an agent, contributors are expected
to understand what they submit, verify its behavior, and respond to review
feedback.

An AI-generated implementation is not inherently unacceptable, and passing
automated checks does not automatically make a change appropriate for the
project.

## Reporting issues

Good bug reports include:

- What you did, what you expected, and what happened instead.
- Your Newt version and build, available under Settings → Diagnostics →
  About.
- Your X3 display controller, if known.
- Reproduction steps and relevant logs or screenshots.

Reports from earlier X3 units using the UC8253 controller are particularly
helpful.

Security vulnerabilities should be reported privately according to
[SECURITY.md](SECURITY.md), never through a public issue.

## Books and test fixtures

Never upload, attach, or identify copyrighted books in issues, pull requests,
logs, or test fixtures. Instead, describe the relevant EPUB structure, such as
chapter count, file size, image use, or formatting characteristics.

Newt provides the CC0 fixture `tests/fixtures/the-small-lantern.epub` for
testing.

Fixes must work for books generally, without special-casing individual books
by title, filename, digest, or other identity.

## Review and maintenance

Newt is currently maintained by one person, with limited time available for
external reviews.

Submitting a pull request does not guarantee that it will be reviewed or
merged. Changes may be declined because of scope, timing, maintenance cost, or
architectural direction, even when technically correct.

Please don't interpret that as a judgment of the quality of your work.

The project owner retains final authority over architecture, roadmap
priorities, merges, and releases.

As Newt matures, the contribution process may evolve to support broader
community participation.

## License

Newt is licensed under the [MIT License](LICENSE). By contributing, you agree
that your contribution is licensed under the same terms.

---

Thanks for helping make Newt better.

Whether you're reporting a bug, submitting a small fix, or simply using the
firmware and sharing your experience, your involvement is appreciated.
