# Reader opening progress and cooperative deadline

The prior installed candidate passed the primary reading/resume prefix but
produced no response to sibling activation within the fixed 30-second command
deadline. The bridge had queued `ACCEPTED` and immediately dispatched a
synchronous opening before its next poll could send that record. Silence could
not distinguish delivery failure from an opening that had already begun.

The correction preserves the existing consumption gate. After it commits a
queued ticket, a bounded progress drain sends `ACCEPTED` and `CONSUMED` before
application dispatch. Every poll is gated by the exact consumed, unrendered
ticket: it can service TX/notifications but cannot read another command or
produce terminal completion. The drain yields and checks its one-second
absolute budget before polling and after driver return. Disconnect, stalled
output, or a late write cancels the request and skips dispatch. No retry is
performed. Terminal completion remains after actual dispatch and painting.
Consumption describes the queue event, not successful book opening.

Only the outer `BookOpenScope` starts a 20-second opening budget. Nested scopes
cannot extend it. Timeout remains sticky until the outer scope unwinds and is
separate from queued physical Sleep counts. Wrap-safe clock checks use the
existing source cancellation hook, before file operations and after reads.
The firmware also checks before publishing a staged reader frame. Rejection
uses the existing selection/checkpoint/settings rollback; timeout feedback says
that the position was kept. No book bytes, identities, admission sizes, parser
arenas, selection schema, or catalog lifecycle change.

Development-only, bounded phase logs identify opening paint, selection/hash,
progress loading, reader opening, and return. They contain no filenames or book
identities. They require 64 bytes of serial space and emit at most 49 bytes,
after protocol acknowledgements have drained. The host diagnostics patch keeps
write completion, validated reply count, and the underlying failure cause.

The deadline is cooperative. The pinned driver contains blocking locks and
busy-wait paths, so a hook cannot interrupt a driver call that has not returned.
This change does not certify a hard 20-second physical bound. The original
30-second terminal deadline, full 23-stage acceptance journey, and actual
ready/open/progress checks remain required. Early progress is not acceptance.

Source fault tests cover one-byte/zero/late writes, disconnect, no new RX or
false terminal during progress, late writes across clock wrap, nested timeout
ownership, queued Sleep preservation, and real compressed-reader rollback after
late Sleep or deadline cancellation in both orientations. Native cost and
fresh hardware evidence are separate gates.

Independent adversarial and maintainability reviews explicitly accept the sole
file-size exception from 608 to 615 lines in `firmware/main.cpp`: two includes,
four phase-trace owner hooks, and one acknowledgement integration. The control,
transport progress, and bounded trace logic live in their small headers. No
generic ceiling, generated exclusion, dependency adapter, selected native
frame, or SDK pin is relaxed. Native resource adoption must be measured and
reviewed separately before freezing a corrected image proposal.

The coordinator-directed observation after the failed run attached through the
established serial route and captured a USB-UART reset. The new boot verified
the existing build and stable ready Home with reader closed and opening false.
It cannot establish the prior activation's completion. The failed raw capture
and receipt remain immutable and private. No new source candidate has been
flashed.
