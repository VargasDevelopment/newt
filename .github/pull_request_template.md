## What and why

<!-- One change. What it does, and the problem it solves. -->

Closes #

## Checks

<!-- The GitHub gate is the required check: it runs the host gate and the clean firmware build, so don't repeat them locally. List the focused checks you ran (`check.py --suite quick`, `--suite ui`, a family's tests, or `firmware-build.py` for firmware). A change to only README.md, AGENTS.md, CONTRIBUTING.md, this template, or Markdown and images directly in docs/ (not docs/toolchain-*), takes the docs lane and needs none. -->

- Focused checks:
- [ ] The host ImageIO cover preparation path changed and the Mac cover gate (`tools/check-mac-covers.py`) passed on this commit
- [ ] A golden or reviewed frame changed on purpose: the reason and evidence are below
- [ ] Safeguard tests added or changed (persistence and cold-boot restore, memory limits, recoverable flash writes, security bounds, release integrity): they have a negative control

## Evidence

<!-- Host tests, the host preview, a firmware compile and a real X3 are separate facts. The gate covers the first three; say what else you have. -->

- On an X3: screen controller (UC8253 / UC8279d), power (USB / battery), what you checked
