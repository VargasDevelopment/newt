# Visible opening feedback and physical cancellation

This isolated change starts at physical baseline metadata `c505be3`. It adds a
canonical shared working screen before long book verification/admission and
lets physical Back cancel the active opening. It changes no hash, admission,
page-cache or reader-engine algorithm and integrates no automatic covers.
The installed X3 remains the previously tested `f22f72044de2` image, with latest
observed primary/sibling anchors 3442/1600 before the owner resumed reading.
Those anchors may now advance; any future device work needs fresh evidence.
No device, USB or card action occurs
in this source phase; all prejourney backups remain historical.

## Display contract

`renderBookOpeningUi()` uses the existing Frosted system backdrop, wordmark,
Inter atlas, opaque card and footer. It says “Opening your book”, explains that
the book is being checked and the page restored, and shows Back/Cancel. It has
no percentage, fabricated progress or animation. The system screen is portrait
under either saved reader orientation; opening preserves that reader setting.
Host preview is [book-opening.png](../artifacts/book-opening.png).
[Decision 075](075-slow-action-feedback.md) later rewords the explanation
and keeps this screen for first opens only.

The outer `BookOpenScope` owns exactly one working paint before source hashing
or reader admission. A nested `openReader()` does not repaint it. The existing
blocking display buffer is used, with no third framebuffer or allocation. The
working frame replaces the prior UI while opening; staged reader publication
remains atomic. The display receipt records the completed working refresh but
does not certify a reader page, wake restoration or reader stack observation.
Only the final completed reader paint can do that.

## Input and checkpoint contract

The input task records cancellation only. Back queues an atomic token only if
opening is active and tags that physical queue event. Power keeps its existing
queued cancellation and sleep behavior. The same count survives scope unwind
until consumption, and a failed queue insertion compensates exactly its token.
Neither producer touches a file, parser, progress store or UI destination.
Synthetic USB events retain their wire button/ticket contract and own no
physical cancellation token.

The existing source and parser hooks see cancellation at their bounded work
boundaries before the staged reader opening passes its final acceptance check.
The UI owner closes/unwinds the candidate and restores prior book
identity, settings, checkpoint, Home projection and durable slots. A tagged
Back consumed after an aborted Library open leaves the user on that shelf;
Home Read uses the existing Back route. Every tagged event exits before
ordinary action mapping; repeated cancellation cannot trigger Home Read again
or become extra Library navigation. The final opening acceptance check is the
cancellation cutoff: for Library this is the outer callback's cancellation check
after staged publication and before selected-identity writes. A Back queued
after that check acts on the accepted Reading state. It does not
roll back the accepted page or a checkpoint transaction already in progress.
The checkpoint owner still commits Reading intent last, after verified selection
and progress writes. The existing 20-second cooperative deadline remains.
Cancellation cannot interrupt a blocked SDK driver call, the initial blocking
refresh or rollback directory scanning. USB polling stays synchronous; this
change promises physical Back cancellation, not concurrent USB commands.

## Source review and verification

Fresh independent adversarial and thermonuclear maintainability reviews are
CLEAN in two rounds. The owner found and closed a repeated cancellation
fallthrough after the first round; both fresh second-round reviews pass the
final consumer. Both explicitly approve only main.cpp 631→644: two owner
includes, four lines at the existing paint owner, one outer-open paint check
and six lines establishing one shared consumer after DEV ticket ownership. No
generic line threshold, adapter, exclusion or native resource margin changes
follow from that structural allowance.

Authored host controls execute the actual producer/consumer statements and
test immediate consumption, failed insertion, physical/synthetic separation
and nested scope ownership. The real SD lifecycle injects Back during full SHA
and verifies complete prior identity/checkpoint and durable-slot rollback.
Actual compressed-reader tests cover Back, Power and timeout in admission,
metadata and layout under both orientations, with retry, unchanged checkpoint,
frame rollback and no live XML allocation. Actual firmware paint tests compare
the shared working frame and native bytes and verify it cannot certify a first
reader page; the nested open paints only once. Existing sleep rejection expects
the new working paint followed by its final reader paint.

Native image/RAM/stack costs, full local gate and final source/image identities
must be recorded after measurement and review. Source/preview/compile evidence
does not establish physical visibility, ghosting, button latency, battery or
owner M1 acceptance. Cold/warm book-open latency remains a separate blocker;
this feedback change does not claim to resolve it. The parent coordinates the
separate SDK cache/reuse assessment and any new exact-image device grant.
