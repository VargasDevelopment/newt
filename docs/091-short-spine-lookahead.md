# Short-spine lookahead (#231)

The flash cost is recorded in flash record 102.
The implementation uses book structure and the current layout only.

After the current chapter's complete cache is confirmed, idle preparation can
prepare up to four following spines. A successful preparation of zero to three
pages advances to the next spine; a longer chapter, EOF, the fourth spine or a
terminal failure ends lookahead for that displayed chapter. Thus three short
spines can precede the first segment of a long fourth spine. Four short spines
exhaust the bound: the fifth waits for a later displayed chapter. Malformed
content is a failure, not permission to skip ahead. An empty spine must finish
its source validation before advancing, and writes no empty cache file.

Each target gets a separate ordinary admission and cancellable synchronous
attempt. The current chapter is rechecked first. Crossing into another chapter
resets the view, so its missing segments have priority. The existing 120-second
attempt deadline, three interruptions per displayed chapter, quota, entry and
segment accounting remain. Successful short targets spend no interruption or
future chapter production budget. One fully prepared normal next chapter ends
work exactly as before; there are no further source parses or card writes.

Two bits of the existing 16-bit view hold the additional distance. The ten
remaining spine bits cover all 1024 supported spines; the reset sentinel is
checked explicitly. No retained owner grows. Cached short chapters are recognized
through the SDK's validated index and existing checked page port, using the lent
parse workspace. No index survives the call. This is idle work, outside page
show and foreground turn frames. DEV `PC target: from=N next=D` now reports the
distance D (zero is current work, one through four is lookahead).

## Bench baseline

DEV USB power, alpha.10 `4e08cc723c5a`, crowded card, 65 library entries. Private
content was neither captured nor copied. The final controlled run used two
25-second page intervals before a one-page spine, then a 1.000-second wait on
that short spine. Firmware action times include display work:

| Action | Alpha.10 | Route |
| --- | ---: | --- |
| Last ordinary cached page before the boundary | 0.726258 s | cache hit |
| Onto the short spine | 0.967589 s | cache hit |
| Off the short spine into the long chapter | 3.711421 s | ordinary parse |

The last turn spent 2.471662 s in chapter layout. It reproduced the owner's
reported pattern before any firmware source changed. The session reading heap
minimum was 63,164 B and loop stack minimum was 6,412 B. Earlier control-book
observations recorded a 2.870676 s cached open, a 0.794058 s cached page turn,
a 1.325058 s cached chapter crossing and a 0.696736 s following page turn.
Those sessions' lowest heap and stack were 63,604 B and 6,284 B.

The controlled run and earlier exploratory sessions are distinguished in
`/var/tmp/newt231-astra/evidence/`. Exploratory runs with stale USB sessions or
an unsuccessful reader-setting save are excluded. The final run renewed the
handshake after each 25-second wait and left the reader on Home; the lease was
released. No sleep or cache-purge action was issued. The original reader setting
remained in use.

## Candidate and resource evidence

Candidate `144c6f6d4c72`, DEV USB power and the same crowded card. The matched
short-spine run used the same two preceding page positions, two 25-second waits,
and a 1.000-second short-page wait. While spine 7 was displayed, the idle target
row reported distance 2; the producer completed 104 pages in 11.784 seconds,
with 416 writes totaling 120,868 B. The prior one-page spine was already cached.
The long chapter's first page then took 1.098819 s, route 3 (cache hit), versus
3.711421 s, route 4 (ordinary parse), on alpha.10: 2.612602 s / 70.4% less.
The immediately preceding ordinary cached page took 0.685965 s versus 0.726258 s.

The turn onto the short spine took 1.255757 s versus 0.967589 s. Its display
refresh was full (855 ms) in this candidate sequence versus partial (550 ms)
in the baseline. The candidate run's reading heap low remained 63,164 B;
its loop-stack low was 6,332 B versus 6,412 B in the short-spine baseline.
Across both books the earlier baseline stack low was 6,284 B. These measured
low-water marks are session evidence, not an inferred call-chain bound.

The initial normal-book control opened at a different saved position and took
2.661156 s versus 2.870676 s. Its same last-page, chapter-boundary and following
page samples were 0.806440 / 1.445615 / 0.840583 s versus the earlier
0.794058 / 1.325058 / 0.696736 s. Those initial opens are not a matched-position comparison. The six-cycle
normal-book repeat and its matched alpha.10 control are reported separately in
the handoff evidence (`normal-comparison.json`) and PR summary; the early
samples above are retained rather than overwritten.
The candidate's normal-book session held heap/stack lows of 63,616 / 6,412 B
versus the earlier baseline's 63,604 / 6,284 B. It prepared only the immediate
next chapter (28 pages, 36,561 write bytes), with no distance-two target or
extra lookahead writes. The authored normal fixture separately verifies zero
additional files, bytes or source work after the ordinary next chapter.

The clean measured image grows from 3,274,000 to 3,274,768 B (+768 B).
RAM remains 125,312 B, RTC 300 B, and lent Wi-Fi storage 22,528 B.
Every protected stack measurement is unchanged; no floor or other ceiling
moves. The one exact flash record cites `artifacts/newt231-short-spine-cost.json`
and records the completed cross-family review. The unselected idle
`produceNext` frame grows 80 to 112 B, admission 256 to 272 B, and the session
production wrapper 64 to 80 B. The new cached-length helper (`shortStored`) is 112 B,
`completeProducerTarget` 16 B, and `releaseForAdmission` 16 B. These frames are
outside `show()` and turn work; there is no new retained state or unshown Pages
descriptor.

The expanded actual idle/selection/SD fixture covers a single short spine,
three-short chains, blank spines, EOF, short-before-short, a normal sequence,
the hard four-spine limit, crowded storage, mid-write errors, press/Back/sleep
cancellation, changed layout/source, and revisiting an already cached short
spine with its following chapter absent. Removal controls exercise each added
case and independently remove the length guard, hard bound, empty handling,
cached-short recognition and descriptor release. Existing segmented production,
quota, retry-ledger, pause and foreground cache-route controls remain.

The clean candidate push gate and resource adoption against `origin/main`
passed. The push run took 1,604.525 seconds on a loaded shared host; its wall-time
overrun was reported by the existing busy-host policy. The first gate attempt
used the system Python and failed the runtime-alias provenance test; rerunning
with the approved project Python passed without changing that test. A separate
unchanged alpha.10 quick run completed producer assertions but exceeded its
360-second CPU limit (373.881 s); the isolated rerun passed at 216.022 CPU s.
The alpha.10 UI development gate and clean build also passed. Both outcomes
are retained; no test, budget or gate policy was relaxed.

Final-head full gate, release/merge checks and exact receipt are recorded in
`/var/tmp/newt231-astra/evidence/` at handoff. Candidate and control numeric logs
are separate from the exploratory runs. Battery sleep/wake and cold power-on
are outside this DEV-USB bench measurement.
