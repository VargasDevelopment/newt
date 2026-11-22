# Reader CSS fidelity and bounded admission

> Status: awaiting_device. Host behavior, native compile/resource measurements
> and physical X3 behavior are separate facts. Owner acceptance and reviewed
> flash adoption remain pending.

## R3 diagnosis

The `73c5ed0` baseline has two generic CSS fidelity failures.

1. The pinned SDK reduces a selector to its rightmost simple selector. The
   authored fixture `p{text-indent:0}p+p{text-indent:1.5em}` therefore applies
   the sibling rule as `p` and indents every paragraph, including the first.
   Mixed `h1+p`, `div>p`, descendant and general-sibling controls reproduce the
   same class of over-application. This is a generic mechanism; it does not
   establish the cause of a particular owner's observed paragraph.
2. Baseline rejects an external stylesheet above 8,192 bytes, and any resulting
   `StyleLimited` discards the chapter's other sources. A later 64-rule limit
   has the same all-or-nothing effect. Authored large-sheet controls show loss
   of early, middle and late indent/zero-indent declarations and embedded rules.

The SDK layout itself latches block `text-indent`, inherits it from the parent
style state, applies it only to the paragraph's first line, and marks buffer/page
continuations as continued. Heterogeneous host fixtures cover heading-adjacent
paragraphs, explicit and zero indentation, specificity overrides, legitimate
inheritance, nested inline styles, unrelated following paragraphs, wrapping,
page continuation, buffer boundaries, cache/reopen and both orientations. No
separate paragraph or continuation leakage was found. Both production reader
paths disable the SDK's independent embedded stylesheet path, so Newt owns the
external/embedded projection.

## Structural selector decision

The selected policy is **DROP**. Newt rejects every unsupported selector-list member
before the pinned SDK can reduce it. A supported member in a mixed list still
survives. The rule never applies where its full selector is unsupported.

The fidelity cost is explicit: `p{text-indent:0}p+p{text-indent:1.5em}` becomes
all flush-left. DROP cannot express adjacent-sibling, child, descendant or
general-sibling styling. It is a bounded safe subset, not full CSS fidelity.

| Option | Measured/prototyped cost | Fidelity and integration |
| --- | --- | --- |
| DROP (selected) | Zero per-depth match state, zero bytes in the retained slot and no relation matching during layout. Current whole-feature image growth is 8,976–8,992 bytes; shared build workspace peaks at 35,184 bytes within 49,152, with no retained-slot growth. | Structural selectors are omitted; simple supported members retain SDK cascade behavior. |
| IMPLEMENT `+` and `>` | Prototype lower bound: 364 RV32 text bytes; 1,156 bytes per-depth element/sibling state; 768 bytes for 64 relation records; 1,924 bytes total workspace; 0-byte match and 16-byte start/end static frames; 3.43 ns for one host start/match/end loop sample. | Excludes parsing, SDK integration, provenance patch, tests and whole-layout cost. A real implementation would require a separately reviewed FreeInk patch and representation. |

The IMPLEMENT numbers compare the two choices but do not describe a complete
candidate. No vendor source was edited and no FreeInk patch is needed for DROP.

## Policy 10 boundary

The layout-cache policy identity is `0x4e53540a`. The compatibility reference is
the direct pinned `CssStylesheetBuilder::addText` behavior used by `73c5ed0`,
followed by the same numeric guard and `display:none` neutralization.

A source is in the exact domain only when all of these hold:

- it is at most 8,192 bytes;
- it produces at most 64 SDK rules;
- every raw SDK selector range has no combinator, ID, attribute or pseudo
  context; and
- the canonical scan finds no protected boundary risk.

In that domain, rule count, order, selector hashes and every declaration exactly
match `73c5ed0`, including zero and empty declarations, UTF-8 selectors, safe
escaped names, BOM handling, malformed comma members, `!important` and the
pinned SDK's raw-hash quirks. NUL or more than six consecutive digits is the
same fatal numeric limit as baseline. `display:none` is still reported and
neutralized. Ordered source boundaries remain separate parses.

`src/reader_css_scan.h::Scanner` is the canonical and sole protected-payload
scanner. Fast-path eligibility and projected selector/list/declaration
boundaries consume the same token stream. It classifies code, comments, strings,
groups/functions (including unquoted `url()`), attributes and escapes across
chunk boundaries.

Any protected `{`, `}`, `;` or `,` is outside the exact domain, wherever it
appears. A selector comment after a token or after a comma is removed even when
it contains no element or delimiter. A leading top-level comment may remain
exact before a selector starts because the SDK performs the same skip. Escaped
structural punctuation, including escaped `.`, is projected and vetoed. Safe
escaped identifier bytes remain unchanged for the SDK's raw hashing.

Outside the exact domain, the same token stream projects complete CSS units.
Protected payload can never manufacture an SDK rule, regardless of the raw
context the SDK cursor would have inferred. Each selector-list member is
handled independently. Combinators (`+`, `>`, `~` and descendant whitespace),
IDs, attributes and pseudos are rejected. A supported `.safe` in `h1+p,.safe`
survives. `StyleSelectorRejected` reports any rejected member and
`StyleCombinatorRejected` additionally reports structural context.

### Per-published-unit projection invariant

Every selector and declaration passed to the SDK retains exactly its source's
significant CSS token sequence. Projection copies all retained non-comment
bytes unchanged. The canonical scanner emits token identities in addition to
protected classes; Admission saves exact token-start/continuation/trivia marks
(two bits per buffered byte), then scans each projected selector member and
whole declaration again and requires those marks to match. This is exact
boundary comparison, not a probabilistic hash. No separate production tokenizer
or rewrite grammar owns the decision.

Comments may disappear only when neighbouring tokens remain unchanged. Thus
`.safe/**/p` cannot become `.safep`: the two source ident tokens would join, so
the selector member is rejected with `StyleSelectorRejected`. The valid
`p/**/.safe` remains `p.safe`. The same rule catches number/dimension,
identifier, escape, hash, comment-start and CDO/CDC joins in declarations.
An unsafe declaration is dropped whole with `StyleTruncated|StyleCssSyntax`,
never truncated. Other unchanged declarations survive; an empty rule remains
until relevance compaction omits it. Unsafe selector members are rejected
individually; no surviving member means no rule. For example,
`p{font-family:"}{";text-indent:0}` retains zero indentation and reports the
removed font-family declaration. Protected raw braces or semicolons in a value
are also unsafe for the pinned SDK and cause whole-declaration removal.

These are named differences from `73c5ed0`, outside the exact domain: comments
cannot manufacture tokens, protected payload cannot manufacture SDK rules,
and declarations whose tokens the raw SDK cannot receive safely are omitted
with an issue instead of being masked into a different value. Unsupported
selectors, at-rule skipping and bounded relevance/truncation remain separate
intentional differences. In the exact domain the SDK reference still matches
exactly, including its parsing quirks and empty rules.

### Reproducible regressions

These authored controls run through the production ZIP, ChapterSource,
streaming admission and retained-slot route in stored and deflated forms. Each
also has a twin with unrelated structural context, proving eligibility cannot
depend on an SDK fragment discovered elsewhere in the source.

1. `.x/*,p,*/{text-indent:2em}` is exercised as a small source and as an
   8,226-byte large source. The selector comment is removed, `.x` remains, and
   no phantom `p` rule appears.
2. `p{font-family:"}.x{text-indent:2em}"}` drops the whole font-family declaration
   with `StyleTruncated|StyleCssSyntax` and keeps only the empty `p` rule. The
   protected string cannot manufacture `.x`. Large-mode relevance may omit an
   empty rule as described below.
3. `p{/*}.x{text-indent:2em}*/font-weight:bold}` keeps only the authored `p`
   bold declaration. The declaration comment cannot manufacture `.x`.

`p,/**/.x{text-indent:2em}` separately fixes the selector-comment policy: the
comment follows a comma, is removed, and both authored supported members remain.
Earlier adversarial cases also remain: declaration-comment fragments cannot
create rules, and a 600-member selector list retains exactly the newest
equal-priority members 536 through 599 in original order while reporting 536
capacity losses.

The independent test oracle compiles an unchanged copy of the pinned SDK source
in a separate translation unit with renamed public types. It exposes the SDK's
actual simple-selector parser and raw cursor, then independently classifies
selector context, protected boundaries and selector comments. Production does
not choose its own excluded corpus cases. No SDK source is patched.

## Bounded admission and retention

The implementation retains these fixed bounds:

| Resource | Bound |
| --- | ---: |
| Retained rules | 64 slots (1,792 bytes) |
| Complete source/projected rule buffer | 8,192 bytes |
| SDK batch scratch | 8,192 bytes |
| Projection identity marks | 2,048 bytes (two bits per buffered byte) |
| SDK batch members | 512 (independent of retained capacity) |
| Pending admission queue | 64 rules (1,792 bytes) |
| External read chunk | 256 bytes |
| Decoded work per external member | 262,144 bytes |
| Linked stylesheet members | 4 |
| Ordered style sources | 8 |
| Embedded style aggregate | 4,096 bytes |
| Entity-filtered head scan | 16,384 bytes |
| Body nesting below body | 14 |

The pending queue is arena workspace; it does not enlarge the retained slot.
Host shared-arena high water is **35,184 of 49,152 bytes**, leaving 13,968
bytes in that arena. The parse peak is **45,824 bytes**. The retained host slot
is unchanged at **1,816 bytes**, including 1,792 rule bytes. These are arena
measurements, not native static RAM or runtime call-chain peaks.

Large sources stream without allocation proportional to member size. One
projected rule must fit the 8,192-byte buffer. SDK batches hold at most 512 raw
selector members, independently of the 64 retained slots. Tests exercise the
pinned builder directly with 512 and 513 rules: both finish at 512. Raw-member
batching therefore cannot hide later members. An overlong or nested projected rule reports nonfatal
`StyleTruncated|StyleTextLimit`. A member beyond the 262,144-byte work cap is not
opened and reports `StyleTruncated|StyleResourceLimit`; other sources survive.
At-rules are skipped. Existing source-integrity and damaged-stream checks remain.

Rules are classified by declaration, independent of titles, filenames, digests,
selectors or known books:

- priority 2: explicit `text-indent`, including zero;
- priority 1: another declaration Newt applies;
- priority 0: empty or neutralized-only.

All rules, including priority 0, retain exact position while the small exact
source fits. Once compaction is required, admission merges the retained set and
one 64-rule pending batch. It compacts once per 64 candidates, caches the minimum
retained priority, keeps the latest rules within each admitted tier, then writes
them back in original order. This preserves SDK specificity/order among kept
rules and final inline precedence. More than 64 relevant rules can still lose
authored distinctions, reported as `StyleTruncated|StyleRuleLimit`.

Host-only counters in `tests/reader_css_work.h` enforce at most **8 retention
checks and 40 assigned rule bytes per decoded CSS byte**, over the complete
chapter and all four maximum-size links. A check counts one priority
classification or a tier/minimum comparison; the two tier comparisons are
counted conservatively even when a predicate short-circuits. Assigned bytes
include both enqueue copies and retained-slot assignments. These are logical
retention operations, not CPU instructions, cache traffic or decoder/SDK work.
C3 builds contain no counters or branches for this instrumentation.

The bound follows from collecting 64 candidates before selection, scanning at
most 128 candidates in each merge, and writing at most 64 retained rules.
Discard the oldest surplus in each of the two priority tiers, then copy kept
rules in source order. The quotas directly determine the cached minimum; a
lower-priority newcomer cannot displace it. A final partial batch, the first
compact-mode transition and the shortest 65-selector overflow are covered by
the bound and tests. Mode changes do not trigger repeated compaction, and an
empty final batch performs no work. No victim scan or per-eviction shift remains.
Normalization is linear; SDK batching operates on normalized syntax only.
SDK input is counted separately from retention: the long-declaration cost
regression enforces at most three SDK input bytes per decoded byte and the
exact SDK call counts below. A single bounded unit can amplify SDK input by
at most eight times its bytes: retained projected selector members are
nonempty, so each needs at least one byte plus its separator. At most 4,095
members fit an 8,192-byte unit, yielding eight batches, each no larger than
the original unit. The tighter three-times test bound applies to the authored
long-declaration workload, not every possible rule.

The same instrumented production mechanism was run before (`8722323`) and
after the change. Earlier counts of 8,059,968 victim comparisons and 225,679,104
shift bytes for the 128,000-rule case exclude priority classifications and
incoming writes. The table includes those operations consistently on both
sides. Every old case fails the new bound; all current cases pass.

| Input | Decoded bytes | Rules | Checks before → after | Assigned rule bytes before → after | Capacity losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| 32 lists of 4,000 selectors | 262,144 | 128,000 | 24,691,712 → 1,791,616 | 229,263,104 → 7,168,000 | 127,936 |
| Many tiny rules | 262,144 | 10,930 | 2,097,202 → 152,720 | 19,473,664 → 612,472 | 10,866 |
| All equal priority | 262,144 | 11,385 | 2,185,017 → 159,048 | 20,289,024 → 637,756 | 11,321 |
| Alternating priorities | 262,144 | 11,153 | 2,140,241 → 89,944 | 9,937,536 → 317,436 | 11,089 |
| Two 2,000-selector lists | 14,336 | 4,000 | 759,712 → 55,808 | 7,055,104 → 224,896 | 3,936 |
| Four production ZIP links at the cap | 1,048,576 | 512,000 | 98,803,904 → 7,167,616 | 917,396,480 → 28,672,000 | 511,936 |

The 65-selector minimum-overflow control also checks the final partial batch,
exact retained order, declarations and one reported loss. The 600-member
control checks complete admission beyond the SDK builder's 512-rule ceiling.

### SDK parse work

Host-only counters measure actual guarded SDK calls and their input bytes, in
addition to retention comparisons and copies. A 256-KiB sheet has 32 rules,
each with 2,048 selector members and a 4,095-byte declaration body. Changing
only SDK capacity from 64 to 512 keeps retained rules, order, declarations,
issues and 65,472 capacity losses identical. The regression fails on
`5a1c75a`'s 1,024 calls / 4,325,376 input bytes. The current checks also replay
four such sources and exercise four real ZIP links at the decoded cap.

| Workload | SDK calls before → after | SDK input bytes before → after |
| --- | ---: | ---: |
| One long-declaration source (262,144 decoded B) | 1,024 → 128 | 4,325,376 → 655,360 |
| Four identical sources (1,048,576 decoded B) | 4,096 → 512 | 17,301,504 → 2,621,440 |

The four-source baseline is four times the measured single-source baseline;
current four-source counts are measured. The separate real four-ZIP-link
128,000-selectors-per-source workload measures 1,024 calls / 1,040,384 SDK bytes.
The retained queue remains 64; batch capacity does not enlarge retained state.

Fatal numeric/source/head/body-nesting/damage issues still discard the whole profile.
Real I/O, allocation and cancellation failures propagate without publication.
A numeric failure prevents later sources from opening after the current member's
existing integrity workflow. Partial admission always carries issue bits; the
probe must not label it complete.

## CSS Syntax tokenization and recovery

The streaming scanner is an authored boundary-only transcription of
[CSS Syntax Level 3 §4, 1 October 2026](https://www.w3.org/TR/2026/CRD-css-syntax-3-20261001/#tokenization).
It emits each original byte exactly once, retaining SDK hashes and declaration
parsing. Its algorithm comments cite the corresponding spec sections:

| Spec algorithm | Boundary implementation |
| --- | --- |
| §4.3.1–2 token dispatch/comments | Eight-byte bounded lookahead; comments cannot emit boundaries; hash/at-keyword names and CDO/CDC retain token identity. |
| §4.3.3, §4.3.13 numeric tokens | Consume number extent and dimension names so `12url(` cannot become a URL token; numeric values remain SDK-owned. |
| §4.3.4, §4.3.12 ident-like/ident sequence | Decode escapes for case-insensitive URL recognition; retain raw escaped names for SDK compatibility. Quoted URLs follow function/string consumption. |
| §4.3.5 strings | Escaped newlines stay in strings; an unescaped CR/LF/FF terminates a bad string before the newline, which is reconsumed. |
| §4.3.6 URLs | Brackets/braces and slash-star are URL data; they cannot open a block or comment. |
| §4.3.7–10 escape/identifier/number starts | Up to six hex digits, optional whitespace code point, replacement for invalid escaped scalar; EOF and newline decisions survive every feed split. |
| §4.3.15 bad-URL remnants | Consume through the first unescaped `)` or EOF; quotes, comments and nested-looking brackets in remnants stay payload. |
| §5.5.9 simple blocks | Matching `() [] {}` stack only outside protected payload; top-level braces remain rule events owned by Admission. |

The §4.2 range controls include every allowed range/singleton boundary and
adjacent excluded points, under bare, dimension, hash and at-keyword contexts.
A separate 9,728-case Admission matrix verifies error issue publication across
13 malformed families × two sizes × 256 offsets (6,656 cases), plus 3,072
production Unicode controls. EOF comment/string/function/block/URL, bad
strings/URLs, backslash-newline, escaped EOF and block-stack overflow all report
`StyleTruncated|StyleCssSyntax`, while real neighboring rules survive.

The scanner core is non-template. Admission and ProjectionCheck retain separate
consumers through narrow call-local adapters for byte and optional lexical
notifications; the scanner never retains an adapter or consumer pointer. All
state and token-consumption branches remain unchanged from `9086f17`. A pinned
host `-Os` comparison with host instrumentation disabled reduces the production
style object's text from 21,540 to 18,500 bytes (3,040 bytes saved); two 2,363-byte
`Scanner::step` specializations become one 2,332-byte core. These object sizes
are attribution evidence; native image savings are recorded below.

The scanner has eight raw lookahead bytes and a 64-byte block stack, no heap and
no member-sized allocation. CRLF/CR/FF follow §3.3 newline semantics while raw
bytes remain intact. UTF-8 is decoded for the current §4.2 non-ASCII identifier ranges and token
identity; SDK selector hashes still receive original bytes. Malformed UTF-8
uses the maximal valid prefix as one replacement point, preserving each raw
span without rewriting the source. Twenty explicit malformed UTF-8 escape-span
cases cover truncated/interrupted prefixes, invalid leads and scalar ranges. This is not a full CSS parser:
number values, Unicode-range mode, token objects, CSSOM, selector matching,
property grammars and encoding detection are deliberately not implemented.
The existing numeric/NUL guard and pinned SDK remain authoritative for their
original responsibilities. A token parse error, EOF in a protected block, or
stack overflow reports nonfatal `StyleTruncated|StyleCssSyntax`; overflow
protects the remainder instead of guessing boundaries. Such sources are outside
the exact domain when CSS recovery changes SDK boundaries. Inside the exact
domain, tests assert `equal || issues` before the stronger exact-equality check,
so a silent divergence has its own explicit failure.

Round-4 production regressions total **14,336 EPUB cases**: fourteen URL forms ×
stored/deflated × small/8,226-byte × all 256 alignments. All retain authored
`p` zero-indent and neighboring `b` bold after URL consumption. Against
`290eb52`, nine forms fail at every alignment/method/size, including
`url(cover[1.png)`, `u\rl(x/*foo)`, `u\72l(x/*foo)`, `U\52L(x/*foo)` and
`url( x[ )`, plus nested-open/quoted/escaped-close bad-URL remnants.
`url(a]b)` and `url(a{b)` already pass on the old code; they remain explicit
sibling controls, not falsely claimed negative controls. Other passing siblings
exercise whitespace/invalid-escape remnants and quoted-string continuations.
The minimized independent old-scanner witness is `@url(/*`: an at-keyword must
not enter the ident-like URL path.

The independent Python oracle is written as a whole-input spec tokenizer with
Unicode code points and raw-byte spans. It neither imports nor translates the
C++ states. Its fixed-seed fuzzing uses hostile random byte sequences and
mutations of **52 maintained authored corpus sources**. Seeds are
`0xc5532026`, `0x7255524c`, `0x0bad5eed`: **15,360 generated/mutated cases**, plus
61 authored, 193 Unicode range-edge/context and 256 offset cases, totaling
**15,870 cases**. A 445-case sample
also runs at every possible feed split, including all 256 chunk alignments.
Per-byte protected classes, scanner error status and derived rule, selector-list
and declaration boundary events must match. Failures print a deterministic minimized raw-byte
counterexample. Elapsed time is diagnostic only; scheduling contention cannot
fail a correctness comparison. The deterministic oracle bounds below replace
both former wall-time assertions. Reproduce through `tests/reader_styles_test.py`
or run `tests/reader_css_tokenizer_test.py` alone with the managed interpreter.

The projection-output differential uses an independent Python significant-token
reference written from the spec, with decoded token values and raw byte spans.
It compares each normalized published selector/declaration with the original,
requires issue bits for rejected units, and cross-checks source and projected
protected classes against the existing independent Python boundary oracle.
It inserts comments at every adjacent token boundary in generated selectors
and declarations, plus hostile splits inside identifiers, numbers, dimensions,
escapes and function names. The fixed seeds are `0xc5532026`, `0x7255524c`,
`0x0bad5eed`. It covers **7,277 source cases, 27,430 source/projected units and
3,840 independent classifier cases**, including all 256 alignments for four
join/survivor shapes and 8,191/8,192/8,193/262,144-byte controls. Diagnostic
elapsed time includes native projection, the complete Python `_problem()`
verification loop and classifier comparison; it has no pass/fail threshold.
Failures still print minimized raw-byte witnesses. The separate production ZIP regressions add 4,096 cases:
four shapes × stored/deflated × small/8,226-byte × all 256 alignments. The
first shape fails against actual `5a1c75a` production at all 1,024 combinations
(`.safep` gets 200%, issues zero); the positive `p/**/.safe` control keeps 200%.
The other two shapes enforce whole-declaration diagnostics and safe neighbours.

### Deterministic differential work

`tests/reader_css_test_work.py` counts test-only reference primitives: each code
point inspection, each byte painted, token-list visits/copies, preprocessing and rule/list/event byte
visits, and bytes submitted to UTF-8 decode attempts (including malformed-input
retries). The independent oracles share accounting only; their tokenization
and recovery decisions are unchanged. Each case uses a counter snapshot, so
corpus generation and unrelated calls between comparisons do not affect it.
Bounds are checked per input, rather than letting padding in another case
subsidize excess work. The extra one in each formula covers empty inputs.

| Comparison | Per-case ceiling | Measured total / total ceiling | Tightest case / ceiling | Minimum headroom |
| --- | --- | ---: | ---: | ---: |
| Projection oracle | `128 * (source bytes + 1)` | 38,293,790 / 240,378,240 | 2,811 / 5,888 | 52.3% |
| Tokenizer, normal | `32 * (bytes + 1)` | 8,393,852 / 22,563,200 | 1,700 / 2,848 | 40.3% |
| Tokenizer, every split | `32 * (bytes + 1)` | 541,867 / 1,576,992 | 1,484 / 3,008 | 50.7% |
| Source/projected classifier | `32 * (bytes + 1)` | 18,616,880 / 53,387,232 | 14,915 / 41,760 | 64.3% |
| Comment-separated trivia control | `128 * (bytes + 1)` | 102,416 / 2,621,696 | 102,416 / 2,621,696 | 96.1% |

The normal/every-split routes retain all native split reconstruction and
correctness assertions. Case counts, seeds and generated inputs are unchanged;
explicit assertions now pin 7,277 projection cases, 27,430 units, 3,840
classifier inputs, 15,870 normal tokenizer cases, 445 every-split cases and
15,360 generated/mutated cases. Existing C++ retention/SDK work bounds remain
separate and unchanged. These are bounds on measured reference primitives,
not CPU instructions or a universal guarantee about arbitrary new algorithms.

The oracle's previous leading-trivia trim used repeated `pop(0)`, which
shifted the rest of the token list quadratically when comments separated
whitespace tokens. It now finds trim indices and slices once, preserving the
same tokens. Accounting conservatively charges token-list construction, trim
visits, the slice and subsequent traversals. A separate 4,096-comment trivia
control returns exactly the authored `p` token in 102,416 steps; replaying the
former front-pop algorithm requires 8,390,656 slot shifts and trips its
2,621,696-step bound. This control adds coverage without changing the existing
corpus counts or seeds. A fresh independent review also compared 55,987
generated trivia/token combinations with the prior trim semantics; all match.

A real quadratic negative control tokenizes the same 152-byte authored sheet
once per source byte and asserts identical results on every pass. Its 186,048
operations exceed both the projection ceiling (19,584) and tokenizer ceiling
(4,896). Removing the accounting or disabling its check makes the negative
control fail. Existing selector-concatenation and old-scanner correctness
negative controls also remain.

The standalone affected tests pass: projection in 19.40s (11.493s comparison),
tokenizer in 3.66s (1.982s comparison). Times are informative only; the full-gate
diagnostics are recorded below. In the final full gate, the projection comparison
passes at 15.616s (10.301s oracle/native plus 5.315s classifier) and the tokenizer
comparison passes at 3.422s. Work counts exactly match the standalone runs,
independently of that timing difference. A source audit finds no remaining
NEWT-4 wall-clock pass/fail assertion.

### Hang supervision boundary

The local gate runner has **no automatic phase deadline**. Its
[`Children.run`](../tools/gate_runner.py#L105) waits in `communicate()` without
a timeout, and [`supervise`](../tools/gate_runner.py#L230) waits on the result
queue without one. Thus an autonomous local hang timeout cannot be confirmed.
Once an external interrupt or supervision error reaches the runner,
[`Children.stop`](../tools/gate_runner.py#L119) sends SIGTERM then SIGKILL with
at most five seconds of reaping per stage, and
[`stop`](../tools/gate_runner.py#L309) permits a one-second epilogue: an
11-second bounded shutdown. Existing `tests/gate_stop_test.py` covers stubborn
phases and descendants. The separate
[CI job limit](../.github/workflows/fresh-gates.yml#L27) is 75 minutes; it is not
a local per-phase deadline. The runner and CI configuration are unchanged in
this test-only round.

## Regression corpus and controls

The generated corpus contains **970,258 cases**: **796,937 exact-reference
cases (82.1%)** and **173,321 outside-domain cases (17.9%)**. Exact cases are
786,432 unique generated safe sheets (128 selector forms × 24 declaration
forms × 256 offsets), 8,457 production ZIP SDK comparisons,
512 baseline numeric/NUL rejections and 1,536 declaration-fragment reference
comparisons. Generated exact sheets explicitly fit the old 8,192-byte/64-rule
bounds. The independent oracle visits 2,195,615 raw SDK selector members.

Outside-domain coverage is 18,185 ZIP cases, 152,064 protected-payload matrix
cases and 3,072 declaration-fragment cases. Reasons overlap: 66,052 have raw
SDK selector context, 164,360 protected boundary risk, 19,205 selector comments,
and 61,442 exceed the small-source byte bound. Counts include every matrix
case; no separate matrix is hidden from the exact/outside totals.

The protected matrix is six payload kinds (comments, double/single strings,
quoted/unquoted URLs and escapes) × 11 delimiters (`{ } , ; /* */ " ' ( ) \`)
× selector/declaration/list position × natural/forced/large source × every
256-byte chunk offset. Literal `*/` terminates a comment; its body variant is
encoded rather than falsely treating following syntax as still protected.
All cases assert that real p/b/i neighbors keep zero/bold/italic declarations
and phantom selectors never apply. Quoted and unquoted URL behavior are tested
separately, including literal slash-star in the latter.

Another 4,608 cases cover protected strings/functions/escapes in values and
property names. `p{font-weight:"x"bold}` remains unset naturally, after an
unrelated rejected selector forces projection, and in a large source.
Non-comment payload bytes are retained unchanged or their whole declaration
is dropped with an issue; masking must never manufacture `bold`, `italic`, an
indent or a property name. Valid neighbour declarations survive. Comments
are discarded only when significant token identity is preserved.

A further 722 selector tokens are compared with the actual private SDK parser:
306 accepts, 342 rejects, and 74 independently classified escaped-boundary
cases that the safe policy projects. Empty rule counts 0/1/63/64, UTF-8,
escaped identifiers, BOMs, CRLF, whitespace, at-rules/nesting, `!important`,
source boundaries and former/current byte limits remain covered.

Reproduce with the managed interpreter and `tests/reader_styles_test.py`.
The four named production ZIP controls independently fail on `8722323`: small
and 8,226-byte selector comments publish three rules and give unrelated p a
200% indent; string/comment bodies publish a phantom `.x`. Their corrected
results and forced-projection twins pass. The counted-work checks independently
fail on the old implementation and pass on the current one.

The wider host matrix still covers explicit/zero/overridden indentation,
supported/unsupported mixed selector lists, heading adjacency, large sheets,
nested inline styles, paragraph and page boundaries, cache/reopen, 64/65 and
512/513 rule cases, late I/O, truncated deflate, source order, numeric limits,
arena teardown and retained-order determinism. The baseline selector and
large-sheet negative controls fail on `73c5ed0`; the policy-identity control
fails if identity 10 is reset. `tests/chapter_page_cache_test.py` rejects cached
layouts from policy identities 5, 6, 7, 8 and 9 and verifies reopen parity.

## Neutral private admission evidence

Production ReaderStyles and an identical Admission replay were run before and
after on at most 64 distinct spine members per book. Replay verifies retained
hashes/declarations before reporting truncation. No private path, title,
selector, CSS, text or digest was exported.

| Neutral book | Chapters | Baseline applied/limited | Candidate applied/limited | Kept baseline → candidate | Truncated rules | Partial chapters | Combinator-rejected chapters |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 64 | 64/0 | 64/0 | 2,880 → 2,880 | 0 | 0 | 0 |
| B | 41 | 1/40 | 41/0 | 1 → 2,561 | 520 | 40 | 0 |
| C | 39 | 1/38 | 39/0 | 9 → 2,441 | 6,952 | 38 | 38 |

This is host admission evidence, not body-nesting, raster or device acceptance.
The permitted read-only integration contract is in
[probe style verdicts](epub-probe-styles.md).

## Verification and native resources

Verification uses the managed, already-approved project toolchain. No
resources, exclusions or selected guards are enlarged to obtain a pass.

| Check | Latest evidence |
| --- | --- |
| Affected CSS differentials | rc 0; projection 19.40s, tokenizer 3.66s (round 7) |
| Quick Linux suite | rc 0, 43.29s (round 6) |
| Full Linux host gate | Round 7: rc 0, 206.73s; ASan/UBSan and all 20 reviewed renderer frames |
| Four clean native health builds | Round 6: all four compile; 91.06s total; lanes rc 1 solely for flash ceilings. Round 7 changes tests/docs only; no firmware rebuild or resource record |
| Flash resource ratchet/adoption | Not passed: all four images exceed the unchanged reviewed flash ceilings; Lead adoption remains required after committing/reviewing the candidate |

Round 7 changes only tests and documentation. Code-health output is byte-for-byte
identical to the output captured at `e4990b5` before editing, and production,
build-driver and workflow inputs have no diff. The native values below are
unchanged round-6 evidence, carried forward without a new build or resource
record. The final fresh bounded test review is clean. An earlier full host run
passed in 220.23s before the oracle trim/accounting correction; the table records
the complete rerun after that correction.

The unchanged `73c5ed0` native baseline is retained for delta calculation:

| Variant | Baseline flash | Baseline RAM | Selected measurements |
| --- | ---: | ---: | ---: |
| default | 1,473,184 | 133,248 | 314 |
| dev-usb | 1,611,040 | 134,080 | 343 |
| regional | 1,479,840 | 133,744 | 316 |
| regional-dev-usb | 1,619,664 | 134,576 | 350 |

| Variant | `9086f17` flash | Current flash | Delta vs `73c5ed0` | Delta vs `9086f17` | RAM / RTC delta vs both |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,484,192 | 1,482,160 | +8,976 | −2,032 | 0 / 0 |
| dev-usb | 1,622,048 | 1,620,032 | +8,992 | −2,016 | 0 / 0 |
| regional | 1,490,832 | 1,488,816 | +8,976 | −2,016 | 0 / 0 |
| regional-dev-usb | 1,630,672 | 1,628,656 | +8,992 | −2,016 | 0 / 0 |

All baseline-selected symbols remain measured (314/343/316/350), with no frame
changes versus `9086f17`. Versus `73c5ed0`, the protected `addText` frame is
48 → 0 bytes and every other selected frame is unchanged; `ReaderStyles::build`
stays 144 bytes. These are
individual compiler frames, not runtime call-chain or stack-peak evidence. RAM
and RTC totals remain identical to baseline for each variant. Dirty build IDs
are included in these image sizes; the Lead must regenerate committed receipts.
None of the four final images contains a zero-address padding segment.

The new non-template core symbols are currently unguarded. Their measured C3
static frames are identical across all four variants, for explicit guard review:

| Shared core function (`const Sink&`) | Static frame |
| --- | ---: |
| `step` | 64 B (each former specialization was also 64 B) |
| `block` | 48 B |
| `emit`, `escaped` | 32 B each |
| `feed`, `finish` | 16 B each |
| `notice`, `beginEscape`, `lexeme` | 0 B |

All four adapter callbacks have zero-byte static frames. No new frame budget,
resource record or ceiling is written in this round. These individual frames
do not establish runtime call-chain peaks.

No resource record or ceiling was changed. A fresh bounded adversarial and
maintainability review of the exact shared-core and timer delta is clean.
Before editing, all four clean `9086f17` builds compiled in 91.50s and failed
only their unchanged flash ceilings. Direct ASan/UBSan comparisons with
captured `9086f17` binaries match all 7,277 projection observations (normalized
bytes, selector ranges, issue bits and rule counts), all 15,870 tokenizer
observations, and the 445 every-split cases, in 2.004s. With the SDK, numeric
guard, publication logic and hash folding unchanged, identical scanner and
projected inputs preserve rule order, declarations and content hashes. The
970,258-case SDK corpus and existing source/cache/work controls still pass.
Policy identity remains `0x4e53540a`; the source digest receives an append-only
same-policy entry for this behavior-preserving refactor.

The generated application partition is 7,798,784 bytes. The protected
sector-rounded reserve is 782,336 bytes, leaving a **7,016,448-byte usable
limit**. Resource results must compare every image and reviewed ceiling with
7,016,448; 7,798,784 is partition capacity and must never be called usable.
RAM and selected-stack ceilings must remain unchanged or stricter.

## Evidence still required

Policy 10 awaits reviewed resource adoption and owner acceptance. It lacks physical X3
evidence for paragraph fidelity, wraps, page continuations and cache/reopen.
Owner review of new reader rasters and macOS ImageIO evidence are not
established. A passing host test or native compile cannot stand in for any of
those facts.
